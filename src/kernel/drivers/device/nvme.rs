//!
//! NVMe Driver
//!

use crate::arch::target_arch::interrupt::InterruptManager;
use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE_USIZE};

use crate::kernel::block_device::{
    BlockDeviceDescriptor, BlockDeviceDriver, BlockDeviceError, BlockDeviceInfo,
};
use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::drivers::pci::{
    msi::setup_msi_or_msi_x, ClassCode, PciDevice, PciDeviceDriver, PciManager,
};
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::{
    alloc_pages_with_physical_address,
    data_type::{
        Address, MIndex, MPageOrder, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
        VAddress,
    },
    free_pages, io_remap, kfree, kmalloc,
};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;
use crate::kernel::task_manager::{TaskStatus, ThreadEntry};

use core::mem::offset_of;

use alloc::collections::LinkedList;
use alloc::vec::Vec;

pub struct NvmeManager {
    controller_properties_base_address: VAddress,
    #[allow(dead_code)]
    controller_properties_size: MSize,
    admin_queue: Queue,
    stride: usize,
    namespace_list: Vec<NameSpace>,
    io_queue_list: Vec<Queue>,
}

struct Queue {
    lock: IrqSaveSpinLockFlag,
    submit_queue: VAddress,
    completion_queue: VAddress,
    id: usize,
    submission_current_pointer: u16,
    completion_current_pointer: u16,
    next_command_id: u16,
    number_of_completion_queue_entries: u16,
    number_of_submission_queue_entries: u16,
    wait_list: PtrLinkedList<WaitListEntry>,
}

struct WaitListEntry {
    list: PtrLinkedListNode<Self>,
    result: [u32; 4],
    thread: &'static mut ThreadEntry,
}

#[derive(Clone)]
struct NameSpace {
    id: u32,
    number_of_lba_blocks: u64,
    lba_block_size_exp: u8,
}

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
enum IdentifyCommandCNS {
    NameSpace = 0x00,
    IdentifyControllerDataStructure = 0x01,
    ActiveNamespaceIdList = 0x02,
}

impl PciDeviceDriver for NvmeManager {
    const BASE_CLASS_CODE: u8 = 0x01;
    const SUB_CLASS_CODE: u8 = 0x08;

    fn setup_device(pci_dev: &PciDevice, class_code: ClassCode) -> Result<(), ()> {
        if class_code.programming_interface != 2 && class_code.programming_interface != 3 {
            pr_err!(
                "Unsupported programming interface: {:#X}",
                class_code.programming_interface
            );
            return Err(());
        }
        macro_rules! read_pci {
            ($offset:expr, $size:expr) => {
                match get_kernel_manager_cluster()
                    .pci_manager
                    .read_data(pci_dev, $offset, $size)
                {
                    Ok(d) => d,
                    Err(e) => {
                        pr_err!("Failed to read PCI configuration space: {:?},", e);
                        return Err(());
                    }
                }
            };
        }
        macro_rules! write_pci {
            ($offset:expr, $data:expr) => {
                if let Err(e) = get_kernel_manager_cluster()
                    .pci_manager
                    .write_data(pci_dev, $offset, $data)
                {
                    pr_err!("Failed to read PCI configuration space: {:?},", e);
                    return Err(());
                }
            };
        }

        let base_address_0 = read_pci!(PciManager::PCI_BAR_0, 4);
        if base_address_0 & 0x01 != 0 {
            pr_err!("Expected MMIO");
            return Err(());
        }
        let is_64bit_bar_address = ((base_address_0 >> 1) & 0b11) == 0b10;
        let base_address = (base_address_0 & !0b1111) as usize
            | if is_64bit_bar_address {
                (read_pci!(PciManager::PCI_BAR_1, 4) as usize) << 32
            } else {
                0
            };

        let mut command_status = read_pci!(PciManager::PCI_CONFIGURATION_COMMAND, 4);
        command_status &= !PciManager::COMMAND_INTERRUPT_DISABLE_BIT;
        command_status |= PciManager::COMMAND_MEMORY_SPACE_BIT | PciManager::COMMAND_BUS_MASTER_BIT;
        write_pci!(PciManager::PCI_CONFIGURATION_COMMAND, command_status);

        let controller_properties_map_size = Self::CONTROLLER_PROPERTIES_DEFAULT_MAP_SIZE;
        let controller_property_base_address = io_remap!(
            PAddress::new(base_address),
            controller_properties_map_size,
            MemoryPermissionFlags::data()
        );
        if let Err(e) = controller_property_base_address {
            pr_err!("Failed to map NVMe Controller Properties: {:?}", e);
            return Err(());
        }

        let controller_properties_base_address = controller_property_base_address.unwrap();
        let version = read_mmio::<u32>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_VERSION,
        );
        if version > ((2 << 16) | (0 << 8)) {
            pr_err!("Unsupported NVMe version: {:#X}", version);
            return Err(());
        }

