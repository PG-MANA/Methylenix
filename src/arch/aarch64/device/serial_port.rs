//!
//! Serial Port Devices
//!

use crate::arch::target_arch::paging::PAGE_SIZE;

use crate::io_remap;
use crate::kernel::drivers::acpi::table::spcr::SpcrManager;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
};
use crate::kernel::sync::spin_lock::SpinLockFlag;

/// Dummy putc Function
fn dummy_putc(_: usize, c: u8) {
    /* Temporary QEMU */
    unsafe { core::ptr::write_volatile(0x9000000 as *mut u8, c) };
}

/// Dummy putc Function
fn dummy_getc(_: usize) -> Option<u8> {
    None
}

/// Dummy wait for buffer function
fn dummy_wait_buffer(_: usize) -> bool {
    /* Temporary QEMU */
    while (unsafe { core::ptr::read_volatile((0x9000000 + 0x018) as *const u16) } & (1 << 5)) != 0 {
        core::hint::spin_loop()
    }

    true
}

pub struct SerialPortManager {
    lock: SpinLockFlag,
    base_address: usize,
    putc_func: fn(base_address: usize, char: u8),
    #[allow(dead_code)]
    getc_func: fn(base_address: usize) -> Option<u8>,
    wait_buffer: fn(base_address: usize) -> bool,
}

impl SerialPortManager {
    pub fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            base_address: 0,
            putc_func: dummy_putc,
            getc_func: dummy_getc,
            wait_buffer: dummy_wait_buffer,
        }
    }

    pub fn init_with_acpi(&mut self) -> bool {
        let _lock = self.lock.lock();
        let spcr_manager = get_kernel_manager_cluster()
            .acpi_manager
            .lock()
            .unwrap()
            .get_table_manager()
            .get_table_manager::<SpcrManager>();
        if spcr_manager.is_none() {
            return false;
        }
        let spcr_manager = spcr_manager.unwrap();
        let base_address = spcr_manager.get_memory_mapped_io_base_address();
        if base_address.is_none() {
            return false;
        }
        let base_address = base_address.unwrap();
        match spcr_manager.get_interface_type() {
            SpcrManager::INTERFACE_TYPE_ARM_PL011 => {
                match io_remap!(
                    PAddress::new(base_address),
                    PAGE_SIZE,
                    MemoryPermissionFlags::data(),
                    MemoryOptionFlags::DEVICE_MEMORY
                ) {
                    Ok(virtual_address) => {
                        self.base_address = virtual_address.to_usize();
                        self.putc_func = pl011_putc;
                        self.wait_buffer = pl011_wait;
                        /*self.getc = ? */
                        return true;
                    }
                    Err(e) => {
                        pr_err!("Failed to map the Serial Port area: {:?}", e);
                        return false;
                    }
                }
            }
            SpcrManager::INTERFACE_TYPE_ARM_SBSA_GENERIC => { /*TODO...*/ }
            _ => {
                return false;
            }
        }
        return true;
    }

    pub fn send_str(&mut self, s: &str) {
        let _lock = self.lock.lock();
        for e in s.as_bytes() {
            if !(self.wait_buffer)(self.base_address) {
                return;
            }
            if *e == b'\n' {
                (self.putc_func)(self.base_address, *e);
                if !(self.wait_buffer)(self.base_address) {
                    return;
                }
            }
            (self.putc_func)(self.base_address, *e);
        }
    }
}

fn pl011_putc(base_address: usize, c: u8) {
    unsafe { core::ptr::write_volatile(base_address as *mut u8, c) };
}

fn pl011_wait(base_address: usize) -> bool {
    let mut time_out = 0xffffffusize;
    while time_out > 0 {
        if (unsafe { core::ptr::read_volatile((base_address + 0x018) as *const u16) } & (1 << 5))
            == 0
        {
            return true;
        }
        time_out -= 1;
        core::hint::spin_loop();
    }
    return false;
}
