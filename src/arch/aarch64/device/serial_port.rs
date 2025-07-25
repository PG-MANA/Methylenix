//!
//! Serial Port Devices
//!

mod devices;

use crate::arch::aarch64::interrupt::gic;
use crate::arch::target_arch::paging::PAGE_SIZE;

use crate::kernel::drivers::acpi::table::spcr::SpcrManager;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
};
use crate::kernel::memory_manager::io_remap;
use crate::kernel::sync::spin_lock::SpinLockFlag;
use crate::kernel::tty::Writer;

use core::fmt;

/// Dummy putc Function
fn dummy_putc(_: usize, _: u8) {}

/// Dummy putc Function
fn dummy_getc(_: usize) -> Option<u8> {
    None
}

/// Dummy wait for buffer function
fn dummy_wait_buffer(_: usize) -> bool {
    true
}

fn dummy_interrupt_setup(_: usize, _: u32, _: fn(usize) -> bool) -> bool {
    false
}

struct SerialPortDeviceEntry {
    interface_type: u8,
    compatible: &'static str,
    early_putc_func: fn(base_address: usize, char: u8),
    putc_func: fn(base_address: usize, char: u8),
    getc_func: fn(base_address: usize) -> Option<u8>,
    interrupt_enable:
        fn(base_address: usize, interrupt_id: u32, handler: fn(usize) -> bool) -> bool,
    wait_buffer: fn(base_address: usize) -> bool,
}

const SERIAL_PORT_DEVICES: [SerialPortDeviceEntry; 2] = [devices::PL011, devices::DW_APB_UART];

pub struct SerialPortManager {
    lock: SpinLockFlag,
    base_address: usize,
    interrupt_id: u32,
    putc_func: fn(base_address: usize, char: u8),
    getc_func: fn(base_address: usize) -> Option<u8>,
    interrupt_enable:
        fn(base_address: usize, interrupt_id: u32, handler: fn(usize) -> bool) -> bool,
    wait_buffer: fn(base_address: usize) -> bool,
}

impl Default for SerialPortManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SerialPortManager {
    const SERIAL_PORT_DEFAULT_PRIORITY: u8 = 0x00;
    pub fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            base_address: 0,
            interrupt_id: 0,
            putc_func: dummy_putc,
            getc_func: dummy_getc,
            interrupt_enable: dummy_interrupt_setup,
            wait_buffer: dummy_wait_buffer,
        }
    }

    /// For debug
    #[allow(dead_code)]
    pub unsafe fn init_early(&mut self, physical_address: usize, device_index: usize) {
        self.base_address = physical_address;
        self.putc_func = SERIAL_PORT_DEVICES[device_index].early_putc_func;
        self.wait_buffer = SERIAL_PORT_DEVICES[device_index].wait_buffer;
    }

    pub fn init_with_acpi(&mut self) -> bool {
        let Some(spcr_manager) = get_kernel_manager_cluster()
            .acpi_manager
            .lock()
            .unwrap()
            .get_table_manager()
            .get_table_manager::<SpcrManager>()
        else {
            return false;
        };
        let Some(base_address) = spcr_manager.get_memory_mapped_io_base_address() else {
            return false;
        };
        for e in &SERIAL_PORT_DEVICES {
            if spcr_manager.get_interface_type() == e.interface_type {
                return match io_remap!(
                    PAddress::new(base_address),
                    PAGE_SIZE,
                    MemoryPermissionFlags::data(),
                    MemoryOptionFlags::DEVICE_MEMORY
                ) {
                    Ok(virtual_address) => {
                        let _lock = self.lock.lock();
                        self.base_address = virtual_address.to_usize();
                        self.interrupt_id = spcr_manager.get_interrupt_id();
                        self.putc_func = e.putc_func;
                        self.wait_buffer = e.wait_buffer;
                        self.getc_func = e.getc_func;
                        self.interrupt_enable = e.interrupt_enable;
                        true
                    }
                    Err(e) => {
                        pr_err!("Failed to map the Serial Port area: {:?}", e);
                        false
                    }
                };
            }
        }
        false
    }

    pub fn init_with_dtb(&mut self) -> bool {
        let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;

        for node_name in [b"uart".as_slice(), b"serial".as_slice()].iter() {
            let mut previous = None;
            while let Some(info) = dtb_manager.search_node(node_name, previous.as_ref()) {
                if !dtb_manager.is_node_operational(&info) {
                    previous = Some(info);
                    continue;
                }
                for e in &SERIAL_PORT_DEVICES {
                    if dtb_manager.is_device_compatible(&info, e.compatible.as_bytes()) {
                        if let Some((address, size)) = dtb_manager.read_reg_property(&info, 0) {
                            return match io_remap!(
                                PAddress::new(address),
                                MSize::new(size),
                                MemoryPermissionFlags::data(),
                                MemoryOptionFlags::DEVICE_MEMORY
                            ) {
                                Ok(virtual_address) => {
                                    let _lock = self.lock.lock();
                                    self.base_address = virtual_address.to_usize();
                                    self.putc_func = e.putc_func;
                                    self.wait_buffer = e.wait_buffer;
                                    self.getc_func = e.getc_func;
                                    if let Some((interrupt_id, _)) =
                                        gic::read_interrupt_info_from_dtb(dtb_manager, &info, 0)
                                    {
                                        self.interrupt_id = interrupt_id;
                                        self.interrupt_enable = e.interrupt_enable;
                                    } else {
                                        pr_warn!("Failed to get the interrupt information");
                                    }
                                    true
                                }
                                Err(e) => {
                                    pr_err!("Failed to map the Serial Port area: {:?}", e);
                                    false
                                }
                            };
                        } else {
                            pr_err!("No address available");
                        }
                    }
                }
                previous = Some(info);
            }
        }
        false
    }

    pub fn setup_interrupt(&self) -> bool {
        (self.interrupt_enable)(
            self.base_address,
            self.interrupt_id,
            Self::interrupt_handler,
        )
    }

    fn interrupt_handler(_: usize) -> bool {
        let serial_manager = &get_kernel_manager_cluster().serial_port_manager;
        if let Some(c) = (serial_manager.getc_func)(serial_manager.base_address) {
            crate::kernel::tty::TtyManager::input_from_interrupt_handler(c);
            true
        } else {
            false
        }
    }
}

impl Writer for SerialPortManager {
    fn write(
        &self,
        buf: &[u8],
        size_to_write: usize,
        _foreground_color: u32,
        _background_color: u32,
    ) -> fmt::Result {
        let _lock = self.lock.lock();
        for e in buf[0..size_to_write].iter() {
            if !(self.wait_buffer)(self.base_address) {
                return Err(fmt::Error {});
            }
            if *e == b'\n' {
                (self.putc_func)(self.base_address, b'\r');
                if !(self.wait_buffer)(self.base_address) {
                    return Err(fmt::Error {});
                }
            }
            (self.putc_func)(self.base_address, *e);
        }
        Ok(())
    }
}