        let controller_capability = read_mmio::<u64>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_CAPABILITIES,
        );
        let memory_page_max =
            ((controller_capability & Self::CAP_MPS_MAX) >> Self::CAP_MPS_MAX_OFFSET) as u8;
        let memory_page_min =
            ((controller_capability & Self::CAP_MPS_MIN) >> Self::CAP_MPS_MIN_OFFSET) as u8;
        let stride = 4usize
            << ((controller_capability & Self::CAP_DOOR_BELL_STRIDE)
                >> Self::CAP_DOOR_BELL_STRIDE_OFFSET);
        let max_queue =
            ((controller_capability & Self::CAP_MQES) >> Self::CAP_MQES_OFFSET) as u16 + 1;

        if (memory_page_min as usize + 12) > PAGE_SHIFT
            || (memory_page_max as usize + 12) < PAGE_SHIFT
        {
            pr_err!("Controller is not supported of the host memory page size.");
            return Err(());
        }
        if (((controller_capability & Self::CAP_CSS) >> Self::CAP_CSS_OFFSET) & (1 << 7)) != 0 {
            pr_warn!("I/O command set is not supported.");
        }

        let controller_configuration = read_mmio::<u32>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_CONFIGURATION,
        );
        let completion_queue_entry_size =
            if ((controller_configuration & Self::CC_IOCQES) >> Self::CC_IOCQES_OFFSET) == 0 {
                pr_debug!("Completion Queue Entry Size is zero, assume as 2^4");
                4
            } else {
                (controller_configuration & Self::CC_IOCQES) >> Self::CC_IOCQES_OFFSET
            };
        let submission_queue_entry_size =
            if ((controller_configuration & Self::CC_IOSQES) >> Self::CC_IOSQES_OFFSET) == 0 {
                pr_debug!("Submission Queue Entry Size is zero, assume as 2^6");
                6
            } else {
                (controller_configuration & Self::CC_IOSQES) >> Self::CC_IOSQES_OFFSET
            };

        if submission_queue_entry_size != 6 || completion_queue_entry_size != 4 {
            pr_err!(
                "Unsupported queue size(Submission: 2^{}, Completion: 2^{})",
                submission_queue_entry_size,
                completion_queue_entry_size
            );
            return Err(());
        }

        if (controller_configuration & Self::CC_ENABLE) != 0 {
            /* Reset */
            write_mmio::<u32>(
                controller_properties_base_address,
                Self::CONTROLLER_PROPERTIES_CONFIGURATION,
                controller_configuration & !Self::CC_ENABLE,
            );

            while (read_mmio::<u32>(
                controller_properties_base_address,
                Self::CONTROLLER_PROPERTIES_STATUS,
            ) & Self::CSTS_READY)
                != 0
            {
                core::hint::spin_loop()
            }
        }
        /* Setup Admin Queue */
        let queue_size: MSize = MSize::new(0x1000);
        let (admin_submission_queue_virtual_address, admin_submission_queue_physical_address) =
            match alloc_pages_with_physical_address!(
                queue_size.to_order(None).to_page_order(),
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::DEVICE_MEMORY
            ) {
                Ok(a) => a,
                Err(e) => {
                    pr_err!("Failed to alloc memory for the admin queue: {:?}", e);
                    return Err(());
                }
            };
        let (admin_completion_queue_virtual_address, admin_completion_queue_physical_address) =
            match alloc_pages_with_physical_address!(
                queue_size.to_order(None).to_page_order(),
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::DEVICE_MEMORY
            ) {
                Ok(a) => a,
                Err(e) => {
                    pr_err!("Failed to alloc memory for the admin queue: {:?}", e);
                    let _ = free_pages!(admin_submission_queue_virtual_address);
                    return Err(());
                }
            };
        /* Zero clear admin completion queue */
        unsafe {
            core::ptr::write_bytes(
                admin_completion_queue_virtual_address.to_usize() as *mut u8,
                0,
                queue_size.to_usize(),
            )
        };

        let admin_completion_queue_size: u16 =
            (queue_size.to_usize() / 2usize.pow(completion_queue_entry_size)) as u16;
        let admin_submission_queue_size: u16 =
            (queue_size.to_usize() / 2usize.pow(submission_queue_entry_size)) as u16;
        if admin_completion_queue_size > max_queue || admin_submission_queue_size > max_queue {
            pr_err!(
                "The number of queue entries is exceeded max_queue size({})",
                max_queue
            );

            let _ = free_pages!(admin_completion_queue_virtual_address);
            let _ = free_pages!(admin_submission_queue_virtual_address);
            return Err(());
        }

        write_mmio::<u32>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_ADMIN_QUEUE_ATTRIBUTES,
            (admin_completion_queue_size as u32 - 1) << 16
                | (admin_submission_queue_size as u32 - 1),
        );
        write_mmio::<u64>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_ADMIN_SUBMISSION_QUEUE_BASE_ADDRESS,
            admin_submission_queue_physical_address.to_usize() as u64,
        );
        write_mmio::<u64>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_ADMIN_COMPLETION_QUEUE_BASE_ADDRESS,
            admin_completion_queue_physical_address.to_usize() as u64,
        );

        let admin_queue = Queue::new(
            admin_submission_queue_virtual_address,
            admin_completion_queue_virtual_address,
            0,
            admin_completion_queue_size,
            admin_submission_queue_size,
        );
        let nvme_manager = match kmalloc!(
            NvmeManager,
            NvmeManager::new(
                controller_properties_base_address,
                controller_properties_map_size,
                admin_queue,
                stride,
            )
        ) {
            Ok(n) => n,
            Err(e) => {
                pr_err!("Failed to allocate memory for NVMe manager: {:?}", e);
                return Err(());
            }
        };

        if let Err(e) = nvme_manager.setup_interrupt(pci_dev) {
            pr_debug!("Failed to setup interrupt: {:?}", e);
            let _ = free_pages!(admin_completion_queue_virtual_address);
            let _ = free_pages!(admin_submission_queue_virtual_address);
            return Err(());
        }

        /* Set Controller Configuration and Enable */
        write_mmio::<u32>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_CONFIGURATION,
            (completion_queue_entry_size << 20)
                | (submission_queue_entry_size << 16)
                | (PAGE_SHIFT as u32 - 12) << 7
                | Self::CC_ENABLE,
        );
        while (read_mmio::<u32>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_STATUS,
        ) & Self::CSTS_READY)
            == 0
        {
            core::hint::spin_loop()
        }

        let (identify_info_virtual_address, identify_info_physical_address) = match alloc_pages_with_physical_address!(
            MSize::new(0x1000).to_order(None).to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(a) => a,
            Err(e) => {
                pr_err!("Failed to alloc memory for the admin queue: {:?}", e);
                return Err(());
            }
        };

        let command_id = nvme_manager.submit_identify_command(
            identify_info_physical_address,
            IdentifyCommandCNS::IdentifyControllerDataStructure,
            0,
        );
        if let Err(e) = nvme_manager
            .wait_completion_of_admin_command_by_spin(command_id, Self::SPIN_WAIT_TIMEOUT_MS)
        {
            pr_err!("Failed to wait the command: {:?}", e);
            let _ = free_pages!(identify_info_virtual_address);
            return Err(());
        }
        let result = nvme_manager.take_completed_admin_command();
        if !Self::is_command_successful(&result) {
            pr_err!(
                "Identify command is failed, Result: {:#X?}(Status: {:#X})",
                result,
                (result[3] >> 16) & !1
            );
            let _ = free_pages!(identify_info_virtual_address);
            return Err(());
        }
        pr_debug!(
            "Vendor ID: {:#X}, SerialNumber: {}",
            unsafe {
                core::ptr::read_volatile(identify_info_virtual_address.to_usize() as *const u16)
            },
            core::str::from_utf8(unsafe {
                core::slice::from_raw_parts(
                    (identify_info_virtual_address.to_usize() + 4) as *mut u8,
                    19,
                )
            })
            .unwrap_or("Unknown")
        );

        let max_transfer_size =
            unsafe { *((identify_info_virtual_address.to_usize() + 77) as *const u8) };
        pr_debug!("Max Transfer Size: 2^{}", max_transfer_size);

        /* Add I/O Completion/Submission Queue */
        let io_queue_size = MSize::new(0x1000);
        let (io_submission_queue_virtual_address, io_submission_queue_physical_address) = match alloc_pages_with_physical_address!(
            io_queue_size.to_order(None).to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(a) => a,
            Err(e) => {
                pr_err!("Failed to alloc memory for the admin queue: {:?}", e);
                let _ = free_pages!(identify_info_virtual_address);
                return Err(());
            }
        };
        let (io_completion_queue_virtual_address, io_completion_queue_physical_address) = match alloc_pages_with_physical_address!(
            io_queue_size.to_order(None).to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(a) => a,
            Err(e) => {
                pr_err!("Failed to alloc memory for the admin queue: {:?}", e);
                let _ = free_pages!(identify_info_virtual_address);
                let _ = free_pages!(io_submission_queue_virtual_address);
                return Err(());
            }
        };

        let num_of_completion_queue_entries =
            (io_queue_size.to_usize() / 2usize.pow(completion_queue_entry_size)) as u16;
        if num_of_completion_queue_entries > max_queue {
            pr_err!("Invalid Queue Size");
            /* TODO: adjust queue size */
        }
        unsafe {
            core::ptr::write_bytes(
                io_completion_queue_virtual_address.to_usize() as *mut u8,
                0,
                io_queue_size.to_usize(),
            )
        };
        let command_id = nvme_manager.submit_create_completion_command(
            io_completion_queue_physical_address,
            num_of_completion_queue_entries,
            0x01,
            0x00,
            true,
        );
        if let Err(e) = nvme_manager
            .wait_completion_of_admin_command_by_spin(command_id, Self::SPIN_WAIT_TIMEOUT_MS)
        {
            pr_err!("Failed to wait the command: {:?}", e);
            let _ = free_pages!(identify_info_virtual_address);
            let _ = free_pages!(io_completion_queue_virtual_address);
            let _ = free_pages!(io_submission_queue_virtual_address);
            return Err(());
        }
        let result = nvme_manager.take_completed_admin_command();
        if !Self::is_command_successful(&result) {
            pr_err!(
                "Create completion queue command is is failed, Result: {:#X?}(Status: {:#X})",
                result,
                (result[3] >> 16) & !1
            );
            let _ = free_pages!(identify_info_virtual_address);
            let _ = free_pages!(io_completion_queue_virtual_address);
            let _ = free_pages!(io_submission_queue_virtual_address);
            return Err(());
        }

        let num_of_submission_queue_entries =
            (io_queue_size.to_usize() / 2usize.pow(submission_queue_entry_size)) as u16;
        if num_of_submission_queue_entries > max_queue {
            pr_err!("Invalid Queue Size");
            /* TODO: adjust queue size */
        }
        let command_id = nvme_manager.submit_create_submission_command(
            io_submission_queue_physical_address,
            num_of_submission_queue_entries as u16,
            0x01,
            0x01,
            0,
        );
        if let Err(e) = nvme_manager
            .wait_completion_of_admin_command_by_spin(command_id, Self::SPIN_WAIT_TIMEOUT_MS)
        {
            pr_err!("Failed to wait the command: {:?}", e);
            let _ = free_pages!(identify_info_virtual_address);
            let _ = free_pages!(io_completion_queue_virtual_address);
            let _ = free_pages!(io_submission_queue_virtual_address);
            return Err(());
        }
        let result = nvme_manager.take_completed_admin_command();
        if !Self::is_command_successful(&result) {
            pr_err!(
                "Create submission queue command is is failed, Result: {:#X?}(Status: {:#X})",
                result,
                (result[3] >> 16) & !1
            );
            let _ = free_pages!(identify_info_virtual_address);
            let _ = free_pages!(io_completion_queue_virtual_address);
            let _ = free_pages!(io_submission_queue_virtual_address);
            return Err(());
        }

        let io_queue = Queue::new(
            io_submission_queue_virtual_address,
            io_completion_queue_virtual_address,
            0x01,
            num_of_completion_queue_entries,
            num_of_submission_queue_entries,
        );
        nvme_manager
            .add_io_queue(io_queue)
            .expect("Failed to add I/O queue");

        let command_id = nvme_manager.submit_identify_command(
            identify_info_physical_address,
            IdentifyCommandCNS::ActiveNamespaceIdList,
            0x00,
        );
        if let Err(e) = nvme_manager
            .wait_completion_of_admin_command_by_spin(command_id, Self::SPIN_WAIT_TIMEOUT_MS)
        {
            pr_err!("Failed to wait the command: {:?}", e);
            let _ = free_pages!(identify_info_virtual_address);
            return Err(());
        }
        let result = nvme_manager.take_completed_admin_command();
        if !Self::is_command_successful(&result) {
            pr_err!(
                "Identify command is failed, Result: {:#X?}(Status: {:#X})",
                result,
                (result[3] >> 16) & !1
            );
            let _ = free_pages!(identify_info_virtual_address);
            return Err(());
        }

        let nsid_table =
            unsafe { &*(identify_info_virtual_address.to_usize() as *const [u32; 0x1000 / 4]) };
        for nsid in nsid_table {
            if *nsid == 0 {
                break;
            }
            pr_debug!("Active NSID: {:#X}", *nsid);
            match nvme_manager.detect_name_space(*nsid, false) {
                Ok(n) => {
                    nvme_manager.add_name_space(n);
                }
                Err(e) => {
                    pr_err!("Failed to detect Name Space {:#X}: {:?}", nsid, e);
                    continue;
                }
            }
            let name_space_index = nvme_manager.namespace_list.len() - 1;
            let descriptor = BlockDeviceDescriptor::new(name_space_index, nvme_manager as *mut _);
            get_kernel_manager_cluster()
                .block_device_manager
                .add_block_device(descriptor);
        }
        if nsid_table[0] == 0 {
            pr_err!("There is no usable name space");
            let _ = free_pages!(identify_info_virtual_address);
            let _ = free_pages!(admin_completion_queue_virtual_address);
            let _ = free_pages!(admin_submission_queue_virtual_address);
            return Ok(());
        }

        let _ = free_pages!(identify_info_virtual_address);
        return Ok(());
    }
}

