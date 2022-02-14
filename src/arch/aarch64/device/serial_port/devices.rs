//!
//! Serial Port Devices
//!

use super::SerialPortDeviceEntry;

use crate::kernel::drivers::acpi::table::spcr::SpcrManager;

/// PL011
pub(super) const PL011: SerialPortDeviceEntry = SerialPortDeviceEntry {
    interface_type: SpcrManager::INTERFACE_TYPE_ARM_PL011,
    compatible: "arm,pl011",
    putc_func: pl011_putc,
    getc_func: pl011_getc,
    wait_buffer: pl011_wait,
};

fn pl011_putc(base_address: usize, c: u8) {
    unsafe { core::ptr::write_volatile(base_address as *mut u8, c) };
}

fn pl011_getc(_base_address: usize) -> Option<u8> {
    unimplemented!()
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

/// PL011
pub(super) const MESON_GX_UART: SerialPortDeviceEntry = SerialPortDeviceEntry {
    interface_type: 0xFF,
    compatible: "amlogic,meson-gx-uart",
    putc_func: meson_gx_putc,
    getc_func: meson_gx_getc,
    wait_buffer: meson_gx_wait,
};

fn meson_gx_putc(base_address: usize, c: u8) {
    unsafe { core::ptr::write_volatile(base_address as *mut u32, c as u32) };
}

fn meson_gx_getc(_base_address: usize) -> Option<u8> {
    unimplemented!()
}

fn meson_gx_wait(base_address: usize) -> bool {
    let mut time_out = 0xffffffusize;
    while time_out > 0 {
        if (unsafe { core::ptr::read_volatile((base_address + 0x0c) as *const u32) } & (1 << 21))
            == 0
        {
            return true;
        }
        time_out -= 1;
        core::hint::spin_loop();
    }
    return false;
}
