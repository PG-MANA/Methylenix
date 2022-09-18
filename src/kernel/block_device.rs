//!
//! Block Device
//!
//! The structures are temporary

use crate::kernel::memory_manager::{data_type::VAddress, MemoryError};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

use alloc::vec::Vec;

pub trait BlockDeviceDriver {
    fn read_data_lba(
        &mut self,
        info: &BlockDeviceInfo,
        buffer: VAddress,
        base_lba: u64,
        number_of_blocks: u64,
    ) -> Result<(), BlockDeviceError>;

    fn get_lba_block_size(&self, info: &BlockDeviceInfo) -> u64;
}

#[derive(Clone)]
pub struct BlockDeviceInfo {
    pub info_id: usize,
    pub device_id: usize,
}

#[derive(Clone)]
pub struct BlockDeviceDescriptor {
    info: BlockDeviceInfo,
    driver: *mut dyn BlockDeviceDriver,
}

pub struct BlockDeviceManager {
    lock: IrqSaveSpinLockFlag,
    device_list: Vec<BlockDeviceDescriptor>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BlockDeviceError {
    InvalidDevice,
    InvalidBuffer,
    InvalidOperation,
    DeviceError,
    MemoryError(MemoryError),
}

impl From<MemoryError> for BlockDeviceError {
    fn from(m: MemoryError) -> Self {
        Self::MemoryError(m)
    }
}

impl BlockDeviceManager {
    pub const fn new() -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            device_list: Vec::new(),
        }
    }

    pub fn add_block_device(&mut self, mut d: BlockDeviceDescriptor) {
        let _lock = self.lock.lock();
        d.info.info_id = self.device_list.len();
        self.device_list.push(d);
        drop(_lock);
    }

    pub fn get_number_of_devices(&self) -> usize {
        self.device_list.len()
    }

    pub fn read_lba(
        &self,
        id: usize,
        buffer: VAddress,
        base_lba: u64,
        number_of_blocks: u64,
    ) -> Result<(), BlockDeviceError> {
        let _lock = self.lock.lock();
        if id >= self.device_list.len() {
            drop(_lock);
            return Err(BlockDeviceError::InvalidDevice);
        }

        let d = &self.device_list[id];
        unsafe { &mut *d.driver }.read_data_lba(&d.info, buffer, base_lba, number_of_blocks)
    }

    pub fn get_lba_block_size(&self, device_id: usize) -> u64 {
        let _lock = self.lock.lock();
        if device_id >= self.device_list.len() {
            drop(_lock);
            pr_err!("Invalid device_id: {}", device_id);
            return 0;
        }
        let size = unsafe { &*self.device_list[device_id].driver }
            .get_lba_block_size(&self.device_list[device_id].info);
        drop(_lock);
        return size;
    }
}

impl BlockDeviceDescriptor {
    pub fn new(device_id: usize, driver: *mut dyn BlockDeviceDriver) -> Self {
        Self {
            info: BlockDeviceInfo {
                info_id: 0,
                device_id,
            },
            driver,
        }
    }
}
