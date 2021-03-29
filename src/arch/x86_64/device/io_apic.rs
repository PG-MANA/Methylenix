//!
//! I/O APIC Manager
//!
//! Manager to control I/O APIC
//! I/O APIC is located at 0xfec00000 on the default.
//! It is used to set redirect to each cpu.
//!

use crate::arch::target_arch::paging::PAGE_SIZE;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};

pub struct IoApicManager {
    base_address: VAddress,
}

impl IoApicManager {
    /// Create IoApicManager with invalid address.
    ///
    /// Before use, **you must call [`init`]**.
    ///
    /// [`init`]: #method.init
    pub const fn new() -> IoApicManager {
        IoApicManager {
            base_address: VAddress::new(0),
        }
    }

    /// Init this manager.
    ///
    /// This function calls memory_manager.mmap_dev()
    /// This will panic when mmap_dev() was failed.
    pub fn init(&mut self) {
        match get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .mmap_dev(
                PAddress::new(0xfec00000),
                PAGE_SIZE, /* is it ok?*/
                MemoryPermissionFlags::data(),
                Some(MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS),
            ) {
            Ok(address) => {
                self.base_address = address;
            }
            Err(e) => {
                panic!("Cannot reserve memory of IO APIC Err:{:?}", e);
            }
        };
    }

    /// Set the specific device interruption to the specific cpu.
    ///
    /// local_apic_id: the local apic id(identify cpu core) to redirect interrupt
    /// irq: target device's irq
    /// index: The index of IDT vector table to accept the interrupt
    pub fn set_redirect(&self, local_apic_id: u32, irq: u8, index: u8) {
        //tmp
        let mut table = unsafe { self.read_register(0x10 + (irq as u32) * 2) };
        table &= 0x00fffffffffe0000u64;
        table |= ((local_apic_id as u64) << 56) | index as u64;
        unsafe { self.write_register(0x10 + (irq as u32) * 2, table) };
    }

    /// Read I/O register.
    unsafe fn read_register(&self, index: u32) -> u64 {
        use core::ptr::{read_volatile, write_volatile};
        write_volatile(self.base_address.to_usize() as *mut u32, index);
        let mut result = read_volatile((self.base_address.to_usize() + 0x10) as *mut u32) as u64;
        write_volatile(self.base_address.to_usize() as *mut u32, index + 1);
        result |= (read_volatile((self.base_address.to_usize() + 0x10) as *mut u32) as u64) << 32;
        result
    }

    /// Write I/O register.
    unsafe fn write_register(&self, index: u32, data: u64) {
        use core::ptr::write_volatile;
        write_volatile(self.base_address.to_usize() as *mut u32, index);
        write_volatile(
            (self.base_address.to_usize() + 0x10) as *mut u32,
            data as u32,
        );
        write_volatile(self.base_address.to_usize() as *mut u32, index + 1);
        write_volatile(
            (self.base_address.to_usize() + 0x10) as *mut u32,
            (data >> 32) as u32,
        );
    }
}
