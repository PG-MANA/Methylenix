//!
//! Serial Port Devices
//!

use super::{SerialPortDeviceEntry, SerialPortManager};

use crate::kernel::drivers::acpi::table::spcr::SpcrManager;
use crate::kernel::manager_cluster::get_cpu_manager_cluster;

use core::ptr::{read_volatile, write_volatile};

const PL011_UARTDR: usize = 0x00;
const PL011_UARTFR: usize = 0x18;
const PL011_UARTFR_TXFF: u16 = 1 << 5;
const PL011_UARTFR_RXFE: u16 = 1 << 4;
const PL011_UARTLCR_H: usize = 0x2C;
const PL011_UARTLCR_H_FEN: u16 = 1 << 4;
const PL011_UARTIMSC: usize = 0x38;
const PL011_UARTIMSC_RXIM: u16 = 1 << 4;

/// PL011
pub(super) const PL011: SerialPortDeviceEntry = SerialPortDeviceEntry {
    interface_type: SpcrManager::INTERFACE_TYPE_ARM_PL011,
    compatible: "arm,pl011",
    putc_func: pl011_putc,
    getc_func: pl011_getc,
    interrupt_enable: pl011_setup_interrupt,
    wait_buffer: pl011_wait,
};

fn pl011_putc(base_address: usize, c: u8) {
    unsafe { write_volatile((base_address + PL011_UARTDR) as *mut u16, c as u16) };
}

fn pl011_getc(base_address: usize) -> Option<u8> {
    unsafe {
        if (read_volatile((base_address + PL011_UARTFR) as *const u16) & PL011_UARTFR_RXFE) == 0 {
            Some(
                (read_volatile((base_address + PL011_UARTDR) as *const u16) & u8::MAX as u16) as u8,
            )
        } else {
            None
        }
    }
}

fn pl011_wait(base_address: usize) -> bool {
    let mut time_out = 0xffffffusize;
    while (unsafe { read_volatile((base_address + PL011_UARTFR) as *const u16) }
        & PL011_UARTFR_TXFF)
        != 0
    {
        if time_out == 0 {
            return false;
        }
        time_out -= 1;
        core::hint::spin_loop();
    }
    return true;
}

fn pl011_setup_interrupt(
    base_address: usize,
    interrupt_id: u32,
    handler: fn(usize) -> bool,
) -> bool {
    if get_cpu_manager_cluster()
        .interrupt_manager
        .set_device_interrupt_function(
            handler,
            interrupt_id,
            SerialPortManager::SERIAL_PORT_DEFAULT_PRIORITY,
            None,
            true,
        )
        .is_err()
    {
        return false;
    }
    while pl011_getc(base_address).is_some() {
        core::hint::spin_loop();
    }
    unsafe {
        write_volatile(
            (base_address + PL011_UARTLCR_H) as *mut u16,
            read_volatile((base_address + PL011_UARTLCR_H) as *const u16) | PL011_UARTLCR_H_FEN,
        );
        write_volatile(
            (base_address + PL011_UARTIMSC) as *mut u16,
            read_volatile((base_address + PL011_UARTIMSC) as *const u16) | PL011_UARTIMSC_RXIM,
        );
    }
    return true;
}

/// PL011
pub(super) const MESON_GX_UART: SerialPortDeviceEntry = SerialPortDeviceEntry {
    interface_type: 0xFF,
    compatible: "amlogic,meson-gx-uart",
    putc_func: meson_gx_putc,
    getc_func: meson_gx_getc,
    interrupt_enable: super::dummy_interrupt_setup,
    wait_buffer: meson_gx_wait,
};

fn meson_gx_putc(base_address: usize, c: u8) {
    unsafe { write_volatile(base_address as *mut u32, c as u32) };
}

fn meson_gx_getc(_base_address: usize) -> Option<u8> {
    unimplemented!()
}

fn meson_gx_wait(base_address: usize) -> bool {
    let mut time_out = 0xffffffusize;
    while time_out > 0 {
        if (unsafe { read_volatile((base_address + 0x0c) as *const u32) } & (1 << 21)) == 0 {
            return true;
        }
        time_out -= 1;
        core::hint::spin_loop();
    }
    return false;
}
