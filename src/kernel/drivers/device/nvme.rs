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

pub struct NvmeManager {
    controller_properties_base_address: VAddress,
    #[allow(dead_code)]
    controller_properties_size: MSize,
    admin_queue: Queue,
    stride: usize,
}

struct Queue {
    submit_queue: VAddress,
    completion_queue: VAddress,
    id: usize,
    submission_current_pointer: u16,
    completion_current_pointer: u16,
    number_of_queue_entries: u16,
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
        //let max_queue = ((controller_capability & Self::CAP_MQES) >> Self::CAP_MQES_OFFSET) as u16;

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

        let num_of_queue: u16 = 64;
        let admin_queue_attributes: u32 = ((num_of_queue as u32) << 16) | (num_of_queue as u32);
        write_mmio::<u32>(
            controller_properties_base_address,
            Self::CONTROLLER_PROPERTIES_ADMIN_QUEUE_ATTRIBUTES,
            admin_queue_attributes,
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
            num_of_queue,
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

        let mut command = [0u32; 16];
        command[0] = 0x06 | (0x1 << 16);
        command[1] = 0xffffffff;
        command[4] = (identify_info_physical_address.to_usize() & (u32::MAX as usize)) as u32;
        command[5] = (identify_info_physical_address.to_usize() >> 32) as u32;
        command[6] = (identify_info_physical_address.to_usize() & (u32::MAX as usize)) as u32;
        command[7] = (identify_info_physical_address.to_usize() >> 32) as u32;
        command[10] = 1;
        nvme_manager.submit_admin_command(command);
        nvme_manager.wait_completion_of_admin_command_by_spin(0x01);
        while read_mmio::<u16>(identify_info_virtual_address, 0) == 0 {}
        let result = nvme_manager.take_completed_admin_command();
        pr_debug!("Identify command is finished, Result: {:#X?}", result);
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

        let _ = get_kernel_manager_cluster()
            .kernel_memory_manager
            .free(identify_info_virtual_address);
        pr_debug!("All initialize is finished.");

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
    //const CAP_MQES_OFFSET: u64 = 0;
    //const CAP_MQES: u64 = 0xffff << Self::CAP_MQES_OFFSET;
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
        }
    }

    fn submit_admin_command(&mut self, command: [u32; 16]) {
        assert_ne!(command[0] >> 16, 0);
        write_mmio::<[u32; 16]>(
            self.admin_queue.submit_queue,
            (self.admin_queue.submission_current_pointer as usize)
                * core::mem::size_of::<[u32; 16]>(),
            command,
        );
        let mut next_pointer = self.admin_queue.submission_current_pointer + 1;
        if next_pointer >= self.admin_queue.number_of_queue_entries {
            next_pointer = 0;
        }
        write_mmio::<u32>(
            self.controller_properties_base_address,
            Self::PCIE_SPECIFIC_DEFINITIONS_BASE + (2 * self.admin_queue.id) * self.stride,
            next_pointer as u32,
        );
        self.admin_queue.submission_current_pointer = next_pointer;
    }

    fn wait_completion_of_admin_command_by_spin(&self, command_id: u16) {
        while (read_mmio::<[u32; 4]>(
            self.admin_queue.completion_queue,
            (self.admin_queue.completion_current_pointer as usize)
                * core::mem::size_of::<[u32; 4]>(),
        )[3] & 0xffff) as u16
            != command_id
        {
            core::hint::spin_loop()
        }
    }

    fn submit_command(&self, queue: &mut Queue, command: [u32; 16]) {
        assert_ne!(command[0] >> 16, 0);
        write_mmio::<[u32; 16]>(
            queue.submit_queue,
            (queue.submission_current_pointer as usize) * core::mem::size_of::<[u32; 16]>(),
            command,
        );
        let mut next_pointer = queue.submission_current_pointer + 1;
        if next_pointer >= queue.number_of_queue_entries {
            next_pointer = 0;
        }
        write_mmio::<u32>(
            self.controller_properties_base_address,
            Self::PCIE_SPECIFIC_DEFINITIONS_BASE + (2 * queue.id) * self.stride,
            next_pointer as u32,
        );
        queue.submission_current_pointer = next_pointer;
    }

    fn wait_completion_of_command_by_spin(&self, queue: &Queue, command_id: u16) {
        while (read_mmio::<[u32; 4]>(
            queue.completion_queue,
            (queue.completion_current_pointer as usize) * core::mem::size_of::<[u32; 4]>(),
        )[3] & 0xffff) as u16
            != command_id
        {
            core::hint::spin_loop()
        }
    }

    fn take_completed_command(&self, queue: &mut Queue) -> [u32; 4] {
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
        if queue.completion_current_pointer >= queue.number_of_queue_entries {
            queue.completion_current_pointer = 0;
        }
        return data;
    }

    fn take_completed_admin_command(&mut self) -> [u32; 4] {
        let data = read_mmio::<[u32; 4]>(
            self.admin_queue.completion_queue,
            (self.admin_queue.completion_current_pointer as usize)
                * core::mem::size_of::<[u32; 4]>(),
        );
        write_mmio::<[u32; 4]>(
            self.admin_queue.completion_queue,
            (self.admin_queue.completion_current_pointer as usize)
                * core::mem::size_of::<[u32; 4]>(),
            [0; 4],
        );
        self.admin_queue.completion_current_pointer += 1;
        if self.admin_queue.completion_current_pointer >= self.admin_queue.number_of_queue_entries {
            self.admin_queue.completion_current_pointer = 0;
        }
        return data;
    }
}

impl Queue {
    pub const fn new(
        submit_queue: VAddress,
        completion_queue: VAddress,
        id: usize,
        number_of_queue_entries: u16,
    ) -> Self {
        Self {
            submit_queue,
            completion_queue,
            id,
            submission_current_pointer: 0,
            completion_current_pointer: 0,
            number_of_queue_entries,
        }
    }
}

fn read_mmio<T: Sized>(base: VAddress, offset: usize) -> T {
    unsafe { core::ptr::read_volatile((base.to_usize() + offset) as *const T) }
}

fn write_mmio<T: Sized>(base: VAddress, offset: usize, data: T) {
    unsafe { core::ptr::write_volatile((base.to_usize() + offset) as *mut T, data) }
}
