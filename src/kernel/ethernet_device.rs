//!
//! Ethernet RX/TX Device Manager
//!
//!

use crate::kernel::memory_manager::data_type::{MSize, VAddress};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

use alloc::vec::Vec;

pub trait EthernetDeviceDriver {
    fn send(
        &mut self,
        info: &EthernetDeviceInfo,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, ()>;
}

#[derive(Clone)]
pub struct EthernetDeviceInfo {
    pub info_id: usize,
    pub device_id: usize,
}

#[derive(Clone)]
pub struct EthernetDeviceDescriptor {
    info: EthernetDeviceInfo,
    driver: *mut dyn EthernetDeviceDriver,
}

pub struct EthernetDeviceManager {
    lock: IrqSaveSpinLockFlag,
    device_list: Vec<EthernetDeviceDescriptor>,
}

impl EthernetDeviceManager {
    pub const fn new() -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            device_list: Vec::new(),
        }
    }

    pub fn add_device(&mut self, mut d: EthernetDeviceDescriptor) {
        let _lock = self.lock.lock();
        d.info.info_id = self.device_list.len();
        self.device_list.push(d);
        drop(_lock);
    }
}

impl EthernetDeviceDescriptor {
    pub fn new(device_id: usize, driver: *mut dyn EthernetDeviceDriver) -> Self {
        Self {
            info: EthernetDeviceInfo {
                info_id: 0,
                device_id,
            },
            driver,
        }
    }
}
