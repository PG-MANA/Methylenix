//!
//! File System
//!

mod gpt;

pub fn detect_partitions(device_id: usize) {
    gpt::detect_file_system(device_id);
}
