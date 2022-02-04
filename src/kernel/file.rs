//!
//! File System
//!

use crate::free_pages;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;

mod fat32;
mod gpt;

pub fn detect_partitions(device_id: usize) {
    gpt::detect_file_system(device_id);
}

//#[derive(Clone)]
struct PartitionInfo {
    device_id: usize,
    starting_lba: usize,
    #[allow(dead_code)]
    ending_lba: usize,
    lba_block_size: usize,
}

fn analysis_partition(
    device_id: usize,
    starting_lba: usize,
    ending_lba: usize,
    lba_sector_size: usize,
) {
    let first_block = match get_kernel_manager_cluster()
        .block_device_manager
        .read_by_lba(device_id, starting_lba, 1)
    {
        Ok(address) => address,
        Err(e) => {
            pr_err!("Failed to read data from disk: {:?}", e);
            return;
        }
    };
    let _ = fat32::try_detect_file_system(
        &PartitionInfo {
            device_id,
            starting_lba,
            ending_lba,
            lba_block_size: lba_sector_size,
        },
        first_block,
    );
    let _ = free_pages!(first_block);
    return;
}
