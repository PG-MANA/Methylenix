//!
//! NVMe Driver
//!

use crate::arch::target_arch::device::pci::nvme::setup_interrupt;

use crate::kernel::drivers::pci::{ClassCode, PciDevice, PciDeviceDriver, PciManager};
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};

use crate::{alloc_pages_with_physical_address, io_remap};

use alloc::vec::Vec;

pub struct NvmeManager {
    controller_properties_base_address: VAddress,
    #[allow(dead_code)]
    controller_properties_size: MSize,
    admin_queue: Queue,
    stride: usize,
    namespace_id: u32, /* Temporary */
    io_queue_list: Vec<Queue>,
}

struct Queue {
    submit_queue: VAddress,
    completion_queue: VAddress,
    id: usize,
    submission_current_pointer: u16,
    completion_current_pointer: u16,
    number_of_completion_queue_entries: u16,
    number_of_submission_queue_entries: u16,
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

    fn setup_device(pci_dev: &PciDevice, class_code: ClassCode) {
        if class_code.programming_interface != 2 && class_code.programming_interface != 3 {
            pr_err!(
                "Unsupported programming interface: {:#X}",
                class_code.programming_interface
            );
            return;
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
                        return;
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
                    return;
                }
            };
        }

        let base_address_0 = read_pci!(PciManager::PCI_BAR_0, 4);
        if base_address_0 & 0x01 != 0 {
            pr_err!("Expected MMIO");
            return;
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
            return;
        }

        let controller_properties_base_address = controller_property_base_address.unwrap();
        let version = read_mmio::<u32>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_VERSION,
        );
        if version > ((2 << 16) | (0 << 8)) {
            pr_err!("Unsupported NVMe version: {:#X}", version);
            return;
        }

        let controller_capability = read_mmio::<u64>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_CAPABILITIES,
        );
        /*let memory_page_max =
        ((controller_capability & Self::CAP_MPS_MAX) >> Self::CAP_MPS_MAX_OFFSET) as u8;*/
        let memory_page_min =
            ((controller_capability & Self::CAP_MPS_MIN) >> Self::CAP_MPS_MIN_OFFSET) as u8;
        let stride = 4usize
            << ((controller_capability & Self::CAP_DOOR_BELL_STRIDE)
                >> Self::CAP_DOOR_BELL_STRIDE_OFFSET);
        let max_queue =
            ((controller_capability & Self::CAP_MQES) >> Self::CAP_MQES_OFFSET) as u16 + 1;

        if memory_page_min > 0 {
            pr_err!("4KiB Memory Page is not supported.");
            return;
        }
        if (((controller_capability & Self::CAP_CSS) >> Self::CAP_CSS_OFFSET) & (1 << 7)) != 0 {
            pr_err!("I/O command set is not supported.");
            //return;
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
            return;
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
                    return;
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
                    return;
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
                "The number of queue entries is exceeded max_queue size({}",
                max_queue
            );

            let _ = get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(admin_completion_queue_virtual_address);
            let _ = get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(admin_submission_queue_virtual_address);
            return;
        }

        write_mmio::<u32>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_ADMIN_QUEUE_ATTRIBUTES,
            (admin_completion_queue_size as u32) << 16 | (admin_submission_queue_size as u32),
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

        /* Set Controller Configuration and Enable */
        write_mmio::<u32>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_CONFIGURATION,
            (completion_queue_entry_size << 20)
                | (submission_queue_entry_size << 16)
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

        if let Err(e) = setup_interrupt(pci_dev) {
            pr_debug!("Failed to setup interrupt: {:?}", e);
            return;
        }

        let admin_queue = Queue::new(
            admin_submission_queue_virtual_address,
            admin_completion_queue_virtual_address,
            0,
            admin_completion_queue_size,
            admin_submission_queue_size,
        );
        let mut nvme_manager = NvmeManager::new(
            controller_properties_base_address,
            controller_properties_map_size,
            admin_queue,
            stride,
        );

        let (identify_info_virtual_address, identify_info_physical_address) = match alloc_pages_with_physical_address!(
            MSize::new(0x1000).to_order(None).to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(a) => a,
            Err(e) => {
                pr_err!("Failed to alloc memory for the admin queue: {:?}", e);
                return;
            }
        };

        let mut command_id = 0x01;

        nvme_manager.submit_identify_command(
            command_id,
            identify_info_physical_address,
            IdentifyCommandCNS::IdentifyControllerDataStructure,
            0,
        );
        nvme_manager.wait_completion_of_admin_command_by_spin(command_id);
        let result = nvme_manager.take_completed_admin_command();
        pr_debug!(
            "Identify command is finished, Result: {:#X?}(Status: {:#X})",
            result,
            (result[3] >> 16) & !1
        );
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

        command_id += 1;
        nvme_manager.submit_identify_command(
            command_id,
            identify_info_physical_address,
            IdentifyCommandCNS::ActiveNamespaceIdList,
            0x00,
        );
        nvme_manager.wait_completion_of_admin_command_by_spin(command_id);
        let result = nvme_manager.take_completed_admin_command();
        pr_debug!(
            "Identify command is finished, Result: {:#X?}(Status: {:#X})",
            result,
            (result[3] >> 16) & !1
        );
        let nsid_table =
            unsafe { &*(identify_info_virtual_address.to_usize() as *const [u32; 0x1000 / 4]) };
        for nsid in nsid_table {
            if *nsid == 0 {
                break;
            }
            pr_debug!("Active NSID: {:#X}", *nsid);
        }
        if nsid_table[0] == 0 {
            pr_err!("There is no usable name space");
            let _ = get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(identify_info_virtual_address);
            let _ = get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(admin_completion_queue_virtual_address);
            let _ = get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(admin_submission_queue_virtual_address);
            return;
        }
        nvme_manager.add_name_space_id(nsid_table[0]);

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
                return;
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
                return;
            }
        };

        command_id += 1;
        let num_of_completion_queue_entries =
            (io_queue_size.to_usize() / 2usize.pow(completion_queue_entry_size)) as u16;
        if num_of_completion_queue_entries > max_queue {
            pr_err!("Invalid Queue Size");
        }
        nvme_manager.submit_create_completion_command(
            command_id,
            io_completion_queue_physical_address,
            num_of_completion_queue_entries,
            0x01,
            0,
            false,
        );
        nvme_manager.wait_completion_of_admin_command_by_spin(command_id);
        let result = nvme_manager.take_completed_admin_command();
        pr_debug!(
            "Create completion queue command is finished, Result: {:#X?}(Status: {:#X})",
            result,
            (result[3] >> 16) & !1
        );

        command_id += 1;
        let num_of_submission_queue_entries =
            (io_queue_size.to_usize() / 2usize.pow(submission_queue_entry_size)) as u16;
        if num_of_submission_queue_entries > max_queue {
            pr_err!("Invalid Queue Size");
        }
        nvme_manager.submit_create_submission_command(
            command_id,
            io_submission_queue_physical_address,
            num_of_submission_queue_entries as u16,
            0x01,
            0x01,
            0,
        );
        nvme_manager.wait_completion_of_admin_command_by_spin(command_id);
        let result = nvme_manager.take_completed_admin_command();
        pr_debug!(
            "Create submission queue command is finished, Result: {:#X?}(Status: {:#X})",
            result,
            (result[3] >> 16) & !1
        );

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

        /* Read Test */
        let (prp_list_virtual_address, prp_list_physical_address) = match alloc_pages_with_physical_address!(
            MSize::new(0x1000).to_order(None).to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(a) => a,
            Err(e) => {
                pr_err!("Failed to alloc memory for the  PRP List: {:?}", e);
                let _ = get_kernel_manager_cluster()
                    .kernel_memory_manager
                    .free(identify_info_virtual_address);
                return;
            }
        };
        unsafe {
            core::ptr::write_bytes(prp_list_virtual_address.to_usize() as *mut u8, 0, 0x1000)
        };
        unsafe {
            *(prp_list_virtual_address.to_usize() as *mut u64) =
                identify_info_physical_address.to_usize() as u64
        };

        let mut command = [0u32; 16];
        command[0] = 0x02 | ((0x01 as u32) << 16);
        command[1] = 0x01;
        unsafe {
            *(core::mem::transmute::<&mut u32, &mut u64>(&mut command[6])) =
                identify_info_physical_address.to_usize() as u64
        };
        unsafe {
            *(core::mem::transmute::<&mut u32, &mut u64>(&mut command[8])) =
                prp_list_physical_address.to_usize() as u64
        };
        command[10] = 0; /* LBA[0:32] */
        command[12] = 0x1000 / 512 - 1;
        nvme_manager
            .submit_command(0x01, command)
            .expect("Failed to submit command");
        nvme_manager
            .wait_completion_of_command_by_spin(0x01, 0x01)
            .expect("Failed to wait command");
        let result = nvme_manager
            .take_completed_command(0x01)
            .expect("Failed to get result");
        pr_debug!(
            "Data read command is finished,  Result: {:#X?}(Status: {:#X})",
            result,
            (result[3] >> 16) & !1
        );
        pr_debug!("Data: {:#X?}", unsafe {
            &*(identify_info_virtual_address.to_usize() as *const [u8; 8])
        });
        let mut len = 0;
        for e in unsafe { *(identify_info_virtual_address.to_usize() as *const [u8; 256]) } {
            if e == 0 {
                break;
            }

            len += 1;
        }
        if len != 256 {
            pr_debug!(
                "Text: {}",
                core::str::from_utf8(unsafe {
                    core::slice::from_raw_parts(
                        identify_info_virtual_address.to_usize() as *const u8,
                        len,
                    )
                })
                .unwrap_or("Failed to convert")
            );
        }

        let _ = get_kernel_manager_cluster()
            .kernel_memory_manager
            .free(prp_list_virtual_address);
        let _ = get_kernel_manager_cluster()
            .kernel_memory_manager
            .free(identify_info_virtual_address);
        return;
    }
}

