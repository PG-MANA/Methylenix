//!
//! File System
//!

use crate::free_pages;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::Address;

mod fat32;
mod gpt;

pub fn detect_partitions(device_id: usize) {
    gpt::detect_file_system(device_id);
}

fn analysis_partition(device_id: usize, starting_lba: usize, ending_lba: usize) {
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
    let header = unsafe { &*(first_block.to_usize() as *const [u8; 3]) };
    pr_debug!(
        "First 3 block: {:#X}, {:#X}, {:#X}",
        header[0],
        header[1],
        header[2]
    );
    let _ = fat32::try_detect_file_system(device_id, first_block, starting_lba, ending_lba);
    let _ = free_pages!(first_block);
    return;
}
