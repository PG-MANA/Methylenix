//!
//! File System
//!

pub mod elf;
mod fat32;
mod gpt;
mod path_info;
mod vfs;
mod xfs;

pub use self::path_info::{PathInfo, PathInfoIter};
pub use self::vfs::{
    File, FileDescriptor, FileOperationDriver, FileSeekOrigin, FILE_PERMISSION_READ,
    FILE_PERMISSION_WRITE,
};

use crate::kernel::block_device::BlockDeviceError;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{MOffset, MSize, VAddress};
use crate::kernel::memory_manager::{alloc_non_linear_pages, free_pages, MemoryError};

use alloc::boxed::Box;
use alloc::vec::Vec;

pub struct FileManager {
    partition_list: Vec<(PartitionInfo, Box<dyn PartitionManager>)>,
}

//#[derive(Clone)]
struct PartitionInfo {
    device_id: usize,
    starting_lba: u64,
    #[allow(dead_code)]
    ending_lba: u64,
    lba_block_size: u64,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum FileError {
    MemoryError(MemoryError),
    BadSignature,
    FileNotFound,
    InvalidFile,
    OperationNotPermitted,
    OperationNotSupported,
    DeviceError,
}

impl From<MemoryError> for FileError {
    fn from(m: MemoryError) -> Self {
        Self::MemoryError(m)
    }
}

impl From<BlockDeviceError> for FileError {
    fn from(b: BlockDeviceError) -> Self {
        if let BlockDeviceError::MemoryError(m) = b {
            Self::MemoryError(m)
        } else {
            Self::DeviceError
        }
    }
}

trait PartitionManager {
    fn search_file(
        &self,
        partition_info: &PartitionInfo,
        file_name: &PathInfo,
    ) -> Result<usize, FileError>;

    fn get_file_size(
        &self,
        partition_info: &PartitionInfo,
        file_info: usize,
    ) -> Result<usize, FileError>;

    fn read_file(
        &self,
        partition_info: &PartitionInfo,
        file_info: usize,
        offset: MOffset,
        length: MSize,
        buffer: VAddress,
    ) -> Result<MSize, FileError>;

    fn close_file(&self, partition_info: &PartitionInfo, file_info: usize);
}

impl FileManager {
    pub fn new() -> Self {
        Self {
            partition_list: Vec::new(),
        }
    }

    pub fn detect_partitions(&mut self, device_id: usize) {
        gpt::detect_file_system(self, device_id);
    }

    fn analysis_partition(
        &mut self,
        device_id: usize,
        starting_lba: u64,
        ending_lba: u64,
        lba_block_size: u64,
    ) {
        let first_block_data =
            match alloc_non_linear_pages!(MSize::new(lba_block_size as usize).page_align_up()) {
                Ok(a) => a,
                Err(e) => {
                    pr_err!("Failed to allocate memory: {:?}", e);
                    return;
                }
            };
        if let Err(e) = get_kernel_manager_cluster().block_device_manager.read_lba(
            device_id,
            first_block_data,
            starting_lba,
            1,
        ) {
            pr_err!("Failed to read data from disk: {:?}", e);
            return;
        }

        let partition_info = PartitionInfo {
            device_id,
            starting_lba,
            ending_lba,
            lba_block_size,
        };

        match fat32::try_detect_file_system(&partition_info, first_block_data) {
            Ok(f) => {
                self.partition_list.push((partition_info, Box::new(f)));
                let _ = free_pages!(first_block_data);
                return;
            }
            Err(FileError::BadSignature) => { /* Next FS */ }
            Err(err) => {
                pr_err!("Failed to detect the file system: {:?}", err);
                let _ = free_pages!(first_block_data);
                return;
            }
        }

        match xfs::try_detect_file_system(&partition_info, first_block_data) {
            Ok(f) => {
                self.partition_list.push((partition_info, Box::new(f)));
                let _ = free_pages!(first_block_data);
                return;
            }
            Err(FileError::BadSignature) => { /* Next FS */ }
            Err(err) => {
                pr_err!("Failed to detect the file system: {:?}", err);
                let _ = free_pages!(first_block_data);
                return;
            }
        }
        pr_err!("Unknown File System");
        let _ = free_pages!(first_block_data);
        return;
    }

    pub fn get_number_of_file_systems(&self) -> usize {
        self.partition_list.len()
    }

    fn get_file_size(&self, descriptor: &FileDescriptor) -> Result<usize, FileError> {
        let p = &self.partition_list[descriptor.get_device_index()];
        p.1.get_file_size(&p.0, descriptor.get_data())
    }

    pub fn file_open(&mut self, file_name: &PathInfo, permission: u8) -> Result<File, FileError> {
        for (index, e) in self.partition_list.iter().enumerate() {
            match e.1.search_file(&e.0, file_name) {
                Ok(data) => {
                    return Ok(File::new(
                        FileDescriptor::new(data, index, permission),
                        self,
                    ));
                }
                Err(FileError::FileNotFound) => { /* Continue */ }
                Err(e) => Err(e)?,
            }
        }
        return Err(FileError::FileNotFound);
    }
}

impl FileOperationDriver for FileManager {
    fn read(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, FileError> {
        let p = &self.partition_list[descriptor.get_device_index()];
        let result = p.1.read_file(
            &p.0,
            descriptor.get_data(),
            descriptor.get_position(),
            length,
            buffer,
        );
        if let Ok(s) = result {
            descriptor.add_position(s);
        }
        return result;
    }

    fn write(
        &mut self,
        _descriptor: &mut FileDescriptor,
        _buffer: VAddress,
        _length: MSize,
    ) -> Result<MSize, FileError> {
        Err(FileError::OperationNotSupported)
    }

    fn seek(
        &mut self,
        descriptor: &mut FileDescriptor,
        offset: MOffset,
        origin: FileSeekOrigin,
    ) -> Result<MOffset, FileError> {
        match origin {
            FileSeekOrigin::SeekSet => descriptor.set_position(offset),
            FileSeekOrigin::SeekCur => descriptor.add_position(offset),
            FileSeekOrigin::SeekEnd => {
                let pos = self.get_file_size(descriptor)?;
                descriptor.set_position(MOffset::new(pos));
            }
        }
        return Ok(descriptor.get_position());
    }

    fn close(&mut self, descriptor: FileDescriptor) {
        let p = &self.partition_list[descriptor.get_device_index()];
        p.1.close_file(&p.0, descriptor.get_data());
    }
}