impl BlockDeviceDriver for NvmeManager {
    fn read_data_lba(
        &mut self,
        info: &BlockDeviceInfo,
        buffer: VAddress,
        base_lba: u64,
        number_of_blocks: u64,
    ) -> Result<(), BlockDeviceError> {
        self._read_data_lba(
            0x01,
            info.device_id as u32,
            buffer,
            base_lba,
            number_of_blocks,
        )
    }

    fn get_lba_block_size(&self, info: &BlockDeviceInfo) -> u64 {
        1 << self.namespace_list[info.device_id].lba_block_size_exp
    }
}

impl NvmeManager {
    const CONTROLLER_PROPERTIES_DEFAULT_MAP_SIZE: MSize = MSize::new(0x2000);
    const CONTROLLER_PROPERTIES_CAPABILITIES: usize = 0x00;
    const CAP_MPS_MAX_OFFSET: u64 = 52;
    const CAP_MPS_MAX: u64 = 0b1111 << Self::CAP_MPS_MAX_OFFSET;
    const CAP_MPS_MIN_OFFSET: u64 = 48;
    const CAP_MPS_MIN: u64 = 0b1111 << Self::CAP_MPS_MIN_OFFSET;
    const CAP_CSS_OFFSET: u64 = 37;
    const CAP_CSS: u64 = (u8::MAX as u64) << Self::CAP_CSS_OFFSET;
    const CAP_DOOR_BELL_STRIDE_OFFSET: u64 = 32;
    const CAP_DOOR_BELL_STRIDE: u64 = 0b1111 << Self::CAP_DOOR_BELL_STRIDE_OFFSET;
    const CAP_MQES_OFFSET: u64 = 0;
    const CAP_MQES: u64 = 0xffff << Self::CAP_MQES_OFFSET;
    const CONTROLLER_PROPERTIES_VERSION: usize = 0x08;
    const CONTROLLER_PROPERTIES_CONFIGURATION: usize = 0x14;
    const CC_IOCQES_OFFSET: u32 = 20;
    const CC_IOCQES: u32 = 0b1111 << Self::CC_IOCQES_OFFSET;
    const CC_IOSQES_OFFSET: u32 = 16;
    const CC_IOSQES: u32 = 0b1111 << Self::CC_IOSQES_OFFSET;

