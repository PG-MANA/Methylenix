//!
//! File System
//!

pub mod elf;
mod fat32;
mod gpt;
mod path_info;

pub use self::path_info::{PathInfo, PathInfoIter};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{MSize, VAddress};
use crate::{alloc_non_linear_pages, free_pages};

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::Any;

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

pub struct FileInfo {
    d: Box<dyn Any>,
    index: usize,
    offset: usize,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum FileSeekOrigin {
    SeekSet,
    SeekCur,
    SeekEnd,
}

trait PartitionManager {
    fn search_file(
        &self,
        partition_info: &PartitionInfo,
        file_name: &PathInfo,
    ) -> Result<Box<dyn Any>, ()>;

    fn get_file_size(
        &self,
        partition_info: &PartitionInfo,
        file_info: &Box<dyn Any>,
    ) -> Result<usize, ()>;

    fn read_file(
        &self,
        partition_info: &PartitionInfo,
        file_info: &Box<dyn Any>,
        offset: usize,
        length: usize,
        buffer: VAddress,
    ) -> Result<(), ()>;
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

    fn get_file_size(&self, file_info: &FileInfo) -> Result<usize, ()> {
        let p = &self.partition_list[file_info.index];
        p.1.get_file_size(&p.0, &file_info.d)
    }

    pub fn file_open(&self, file_name: &PathInfo) -> Result<FileInfo, ()> {
        for (index, e) in self.partition_list.iter().enumerate() {
            match e.1.search_file(&e.0, file_name) {
                Ok(d) => {
                    return Ok(FileInfo {
                        d,
                        offset: 0,
                        index,
                    });
                }
                Err(_) => { /* continue */ }
            }
        }
        return Err(());
    }

    pub fn file_read(
        &self,
        file_info: &mut FileInfo,
        buffer: VAddress,
        length: usize,
    ) -> Result<(), ()> {
        let p = &self.partition_list[file_info.index];
        let result =
            p.1.read_file(&p.0, &file_info.d, file_info.offset, length, buffer);
        if result.is_ok() {
            file_info.offset += length;
        }
        return result;
    }

    pub fn file_seek(
        &self,
        file_info: &mut FileInfo,
        offset: usize,
        origin: FileSeekOrigin,
    ) -> Result<usize, ()> {
        match origin {
            FileSeekOrigin::SeekSet => file_info.offset = offset,
            FileSeekOrigin::SeekCur => file_info.offset += offset,
            FileSeekOrigin::SeekEnd => {
                file_info.offset = self.get_file_size(file_info)?;
            }
        }
        return Ok(file_info.offset);
    }

    pub fn file_close(&self, file_info: FileInfo) {
        drop(file_info);
    }
}