impl NvmeManager {
    const CONTROLLER_PROPERTIES_DEFAULT_MAP_SIZE: MSize = MSize::new(0x2000);
    const CONTROLLER_PROPERTIES_CAPABILITIES: usize = 0x00;
    //const CAP_MPS_MAX_OFFSET: u64 = 52;
    //const CAP_MPS_MAX: u64 = 0b1111 << Self::CAP_MPS_MAX_OFFSET;
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
            namespace_id: 0,
            io_queue_list: Vec::new(),
        }
    }

    /// Temporary
    fn add_name_space_id(&mut self, name_space_id: u32) {
        self.namespace_id = name_space_id;
    }

    fn _submit_command(
        base_address: VAddress,
        stride: usize,
        queue: &mut Queue,
        command: [u32; 16],
    ) {
        assert_ne!(command[0] >> 16, 0);
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
    }

    fn submit_command(&mut self, queue_id: u16, command: [u32; 16]) -> Result<(), ()> {
        if queue_id as usize > self.io_queue_list.len() || queue_id == 0 {
            return Err(());
        }
        return Ok(Self::_submit_command(
            self.controller_properties_base_address,
            self.stride,
            &mut self.io_queue_list[queue_id as usize - 1],
            command,
        ));
    }

    fn submit_admin_command(&mut self, command: [u32; 16]) {
        return Self::_submit_command(
            self.controller_properties_base_address,
            self.stride,
            &mut self.admin_queue,
            command,
        );
    }

    fn _wait_completion_of_command_by_spin(queue: &Queue, command_id: u16) {
        while (read_mmio::<[u32; 4]>(
            queue.completion_queue,
            (queue.completion_current_pointer as usize) * core::mem::size_of::<[u32; 4]>(),
        )[3] & 0xffff) as u16
            != command_id
        {
            core::hint::spin_loop()
        }
    }

    fn wait_completion_of_command_by_spin(&self, queue_id: u16, command_id: u16) -> Result<(), ()> {
        if queue_id as usize > self.io_queue_list.len() || queue_id == 0 {
            return Err(());
        }
        return Ok(Self::_wait_completion_of_command_by_spin(
            &self.io_queue_list[queue_id as usize - 1],
            command_id,
        ));
    }

    fn wait_completion_of_admin_command_by_spin(&self, command_id: u16) {
        Self::_wait_completion_of_command_by_spin(&self.admin_queue, command_id);
    }

    fn _take_completed_admin_command(queue: &mut Queue) -> [u32; 4] {
        let data = read_mmio::<[u32; 4]>(
            queue.completion_queue,
            (queue.completion_current_pointer as usize) * core::mem::size_of::<[u32; 4]>(),
        );
        write_mmio::<[u32; 4]>(
            queue.completion_queue,
            (queue.completion_current_pointer as usize) * core::mem::size_of::<[u32; 4]>(),
            [0; 4],
        );
        queue.completion_current_pointer += 1;
        if queue.completion_current_pointer >= queue.number_of_completion_queue_entries {
            queue.completion_current_pointer = 0;
        }
        return data;
    }

    fn take_completed_command(&mut self, queue_id: u16) -> Result<[u32; 4], ()> {
        if queue_id as usize > self.io_queue_list.len() || queue_id == 0 {
            return Err(());
        }
        let queue = &mut self.io_queue_list[queue_id as usize - 1];
        return Ok(Self::_take_completed_admin_command(queue));
    }

    fn take_completed_admin_command(&mut self) -> [u32; 4] {
        return Self::_take_completed_admin_command(&mut self.admin_queue);
    }

    fn submit_identify_command(
        &mut self,
        id: u16,
        output_physical_address: PAddress,
        cns: IdentifyCommandCNS,
        namespace_id: u32,
    ) {
        let mut command = [0u32; 16];
        command[0] = Self::QUEUE_COMMAND_IDENTIFY | ((id as u32) << 16);
        command[1] = namespace_id;
        unsafe {
            *(core::mem::transmute::<&mut u32, &mut u64>(&mut command[6])) =
                output_physical_address.to_usize() as u64
        };
        command[10] = cns as u32;
        self.submit_admin_command(command);
    }

    fn submit_create_completion_command(
        &mut self,
        command_id: u16,
        queue_physical_address: PAddress,
        queue_size: u16,
        queue_id: u16,
        interrupt_vector: u16,
        interrupts_enabled: bool,
    ) {
        let mut command = [0u32; 16];
        command[0] = Self::QUEUE_COMMAND_CREATE_IO_COMPLETION_QUEUE | ((command_id as u32) << 16);
        unsafe {
            *(core::mem::transmute::<&mut u32, &mut u64>(&mut command[6])) =
                queue_physical_address.to_usize() as u64
        };
        command[10] = ((queue_size as u32 - 1) << 16) | (queue_id as u32);
        command[11] = ((interrupt_vector as u32) << 16) | ((interrupts_enabled as u32) << 1) | 1;
        self.submit_admin_command(command);
    }

    fn submit_create_submission_command(
        &mut self,
        command_id: u16,
        queue_physical_address: PAddress,
        queue_size: u16,
        completion_queue_id: u16,
        submission_queue_id: u16,
        queue_priority: u8,
    ) {
        let mut command = [0u32; 16];
        command[0] = Self::QUEUE_COMMAND_CREATE_IO_SUBMISSION_QUEUE | ((command_id as u32) << 16);
        unsafe {
            *(core::mem::transmute::<&mut u32, &mut u64>(&mut command[6])) =
                queue_physical_address.to_usize() as u64
        };
        command[10] = ((queue_size as u32 - 1) << 16) | (submission_queue_id as u32);
        command[11] = ((completion_queue_id as u32) << 16) | ((queue_priority as u32) << 1) | 1;
        self.submit_admin_command(command);
    }

    fn add_io_queue(&mut self, queue: Queue) -> Result<(), ()> {
        if self.io_queue_list.len() + 1 != queue.id {
            pr_err!("Invalid I/O Queue List");
            return Err(());
        }
        self.io_queue_list.push(queue);
        return Ok(());
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
            submit_queue,
            completion_queue,
            id,
            submission_current_pointer: 0,
            completion_current_pointer: 0,
            number_of_completion_queue_entries,
            number_of_submission_queue_entries,
        }
    }
}

fn read_mmio<T: Sized>(base: VAddress, offset: usize) -> T {
    unsafe { core::ptr::read_volatile((base.to_usize() + offset) as *const T) }
}

fn write_mmio<T: Sized>(base: VAddress, offset: usize, data: T) {
    unsafe { core::ptr::write_volatile((base.to_usize() + offset) as *mut T, data) }
}
