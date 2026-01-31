//!
//! DesignWare APB UART
//!

use super::SerialPortDeviceEntry;

use crate::arch::target_arch::device::cpu::flush_data_cache_all;

use core::ptr::{read_volatile, write_volatile};

const DW_APB_UART_RBR: usize = 0x00;
const DW_APB_UART_THR: usize = 0x00;

const DW_APB_UART_IER: usize = 0x04;
const DW_APB_UART_IER_ERBFI: u32 = 1;
const DW_APB_UART_FCR: usize = 0x08;
const DW_APB_UART_FCR_FIFOE: u32 = 1;
const DW_APB_UART_LSR: usize = 0x14;

const DW_APB_UART_LSR_TEMT: u32 = 1 << 6;
const DW_APB_UART_LSR_DR: u32 = 1;

pub(super) const DW_APB_UART: SerialPortDeviceEntry = SerialPortDeviceEntry {
    interface_type: 0xFF,
    compatible: "snps,dw-apb-uart",
    early_putc_func: early_dw_apb_uart_putc,
    putc_func: dw_apb_uart_putc,
    getc_func: dw_apb_uart_getc,
    interrupt_enable: dw_apb_uart_setup_interrupt,
    wait_buffer: dw_apb_uart_wait,
};

fn dw_apb_uart_read_lsr(base_address: usize) -> u32 {
    unsafe { read_volatile((base_address + DW_APB_UART_LSR) as *const u32) }
}

fn early_dw_apb_uart_putc(base_address: usize, c: u8) {
    dw_apb_uart_putc(base_address, c);
    flush_data_cache_all();
}

fn dw_apb_uart_putc(base_address: usize, c: u8) {
    unsafe { write_volatile((base_address + DW_APB_UART_THR) as *mut u32, c as u32) };
}

fn dw_apb_uart_getc(base_address: usize) -> Option<u8> {
    if (dw_apb_uart_read_lsr(base_address) & DW_APB_UART_LSR_DR) != 0 {
        Some(
            (unsafe { read_volatile((base_address + DW_APB_UART_RBR) as *const u32) } & 0xFF) as u8,
        )
    } else {
        None
    }
}

fn dw_apb_uart_wait(base_address: usize) -> bool {
    let mut time_out = 0xffffffusize;
    while time_out > 0 {
        if dw_apb_uart_read_lsr(base_address) & DW_APB_UART_LSR_TEMT != 0 {
            return true;
        }
        time_out -= 1;
        core::hint::spin_loop();
    }
    false
}

// TODO: Re-implement
#[cfg(target_arch = "aarch64")]
fn dw_apb_uart_setup_interrupt(
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

    while dw_apb_uart_getc(base_address).is_some() {
        core::hint::spin_loop();
    }

    unsafe {
        /* Enable FIFO */
        write_volatile(
            (base_address + DW_APB_UART_FCR) as *mut u32,
            DW_APB_UART_FCR_FIFOE,
        );

        /* Enable Interrupt */
        write_volatile(
            (base_address + DW_APB_UART_IER) as *mut u32,
            DW_APB_UART_IER_ERBFI,
        );
    }
    true
}

#[cfg(not(target_arch = "aarch64"))]
fn dw_apb_uart_setup_interrupt(_: usize, _: u32, _: fn(usize) -> bool) -> bool {
    false
}