    const CC_ENABLE: u32 = 1;
    const CONTROLLER_PROPERTIES_STATUS: usize = 0x1c;
    const CSTS_READY: u32 = 1;
    const CONTROLLER_PROPERTIES_ADMIN_QUEUE_ATTRIBUTES: usize = 0x24;
    const CONTROLLER_PROPERTIES_ADMIN_SUBMISSION_QUEUE_BASE_ADDRESS: usize = 0x28;
    const CONTROLLER_PROPERTIES_ADMIN_COMPLETION_QUEUE_BASE_ADDRESS: usize = 0x30;
    const PCIE_SPECIFIC_DEFINITIONS_BASE: usize = 0x1000;

    const QUEUE_COMMAND_CREATE_IO_SUBMISSION_QUEUE: u32 = 0x01;
    const QUEUE_COMMAND_CREATE_IO_COMPLETION_QUEUE: u32 = 0x05;
    const QUEUE_COMMAND_IDENTIFY: u32 = 0x06;

    const SPIN_WAIT_TIMEOUT_MS: usize = 1500;

    const fn new(
        controller_properties_base_address: VAddress,
        controller_properties_size: MSize,
        admin_queue: Queue,
        stride: usize,
    ) -> Self {
        Self {
            controller_properties_base_address,
            controller_properties_size,
            admin_queue,
            stride,
            namespace_list: Vec::new(),
            io_queue_list: Vec::new(),
        }
    }

