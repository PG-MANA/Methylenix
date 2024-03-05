//!
//! Guid Partition Table
//!

use super::{FileManager, PartitionInfo};

use crate::kernel::collections::guid::Guid;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize};
use crate::kernel::memory_manager::{alloc_non_linear_pages, free_pages};

const GPT_OFFSET: usize = 0x200;
const GPT_SIGNATURE_OFFSET: usize = 0x00;
const GPT_VERSION_OFFSET: usize = 0x08;
const GPT_HEADER_SIZE_OFFSET: usize = 0x0C;
const GPT_FIRST_USABLE_LBA_OFFSET: usize = 0x28;
const GPT_LAST_USABLE_LBA_OFFSET: usize = 0x30;
const GPT_DISK_GUID_OFFSET: usize = 0x38;
const GPT_PARTITION_ENTRIES_LBA_OFFSET: usize = 0x48;
const GPT_NUMBER_OF_PARTITIONS_OFFSET: usize = 0x50;
const GPT_SIZE_OF_PARTITION_ENTRY_OFFSET: usize = 0x54;

const GPT_SIGNATURE: [u8; 8] = *b"EFI PART";
const SUPPORTED_GPT_VERSION: u32 = 0x00010000;
const GPT_HEADER_SIZE: u32 = 92;

const PARTITION_GUID_UEFI: Guid = Guid::new(0xC12A7328, 0xF81F, 0x11D2, 0xBA4B, 0x00A0C93EC93B);
const PARTITION_GUID_LINUX_DATA: Guid =
    Guid::new(0x0FC63DAF, 0x8483, 0x4772, 0x8E79, 0x3D69D8477DE4);

pub fn detect_file_system(manager: &mut FileManager, block_device_id: usize) {
    let initial_read_size = 512 * 2;
    let lba_block_size = get_kernel_manager_cluster()
        .block_device_manager
        .get_lba_block_size(block_device_id);
    let first_block_data =
        match alloc_non_linear_pages!(MSize::new(initial_read_size).page_align_up()) {
            Ok(a) => a,

            Err(e) => {
                pr_err!("Failed to allocate memory: {:?}", e);
                return;
            }
        };

    if let Err(e) = get_kernel_manager_cluster().block_device_manager.read_lba(
        block_device_id,
        first_block_data,
        0,
        (initial_read_size as u64 / lba_block_size).max(1),
    ) {
        pr_err!("Failed to read data from disk: {:?}", e);
        return;
    }

    /* Skip MBR */

    /* Check GPT Signature, Version, and header size */
    let gpt_header_address = first_block_data.to_usize() + GPT_OFFSET;
    if unsafe { *((gpt_header_address + GPT_SIGNATURE_OFFSET) as *const [u8; 8]) } != GPT_SIGNATURE
    {
        let _ = free_pages!(first_block_data);
        pr_err!("Invalid GPT signature");
        return;
    }
    let version =
        u32::from_le(unsafe { *((gpt_header_address + GPT_VERSION_OFFSET) as *const u32) });
    if version != SUPPORTED_GPT_VERSION {
        let _ = free_pages!(first_block_data);
        pr_err!("Unsupported version: {:#X}", version);
        return;
    }
    if u32::from_le(unsafe { *((gpt_header_address + GPT_HEADER_SIZE_OFFSET) as *const u32) })
        != GPT_HEADER_SIZE
    {
        let _ = free_pages!(first_block_data);
        pr_err!("Invalid header size");
        return;
    }

    let first_usable_lba = u64::from_le(unsafe {
        *((gpt_header_address + GPT_FIRST_USABLE_LBA_OFFSET) as *const u64)
    });
    let last_usable_lba =
        u64::from_le(unsafe { *((gpt_header_address + GPT_LAST_USABLE_LBA_OFFSET) as *const u64) });
    pr_debug!(
        "First/Last usable LBA: {:#X}/{:#X}",
        first_usable_lba,
        last_usable_lba
    );
    let disk_guid = unsafe { &*((gpt_header_address + GPT_DISK_GUID_OFFSET) as *const [u8; 16]) };
    pr_debug!("Disk GUID: {}", Guid::new_le(disk_guid));

    let starting_lba_partition_entry = u64::from_le(unsafe {
        *((gpt_header_address + GPT_PARTITION_ENTRIES_LBA_OFFSET) as *const u64)
    });
    let number_of_partitions = u32::from_le(unsafe {
        *((gpt_header_address + GPT_NUMBER_OF_PARTITIONS_OFFSET) as *const u32)
    });
    let partition_entry_size = u32::from_le(unsafe {
        *((gpt_header_address + GPT_SIZE_OF_PARTITION_ENTRY_OFFSET) as *const u32)
    });

    let _ = free_pages!(first_block_data);

    'sector_loop: for block in
        0..=((number_of_partitions * partition_entry_size) as usize / lba_block_size as usize)
    {
        let partition_entries =
            match alloc_non_linear_pages!(MSize::new(lba_block_size as usize).page_align_up()) {
                Ok(a) => a,
                Err(e) => {
                    pr_err!("Failed to allocate memory: {:?}", e);
                    return;
                }
            };

        if let Err(e) = get_kernel_manager_cluster().block_device_manager.read_lba(
            block_device_id,
            partition_entries,
            starting_lba_partition_entry + block as u64,
            1,
        ) {
            pr_err!("Failed to read data from disk: {:?}", e);
            return;
        }

        for i in 0..(number_of_partitions as usize) {
            let partition_entry =
                partition_entries.to_usize() + i * (partition_entry_size as usize);
            let partition_type_guid = unsafe { &*(partition_entry as *const [u8; 16]) };
            if *partition_type_guid == [0; 16] {
                let _ = free_pages!(partition_entries);
                break 'sector_loop;
            }
            let partition_type_guid = Guid::new_le(partition_type_guid);
            let partition_guid =
                Guid::new_le(unsafe { &*((partition_entry + 0x10) as *const [u8; 16]) });
            let starting_lba = unsafe { *((partition_entry + 0x20) as *const u64) };
            let ending_lba = unsafe { *((partition_entry + 0x28) as *const u64) };

            pr_debug!(
                "Partition Type GUID: {}({}), Partition GUID: {}, LBA: {:#X}~{:#X}",
                partition_type_guid,
                match partition_type_guid {
                    PARTITION_GUID_UEFI => "EFI system partition",
                    PARTITION_GUID_LINUX_DATA => "Linux Data",
                    _ => "Unknown",
                },
                partition_guid,
                starting_lba,
                ending_lba,
            );
            let partition_info = PartitionInfo {
                device_id: block_device_id,
                starting_lba,
                ending_lba,
                lba_block_size,
            };
            manager.analysis_partition(partition_info);
        }
        let _ = free_pages!(partition_entries);
    }
}
