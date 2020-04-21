/*
 * I/O APIC
 */

use arch::target_arch::paging::PAGE_SIZE;

use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::{MemoryOptionFlags, MemoryPermissionFlags};

pub struct IoApicManager {
    base_address: usize,
}

impl IoApicManager {
    pub const fn new() -> IoApicManager {
        IoApicManager { base_address: 0 }
    }

    pub fn init(&mut self) {
        match get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .memory_remap(
                0xfec00000,
                PAGE_SIZE, /* is it ok?*/
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
            ) {
            Ok(address) => {
                self.base_address = address;
            }
            Err(err) => {
                panic!("Cannot reserve memory of IO APIC: {}", err);
            }
        };
    }

    pub fn set_redirect(&self, local_apic_id: u32, irq: u8, index: u8) {
        //tmp
        let mut table = unsafe { self.read_register(0x10 + (irq as u32) * 2) };
        table &= 0x00fffffffffe0000u64;
        table |= ((local_apic_id as u64) << 56) | index as u64;
        unsafe { self.write_register(0x10 + (irq as u32) * 2, table) };
    }

    unsafe fn read_register(&self, index: u32) -> u64 {
        use core::ptr::{read_volatile, write_volatile};
        write_volatile(self.base_address as *mut u32, index);
        let mut result = read_volatile((self.base_address + 0x10) as *mut u32) as u64;
        write_volatile(self.base_address as *mut u32, index + 1);
        result |= (read_volatile((self.base_address + 0x10) as *mut u32) as u64) << 32;
        result
    }

    unsafe fn write_register(&self, index: u32, data: u64) {
        use core::ptr::write_volatile;
        write_volatile(self.base_address as *mut u32, index);
        write_volatile((self.base_address + 0x10) as *mut u32, data as u32);
        write_volatile(self.base_address as *mut u32, index + 1);
        write_volatile((self.base_address + 0x10) as *mut u32, (data >> 32) as u32);
    }
}