    fn add_name_space(&mut self, name_space: NameSpace) {
        assert!(
            self.namespace_list
                .last()
                .and_then(|n| Some(n.id))
                .unwrap_or(0)
                < name_space.id
        );
        self.namespace_list.push(name_space);
    }

    pub fn setup_interrupt(&mut self, pci_dev: &PciDevice) -> Result<(), ()> {
        let interrupt_id = setup_msi_or_msi_x(pci_dev, nvme_handler, None, true)?;
        unsafe { NVME_LIST.push_back((interrupt_id, self as *mut _)) };
        return Ok(());
    }

    fn _read_completion_queue_head_doorbell(
        base_address: VAddress,
        stride: usize,
        queue: &Queue,
    ) -> u16 {
        (read_mmio::<u32>(
            base_address,
            Self::PCIE_SPECIFIC_DEFINITIONS_BASE + (2 * queue.id + 1) * stride,
        ) & 0xffff) as u16
    }

    fn _write_completion_queue_head_doorbell(
        base_address: VAddress,
        stride: usize,
        queue: &mut Queue,
        pointer: u16,
    ) {
        write_mmio::<u32>(
            base_address,
            Self::PCIE_SPECIFIC_DEFINITIONS_BASE + (2 * queue.id + 1) * stride,
            pointer as u32,
        )
    }

    fn _submit_command(
        base_address: VAddress,
        stride: usize,
        queue: &mut Queue,
        mut command: [u32; 16],
    ) -> u16 {
        if queue.next_command_id == 0 {
            queue.next_command_id = 1;
        }
        command[0] = (command[0] & 0xffff) | ((queue.next_command_id as u32) << 16);

        write_mmio::<[u32; 16]>(
            queue.submit_queue,
            (queue.submission_current_pointer as usize) * core::mem::size_of::<[u32; 16]>(),
            command,
        );
        let mut next_pointer = queue.submission_current_pointer + 1;
        if next_pointer >= queue.number_of_submission_queue_entries {
            next_pointer = 0;
        }
        write_mmio::<u32>(
            base_address,
            Self::PCIE_SPECIFIC_DEFINITIONS_BASE + (2 * queue.id) * stride,
            next_pointer as u32,
        );
        queue.submission_current_pointer = next_pointer;
        let command_id = queue.next_command_id;
        queue.next_command_id += 1;
        return command_id;
    }

    fn submit_admin_command(&mut self, command: [u32; 16]) -> u16 {
        return Self::_submit_command(
            self.controller_properties_base_address,
            self.stride,
            &mut self.admin_queue,
            command,
        );
    }

    fn _wait_completion_of_command_by_spin(
        queue: &Queue,
        command_id: u16,
        time_out_ms: usize,
    ) -> Result<(), ()> {
        let mut time = 0;
        while time < time_out_ms {
            if (read_mmio::<[u32; 4]>(
                queue.completion_queue,
                (queue.completion_current_pointer as usize) * core::mem::size_of::<[u32; 4]>(),
            )[3] & 0xffff) as u16
                == command_id
            {
                return Ok(());
            }
            if !get_kernel_manager_cluster()
                .global_timer_manager
                .busy_wait_ms(1)
            {
                return Err(());
            }
            time += 1;
        }
        return Err(());
    }

