//!
//! NS16550A UART Driver
//!
//! Currently u32 size MMIO Only (TODO: Support u8/u16 MMIO and IO instruction)
//!

use super::SerialPortDeviceEntry;

use crate::arch::target_arch::device::cpu::flush_data_cache_all;

use crate::kernel::drivers::acpi::table::spcr::SpcrManager;

use core::ptr::{read_volatile, write_volatile};

enum Ns16550Registers {
    RBR = 0x00,
    THR = 0x00,
    IER = 0x01,
    IIR = 0x02,
    PCR = 0x02,
    LCR = 0x03,
    LSR = 0x05,
}

const NS16550_IER_DATA_READY: u8 = 1;
/// (ISR Code = 0b0100) << 1) | INTERRUPT_STATUS
const NS16550_ISR_INTERRUPT_DATA_READY: u8 = 0b01001;
const NS16550_LSR_TRANSMITTER_EMPTY: u8 = 1 << 6;
const NS16550_LSR_DATA_READY: u8 = 1;

pub(super) const NS16550: SerialPortDeviceEntry = SerialPortDeviceEntry {
    interface_type: 0xFF,
    compatible: "ns16550a",
    early_putc_func: early_pl011_putc,
    putc_func: pl011_putc,
    getc_func: pl011_getc,
    interrupt_enable: pl011_setup_interrupt,
    wait_buffer: pl011_wait,
};

fn early_pl011_putc(base_address: usize, c: u8) {
    pl011_putc(base_address, c);
    flush_data_cache_all();
}

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
    true
}

// TODO: Re-implement
#[cfg(target_arch = "aarch64")]
fn pl011_setup_interrupt(
    base_address: usize,
    interrupt_id: u32,
    handler: fn(usize) -> bool,
) -> bool {
    use crate::arch::target_arch::device::serial_port::SerialPortManager;
    use crate::kernel::manager_cluster::get_cpu_manager_cluster;

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
    true
}

#[cfg(not(target_arch = "aarch64"))]
fn pl011_setup_interrupt(_: usize, _: u32, _: fn(usize) -> bool) -> bool {
    false
}
