//!
//! Serial Port Devices
//!

mod devices;

use crate::arch::target_arch::paging::PAGE_SIZE;

use crate::io_remap;
use crate::kernel::drivers::acpi::table::spcr::SpcrManager;
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
};
use crate::kernel::sync::spin_lock::SpinLockFlag;
use crate::kernel::task_manager::work_queue::WorkList;

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

fn dummy_interrupt_setup(_: usize, _: u32, _: fn(usize) -> bool) -> bool {
    return false;
}

struct SerialPortDeviceEntry {
    interface_type: u8,
    compatible: &'static str,
    putc_func: fn(base_address: usize, char: u8),
    getc_func: fn(base_address: usize) -> Option<u8>,
    interrupt_enable:
        fn(base_address: usize, interrupt_id: u32, handler: fn(usize) -> bool) -> bool,
    wait_buffer: fn(base_address: usize) -> bool,
}

const SERIAL_PORT_DEVICES: [SerialPortDeviceEntry; 2] = [devices::PL011, devices::MESON_GX_UART];

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
        for e in &SERIAL_PORT_DEVICES {
            if spcr_manager.get_interface_type() == e.interface_type {
                match io_remap!(
                    PAddress::new(base_address),
                    PAGE_SIZE,
                    MemoryPermissionFlags::data(),
                    MemoryOptionFlags::DEVICE_MEMORY
                ) {
                    Ok(virtual_address) => {
                        self.base_address = virtual_address.to_usize();
                        self.interrupt_id = spcr_manager.get_interrupt_id();
                        self.putc_func = e.putc_func;
                        self.wait_buffer = e.wait_buffer;
                        self.getc_func = e.getc_func;
                        self.interrupt_enable = e.interrupt_enable;
                        return true;
                    }
                    Err(e) => {
                        pr_err!("Failed to map the Serial Port area: {:?}", e);
                        return false;
                    }
                }
            }
        }
        return false;
    }

    pub fn init_with_dtb(&mut self) -> bool {
        let _lock = self.lock.lock();
        let dtb_manager = &get_kernel_manager_cluster().arch_depend_data.dtb_manager;

        for node_name in [b"uart".as_slice(), b"serial".as_slice()].iter() {
            let mut previous = None;
            while let Some(info) = dtb_manager.search_node(node_name, previous.as_ref()) {
                for e in &SERIAL_PORT_DEVICES {
                    if dtb_manager.is_device_compatible(&info, e.compatible.as_bytes())
                        && dtb_manager.is_node_operational(&info)
                    {
                        if let Some((address, size)) = dtb_manager.read_reg_property(&info, 0) {
                            match io_remap!(
                                PAddress::new(address),
                                MSize::new(size),
                                MemoryPermissionFlags::data(),
                                MemoryOptionFlags::DEVICE_MEMORY
                            ) {
                                Ok(virtual_address) => {
                                    self.base_address = virtual_address.to_usize();
                                    self.putc_func = e.putc_func;
                                    self.wait_buffer = e.wait_buffer;
                                    self.getc_func = e.getc_func;
                                    return true;
                                }
                                Err(e) => {
                                    pr_err!("Failed to map the Serial Port area: {:?}", e);
                                    return false;
                                }
                            }
                        } else {
                            pr_err!("No address available");
                        }
                    }
                }
                previous = Some(info);
            }
        }
        return false;
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
            let work = WorkList::new(Self::worker, c as usize);
            if let Err(_) = get_cpu_manager_cluster().work_queue.add_work(work) {
                pr_err!("Failed to add work for key event");
            }
            return true;
        }
        return false;
    }

    fn worker(data: usize) {
        if let Err(e) = get_kernel_manager_cluster()
            .kernel_tty_manager
            .input(data as u8)
        {
            pr_err!("Cannot input data to tty. Error: {:?}", e);
        }
    }

    pub fn send_str(&mut self, s: &str) {
        let _lock = self.lock.lock();
        for e in s.as_bytes() {
            if !(self.wait_buffer)(self.base_address) {
                return;
            }
            if *e == b'\n' {
                (self.putc_func)(self.base_address, b'\r');
                if !(self.wait_buffer)(self.base_address) {
                    return;
                }
            }
            (self.putc_func)(self.base_address, *e);
        }
    }
}
