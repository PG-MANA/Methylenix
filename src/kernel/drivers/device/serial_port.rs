//!
//! Serial Port Devices
//!

pub mod dw_apb;
pub mod ns16550a;
pub mod pl011;

/// Dummy putc Function
pub fn dummy_putc(_: usize, _: u8) {}

/// Dummy putc Function
pub fn dummy_getc(_: usize) -> Option<u8> {
    None
}

/// Dummy wait for buffer function
pub fn dummy_wait_buffer(_: usize) -> bool {
    true
}

pub fn dummy_interrupt_setup(_: usize, _: u32, _: fn(usize) -> bool) -> bool {
    false
}

pub struct SerialPortDeviceEntry {
    pub interface_type: u8,
    pub compatible: &'static str,
    pub early_putc_func: fn(base_address: usize, char: u8),
    pub putc_func: fn(base_address: usize, char: u8),
    pub getc_func: fn(base_address: usize) -> Option<u8>,
    pub interrupt_enable:
        fn(base_address: usize, interrupt_id: u32, handler: fn(usize) -> bool) -> bool,
    pub wait_buffer: fn(base_address: usize) -> bool,
}

pub const SERIAL_PORT_DEVICES: [SerialPortDeviceEntry; 3] =
    [pl011::PL011, dw_apb::DW_APB_UART, ns16550a::NS16550A];