    fn wait_completion_of_admin_command_by_spin(
        &self,
        command_id: u16,
        timeout_ms: usize,
    ) -> Result<(), ()> {
        Self::_wait_completion_of_command_by_spin(&self.admin_queue, command_id, timeout_ms)
    }

    fn submit_command_and_wait(
        &mut self,
        queue_id: u16,
        command: [u32; 16],
    ) -> Result<[u32; 4], ()> {
        if queue_id as usize > self.io_queue_list.len() || queue_id == 0 {
            return Err(());
        }
        let irq = InterruptManager::save_and_disable_local_irq();
        let wait_list = match kmalloc!(
            WaitListEntry,
            WaitListEntry {
                list: PtrLinkedListNode::new(),
                result: [0u32; 4],
                thread: get_cpu_manager_cluster().run_queue.get_running_thread(),
            }
        ) {
            Ok(e) => e,
            Err(e) => {
                pr_err!("Failed to allocate wait list: {:?}", e);
                InterruptManager::restore_local_irq(irq);
                return Err(());
            }
        };
        let queue = &mut self.io_queue_list[queue_id as usize - 1];
        let _lock = queue.lock.lock();
        let command_id = Self::_submit_command(
            self.controller_properties_base_address,
            self.stride,
            queue,
            command,
        );
        wait_list.result[3] = command_id as u32;
        queue.wait_list.insert_tail(&mut wait_list.list);
        drop(_lock);
        get_cpu_manager_cluster()
            .run_queue
            .sleep_current_thread(Some(irq), TaskStatus::Interruptible)
            .or_else(|e| {
                pr_err!("Failed to sleep: {:#?}", e);
                let _ = kfree!(wait_list);
                Err(())
            })?;
        let result = wait_list.result;
        let _ = kfree!(wait_list);
        return Ok(result);
    }

    fn _take_completed_command(
        queue: &mut Queue,
        base_address: VAddress,
        stride: usize,
    ) -> [u32; 4] {
        let data = read_mmio::<[u32; 4]>(
            queue.completion_queue,
            (queue.completion_current_pointer as usize) * core::mem::size_of::<[u32; 4]>(),
        );
        /* Clear the command id and phase tag */
        write_mmio::<u32>(
            queue.completion_queue,
            (queue.completion_current_pointer as usize) * core::mem::size_of::<[u32; 4]>()
                + core::mem::size_of::<u32>() * 3,
            0,
        );
        queue.completion_current_pointer += 1;
        if queue.completion_current_pointer >= queue.number_of_completion_queue_entries {
            queue.completion_current_pointer = 0;
        }
        Self::_write_completion_queue_head_doorbell(
            base_address,
            stride,
            queue,
            queue.completion_current_pointer,
        );
        return data;
    }

    fn take_completed_admin_command(&mut self) -> [u32; 4] {
        return Self::_take_completed_command(
            &mut self.admin_queue,
            self.controller_properties_base_address,
            self.stride,
        );
    }

    fn submit_identify_command(
        &mut self,
        output_physical_address: PAddress,
        cns: IdentifyCommandCNS,
        namespace_id: u32,
    ) -> u16 {
        let mut command = [0u32; 16];
        command[0] = Self::QUEUE_COMMAND_IDENTIFY;
        command[1] = namespace_id;
        unsafe {
            *(core::mem::transmute::<&mut u32, &mut u64>(&mut command[6])) =
                output_physical_address.to_usize() as u64
        };
        command[10] = cns as u32;
        return self.submit_admin_command(command);
    }

    fn submit_create_completion_command(
        &mut self,
        queue_physical_address: PAddress,
        queue_size: u16,
        queue_id: u16,
        interrupt_vector: u16,
        interrupts_enabled: bool,
    ) -> u16 {
        let mut command = [0u32; 16];
        command[0] = Self::QUEUE_COMMAND_CREATE_IO_COMPLETION_QUEUE;
        unsafe {
            *(core::mem::transmute::<&mut u32, &mut u64>(&mut command[6])) =
                queue_physical_address.to_usize() as u64
        };
        command[10] = ((queue_size as u32 - 1) << 16) | (queue_id as u32);
        command[11] = ((interrupt_vector as u32) << 16) | ((interrupts_enabled as u32) << 1) | 1;
        return self.submit_admin_command(command);
    }

    fn submit_create_submission_command(
        &mut self,
        queue_physical_address: PAddress,
        queue_size: u16,
        completion_queue_id: u16,
        submission_queue_id: u16,
        queue_priority: u8,
    ) -> u16 {
        let mut command = [0u32; 16];
        command[0] = Self::QUEUE_COMMAND_CREATE_IO_SUBMISSION_QUEUE;
        unsafe {
            *(core::mem::transmute::<&mut u32, &mut u64>(&mut command[6])) =
                queue_physical_address.to_usize() as u64
        };
        command[10] = ((queue_size as u32 - 1) << 16) | (submission_queue_id as u32);
        command[11] = ((completion_queue_id as u32) << 16) | ((queue_priority as u32) << 1) | 1;
        return self.submit_admin_command(command);
    }

