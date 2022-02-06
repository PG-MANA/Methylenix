//!
//! Block Device
//!
//! The structures are temporary

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

use crate::alloc_pages_with_physical_address;
use alloc::vec::Vec;

pub trait BlockDeviceDriver {
    fn read_data(
        &mut self,
        info: &BlockDeviceInfo,
        offset: usize,
        size: usize,
        pages_to_write: PAddress,
    ) -> Result<(), ()>;

    fn read_data_by_lba(
        &mut self,
        info: &BlockDeviceInfo,
        lba: usize,
        sectors: usize,
    ) -> Result<VAddress, ()>;

    fn get_lba_sector_size(&self, info: &BlockDeviceInfo) -> usize;
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

    pub fn read(&self, id: usize, offset: usize, size: usize) -> Result<VAddress, ()> {
        let (page, physical_page) = match alloc_pages_with_physical_address!(
            MSize::new(size).to_order(None).to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(p) => p,
            Err(e) => {
                pr_err!("Failed to allocate memory: {:?}", e);
                return Err(());
            }
        };

        let _lock = self.lock.lock();
        if id >= self.device_list.len() {
            drop(_lock);
            let _ = get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(page);
            return Err(());
        }

        let d = &self.device_list[id];
        if let Err(e) = unsafe { &mut *d.driver }.read_data(&d.info, offset, size, physical_page) {
            pr_err!("Failed to read data: {:?}", e);
            drop(_lock);
            let _ = get_kernel_manager_cluster()
                .kernel_memory_manager
                .free(page);
            return Err(());
        }
        return Ok(page);
    }

    pub fn read_by_lba(&self, id: usize, lba: usize, sectors: usize) -> Result<VAddress, ()> {
        let _lock = self.lock.lock();
        if id >= self.device_list.len() {
            drop(_lock);
            return Err(());
        }

        let d = &self.device_list[id];
        let result = unsafe { &mut *d.driver }.read_data_by_lba(&d.info, lba, sectors);
        if let Err(e) = result {
            pr_err!("Failed to read data: {:?}", e);
            drop(_lock);
            return Err(());
        }
        return result;
    }

    pub fn get_lba_sector_size(&self, device_id: usize) -> usize {
        let _lock = self.lock.lock();
        if device_id >= self.device_list.len() {
            drop(_lock);
            pr_err!("Invalid device_id: {}", device_id);
            return 0;
        }
        let size = unsafe { &*self.device_list[device_id].driver }
            .get_lba_sector_size(&self.device_list[device_id].info);
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
