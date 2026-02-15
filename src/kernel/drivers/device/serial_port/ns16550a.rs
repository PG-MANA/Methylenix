//!
//! NS16550A UART Driver
//!
//! Currently u8 size byte read MMIO Only (TODO: Support u32 read MMIO and IO instruction)
//!

use super::SerialPortDeviceEntry;

use crate::arch::target_arch::device::cpu::flush_data_cache_all;

use core::ptr::{read_volatile, write_volatile};

#[derive(Copy, Clone)]
enum Ns16550aReadableRegisters {
    RBR = 0x00,
    IER = 0x01,
    ISR = 0x02,
    LCR = 0x03,
    MCR = 0x04,
    LSR = 0x05,
    MSR = 0x06,
    SPR = 0x07,
}

#[derive(Copy, Clone)]
enum Ns16550aWritableRegisters {
    THR = 0x00,
    IER = 0x01,
    FCR = 0x02,
    LCR = 0x03,
    MCR = 0x04,
    SPR = 0x07,
}

const NS16550A_IER_DATA_READY: u8 = 1;
// /// (ISR Code = 0b0100) << 1) | INTERRUPT_STATUS
// const NS16550A_ISR_INTERRUPT_DATA_READY: u8 = 0b01001;
const NS16550A_LSR_TRANSMITTER_EMPTY: u8 = 1 << 6;
const NS16550A_LSR_DATA_READY: u8 = 1;

pub(super) const NS16550A: SerialPortDeviceEntry = SerialPortDeviceEntry {
    interface_type: 0xFF,
    compatible: "ns16550a",
    early_putc_func: early_ns16550a_putc,
    putc_func: ns16550a_putc,
    getc_func: ns16550a_getc,
    interrupt_enable: ns16550a_setup_interrupt,
    wait_buffer: ns16550a_wait,
};

unsafe fn read_register<A, L>(port: usize, register: Ns16550aReadableRegisters) -> L {
    unsafe { read_volatile((port as *mut A).add(register as usize) as *const L) as _ }
}

unsafe fn write_register<A, L>(port: usize, register: Ns16550aWritableRegisters, value: L) {
    unsafe { write_volatile((port as *mut A).add(register as usize) as *mut L, value) };
}

fn early_ns16550a_putc(base_address: usize, c: u8) {
    ns16550a_putc(base_address, c);
    flush_data_cache_all();
}

fn ns16550a_putc(base_address: usize, c: u8) {
    unsafe { write_register::<u8, u8>(base_address, Ns16550aWritableRegisters::THR, c as _) };
}

fn ns16550a_getc(base_address: usize) -> Option<u8> {
    unsafe {
        if (read_register::<u8, u8>(base_address, Ns16550aReadableRegisters::LSR)
            & NS16550A_LSR_DATA_READY)
            != 0
        {
            Some(read_register::<u8, u8>(
                base_address,
                Ns16550aReadableRegisters::RBR,
            ))
        } else {
            None
        }
    }
}

fn ns16550a_wait(base_address: usize) -> bool {
    let mut time_out = 0xffffffusize;
    while (unsafe { read_register::<u8, u8>(base_address, Ns16550aReadableRegisters::LSR) }
        & NS16550A_LSR_TRANSMITTER_EMPTY)
        == 0
    {
        if time_out == 0 {
            return false;
        }
        time_out -= 1;
        core::hint::spin_loop();
    }
    true
}

fn ns16550a_setup_interrupt(
    base_address: usize,
    _interrupt_id: u32,
    _handler: fn(usize) -> bool,
) -> bool {
    unsafe {
        write_register::<u8, u8>(
            base_address,
            Ns16550aWritableRegisters::IER,
            NS16550A_IER_DATA_READY,
        )
    };
    true
}
