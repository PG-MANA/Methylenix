//!
//! File System
//!

pub mod elf;
mod fat32;
mod gpt;
mod path_info;
mod vfs;

pub use self::path_info::{PathInfo, PathInfoIter};
pub use self::vfs::{File, FileDescriptor, FileOperationDriver, FileSeekOrigin};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{MSize, VAddress};
use crate::{alloc_non_linear_pages, free_pages};

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

trait PartitionManager {
    fn search_file(
        &self,
        partition_info: &PartitionInfo,
        file_name: &PathInfo,
    ) -> Result<usize, ()>;

    fn get_file_size(&self, partition_info: &PartitionInfo, file_info: usize) -> Result<usize, ()>;

    fn read_file(
        &self,
        partition_info: &PartitionInfo,
        file_info: usize,
        offset: usize,
        length: usize,
        buffer: VAddress,
    ) -> Result<(), ()>;

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
            }
            Err(_) => {}
        }
        let _ = free_pages!(first_block_data);
        return;
    }

    pub fn get_number_of_file_systems(&self) -> usize {
        self.partition_list.len()
    }

    fn get_file_size(&self, descriptor: &FileDescriptor) -> Result<usize, ()> {
        let p = &self.partition_list[descriptor.get_device_index()];
        p.1.get_file_size(&p.0, descriptor.get_data())
    }

    pub fn file_open(&mut self, file_name: &PathInfo) -> Result<File, ()> {
        for (index, e) in self.partition_list.iter().enumerate() {
            match e.1.search_file(&e.0, file_name) {
                Ok(data) => {
                    return Ok(File::new(FileDescriptor::new(data, index), self));
                }
                Err(_) => { /* continue */ }
            }
        }
        return Err(());
    }
}

impl FileOperationDriver for FileManager {
    fn read(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: usize,
    ) -> Result<usize, ()> {
        let p = &self.partition_list[descriptor.get_device_index()];
        let result = p.1.read_file(
            &p.0,
            descriptor.get_data(),
            descriptor.get_position(),
            length,
            buffer,
        );
        if result.is_ok() {
            descriptor.add_position(length);
        }
        return result.and_then(|_| Ok(length));
    }

    fn write(
        &mut self,
        _descriptor: &mut FileDescriptor,
        _buffer: VAddress,
        _length: usize,
    ) -> Result<usize, ()> {
        unimplemented!()
    }

    fn seek(
        &mut self,
        descriptor: &mut FileDescriptor,
        offset: usize,
        origin: FileSeekOrigin,
    ) -> Result<usize, ()> {
        match origin {
            FileSeekOrigin::SeekSet => descriptor.set_position(offset),
            FileSeekOrigin::SeekCur => descriptor.add_position(offset),
            FileSeekOrigin::SeekEnd => {
                let pos = self.get_file_size(descriptor)?;
                descriptor.set_position(pos);
            }
        }
        return Ok(descriptor.get_position());
    }

    fn close(&mut self, descriptor: FileDescriptor) {
        let p = &self.partition_list[descriptor.get_device_index()];
        p.1.close_file(&p.0, descriptor.get_data());
    }
}