    fn add_io_queue(&mut self, queue: Queue) -> Result<(), ()> {
        if self.io_queue_list.len() + 1 != queue.id {
            pr_err!("Invalid I/O Queue List");
            return Err(());
        }
        self.io_queue_list.push(queue);
        return Ok(());
    }

    pub const fn is_command_successful(result: &[u32; 4]) -> bool {
        ((result[3] >> 16) & !1) == 0
    }

    fn detect_name_space(
        &mut self,
        name_space_id: u32,
        allow_sleep: bool,
    ) -> Result<NameSpace, ()> {
        let (identify_info_virtual_address, identify_info_physical_address) = match alloc_pages_with_physical_address!(
            MSize::new(0x1000).to_order(None).to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(a) => a,
            Err(e) => {
                pr_err!("Failed to alloc memory for the admin queue: {:?}", e);
                return Err(());
            }
        };
        let command_id = self.submit_identify_command(
            identify_info_physical_address,
            IdentifyCommandCNS::NameSpace,
            name_space_id,
        );
        if allow_sleep {
            unimplemented!()
        } else if let Err(e) =
            self.wait_completion_of_admin_command_by_spin(command_id, Self::SPIN_WAIT_TIMEOUT_MS)
        {
            pr_err!("Failed to wait the command: {:?}", e);
            let _ = free_pages!(identify_info_virtual_address);
            return Err(e);
        }
        let result = self.take_completed_admin_command();
        if !Self::is_command_successful(&result) {
            pr_err!(
                "Identify command is failed, Result: {:#X?}(Status: {:#X})",
                result,
                (result[3] >> 16) & !1
            );
            let _ = get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(identify_info_virtual_address);
            return Err(());
        }
        let name_space_number_of_lba_blocks =
            unsafe { *((identify_info_virtual_address.to_usize() + 0) as *const u64) };
        let formatted_lba_size =
            unsafe { *((identify_info_virtual_address.to_usize() + 26) as *const u8) };
        let lba_index =
            (((formatted_lba_size & (0b11 << 5)) >> 5) << 4) | (formatted_lba_size & 0b1111);
        pr_debug!(
            "LBA Index: {lba_index}(Formatted LBA Size: {:#X})",
            formatted_lba_size
        );
        let lba_format_info = unsafe {
            *((identify_info_virtual_address.to_usize() + 128 + (lba_index as usize) * 4)
                as *const u32)
        };
        let lba_block_size_exp = ((lba_format_info >> 16) & 0xff) as u8;
        pr_debug!("LBA Data Size: 2^{lba_block_size_exp}");
        let _ = get_kernel_manager_cluster()
            .kernel_memory_manager
            .free(identify_info_virtual_address);
        return Ok(NameSpace {
            id: name_space_id,
            number_of_lba_blocks: name_space_number_of_lba_blocks,
            lba_block_size_exp,
        });
    }

    fn _read_data_lba(
        &mut self,
        queue_id: u16,
        name_space_list_index: u32,
        buffer: VAddress,
        base_lba: u64,
        number_of_blocks: u64,
    ) -> Result<(), BlockDeviceError> {
        if number_of_blocks == 0 {
            pr_err!("Size is zero");
            return Err(BlockDeviceError::InvalidOperation);
        }
        if (buffer & !PAGE_MASK) != 0 {
            pr_err!("Buffer is not page aligned.");
            return Err(BlockDeviceError::InvalidBuffer);
        }

        if name_space_list_index as usize > self.namespace_list.len() {
            pr_err!(
                "Invalid name_space_list's index: {:#X}",
                name_space_list_index
            );
            return Err(BlockDeviceError::InvalidDevice);
        }
        let name_space = &self.namespace_list[name_space_list_index as usize];
        if (base_lba + (number_of_blocks << name_space.lba_block_size_exp))
            >= name_space.number_of_lba_blocks
        {
            pr_err!(
                "The staring LBA({:#X}) and the number of blocks({:#X}) are exceeded from the disk size",
                base_lba,
                number_of_blocks
            );
            return Err(BlockDeviceError::InvalidOperation);
        }

        let mut command = [0u32; 16];
        command[0] = 0x02;
        command[1] = 0x01;

        let mut pre_list_virtual_address: Option<VAddress> = None;
        let read_size = (number_of_blocks << name_space.lba_block_size_exp) as usize;
        if read_size <= PAGE_SIZE_USIZE * 2 {
            let num_of_pages = if read_size <= PAGE_SIZE_USIZE { 1 } else { 2 };
            let mut list = [PAddress::new(0); 2];
            let result = get_kernel_manager_cluster()
                .kernel_memory_manager
                .get_physical_address_list(
                    buffer,
                    MIndex::new(0),
                    MIndex::new(num_of_pages),
                    &mut list,
                );
            if let Err(err) = result {
                pr_err!("Failed to get physical address list: {:?}", err);
                return Err(BlockDeviceError::MemoryError(err));
            } else if result.unwrap() < num_of_pages {
                pr_err!("buffer is smaller than read size.");
                return Err(BlockDeviceError::InvalidBuffer);
            }

            *(unsafe { core::mem::transmute::<&mut u32, &mut u64>(&mut command[6]) }) =
                (list[0].to_usize() as u64).to_le();
            if num_of_pages == 2 {
                *(unsafe { core::mem::transmute::<&mut u32, &mut u64>(&mut command[8]) }) =
                    (list[1].to_usize() as u64).to_le()
            }
        } else {
            let (v, prp_list_physical_address) = match alloc_pages_with_physical_address!(
                MPageOrder::new(0),
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::DEVICE_MEMORY
            ) {
                Ok(a) => a,
                Err(err) => {
                    pr_err!("Failed to alloc memory for the  PRP List: {:?}", err);
                    return Err(BlockDeviceError::MemoryError(err));
                }
            };
            let list = unsafe {
                &mut *(v.to_usize()
                    as *mut [PAddress; PAGE_SIZE_USIZE / core::mem::size_of::<PAddress>()])
            };
            let result = get_kernel_manager_cluster()
                .kernel_memory_manager
                .get_physical_address_list(
                    buffer,
                    MIndex::new(0),
                    MSize::new(read_size).page_align_up().to_index(),
                    list,
                );
            if let Err(err) = result {
                pr_err!("Failed to get physical address list: {:?}", err);
                return Err(BlockDeviceError::MemoryError(err));
            } else if (result.unwrap() << PAGE_SHIFT) < read_size {
                pr_err!(
                    "Expected {:#X} bytes for buffer, but its size is {:#X} bytes",
                    read_size,
                    result.unwrap() << PAGE_SHIFT
                );
                let _ = free_pages!(v);
                return Err(BlockDeviceError::InvalidBuffer);
            }
            let prp1 = (list[0].to_usize() as u64).to_le();
            for i in 0..(result.unwrap() - 1) {
                list[i] = list[i + 1];
            }
            unsafe {
                *(core::mem::transmute::<&mut u32, &mut u64>(&mut command[6])) = prp1;
                *(core::mem::transmute::<&mut u32, &mut u64>(&mut command[8])) =
                    (prp_list_physical_address.to_usize() as u64).to_le();
            }
            pre_list_virtual_address = Some(v);
        }

        command[10] = (base_lba & u32::MAX as u64) as u32; /* LBA[0:31] */
        command[11] = (base_lba >> 32) as u32; /* LBA[32:63] */
        command[12] = (number_of_blocks - 1) as u32; /* [0:15]: Number of Logical Blocks */
        let result = self.submit_command_and_wait(queue_id, command);
        if result.is_err() {
            pr_err!("Failed to execute the command");
            return Err(BlockDeviceError::DeviceError);
        }

        let result = result.unwrap();
        if !Self::is_command_successful(&result) {
            pr_err!(
                "Failed the read command is failed:  {:#X?}(Status: {:#X})",
                result,
                (result[3] >> 16) & !1
            );
            if let Some(v) = pre_list_virtual_address {
                let _ = free_pages!(v);
            }
            return Err(BlockDeviceError::DeviceError);
        }
        if let Some(v) = pre_list_virtual_address {
            let _ = free_pages!(v);
        }
        return Ok(());
    }

    pub fn interrupt_handler(&mut self) {
        for queue in &mut self.io_queue_list {
            let _lock = queue.lock.lock();
            if (read_mmio::<u32>(
                queue.completion_queue,
                (queue.completion_current_pointer as usize) * core::mem::size_of::<[u32; 4]>()
                    + core::mem::size_of::<u32>() * 3,
            ) & (1 << 16))
                != 0
            {
                let data = Self::_take_completed_command(
                    queue,
                    self.controller_properties_base_address,
                    self.stride,
                );
                for e in unsafe { queue.wait_list.iter_mut(offset_of!(WaitListEntry, list)) } {
                    if (e.result[3] & 0xffff) == data[3] & 0xffff {
                        e.result = data;
                        if let Err(error) = get_kernel_manager_cluster()
                            .task_manager
                            .wake_up_thread(e.thread)
                        {
                            pr_err!("Failed to wake up the thread: {:?}", error);
                        }
                        queue.wait_list.remove(&mut e.list);
                        break;
                    }
                }
            }
        }
    }
}

impl Queue {
    pub const fn new(
        submit_queue: VAddress,
        completion_queue: VAddress,
        id: usize,
        number_of_completion_queue_entries: u16,
        number_of_submission_queue_entries: u16,
    ) -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            submit_queue,
            completion_queue,
            id,
            submission_current_pointer: 0,
            completion_current_pointer: 0,
            number_of_completion_queue_entries,
            number_of_submission_queue_entries,
            next_command_id: 0,
            wait_list: PtrLinkedList::new(),
        }
    }
}

fn read_mmio<T: Sized>(base: VAddress, offset: usize) -> T {
    unsafe { core::ptr::read_volatile((base.to_usize() + offset) as *const T) }
}

fn write_mmio<T: Sized>(base: VAddress, offset: usize, data: T) {
    unsafe { core::ptr::write_volatile((base.to_usize() + offset) as *mut T, data) }
}

static mut NVME_LIST: LinkedList<(usize, *mut NvmeManager)> = LinkedList::new();

fn nvme_handler(index: usize) -> bool {
    if let Some(nvme) = unsafe {
        NVME_LIST
            .iter()
            .find(|x| (**x).0 == index)
            .and_then(|x| Some(x.1.clone()))
    } {
        unsafe { &mut *(nvme) }.interrupt_handler();
        true
    } else {
        pr_err!("Unknown NVMe Device");
        false
    }
}
