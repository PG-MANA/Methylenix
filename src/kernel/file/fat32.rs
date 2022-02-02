//!
//! FAT32
//!

use crate::free_pages;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, VAddress};

const FAT32_SIGNATURE: [u8; 8] = [b'F', b'A', b'T', b'3', b'2', b' ', b' ', b' '];

pub fn try_detect_file_system(
    device_id: usize,
    first_4k_data: VAddress,
    starting_lba: usize,
    end_lba: usize,
) -> Result<(), ()> {
    if unsafe { *((first_4k_data.to_usize() + 82) as *const [u8; 8]) } != FAT32_SIGNATURE {
        return Err(());
    }
    pr_debug!(
        "OEM: {}",
        core::str::from_utf8(unsafe { &*((first_4k_data.to_usize() + 3) as *const [u8; 8]) })
            .unwrap_or("N/A")
    );
    let byte_per_sector = u16::from_le(unsafe { *((first_4k_data.to_usize() + 11) as *const u16) });
    let sectors_per_cluster = unsafe { *((first_4k_data.to_usize() + 13) as *const u8) };
    let number_of_reserved_sectors =
        u16::from_le(unsafe { *((first_4k_data.to_usize() + 14) as *const u16) });
    let number_of_fats = u16::from_le(unsafe { *((first_4k_data.to_usize() + 16) as *const u16) });
    let fat_size_32 = u32::from_le(unsafe { *((first_4k_data.to_usize() + 36) as *const u32) });
    let root_cluster_sector =
        u32::from_le(unsafe { *((first_4k_data.to_usize() + 44) as *const u32) });

    let root_directory_sector_number = (number_of_reserved_sectors as usize)
        + (number_of_fats as usize) * (fat_size_32 as usize)
        + (root_cluster_sector as usize - 2) * (sectors_per_cluster as usize);

    if root_directory_sector_number >= end_lba || root_directory_sector_number < starting_lba {
        pr_err!(
            "Invalid root directory sector: {:#}",
            root_directory_sector_number
        );
        return Err(());
    }
    let root_directory = match get_kernel_manager_cluster()
        .block_device_manager
        .read_by_lba(
            device_id,
            starting_lba + root_directory_sector_number,
            sectors_per_cluster as usize, /* OK? */
        ) {
        Ok(address) => address,
        Err(e) => {
            pr_err!("Failed to read data from disk: {:?}", e);
            return Err(());
        }
    };
    const DIRECTORY_ENTRY_SIZE: usize = 32;
    let limit = byte_per_sector as usize * sectors_per_cluster as usize;
    let mut pointer = 0;
    while limit > pointer {
        let entry_base = root_directory.to_usize() + pointer;
        let directory_name = unsafe { &*(entry_base as *const [u8; 11]) };
        if directory_name[0] == 0 {
            pr_debug!("End of Directory");
            break;
        } else if directory_name[0] == 0xe5 {
            pr_debug!("Empty Entry");
            pointer += DIRECTORY_ENTRY_SIZE;
            continue;
        }
        let mut directory_name = *directory_name;
        if directory_name[0] == 0x05 {
            directory_name[0] = 0xe5;
        }
        let directory_name_ascii = core::str::from_utf8(&directory_name).unwrap_or("");
        let file_size = u32::from_le(unsafe { *((entry_base + 28) as *const u32) });
        let attribute = unsafe { *((entry_base + 11) as *const u8) };
        let entry_cluster = ((u16::from_le(unsafe { *((entry_base + 20) as *const u16) }) as u32)
            << 16)
            | u16::from_le(unsafe { *((entry_base + 26) as *const u16) }) as u32;
        pr_debug!(
            "Entry: Name: {}, Attribute: {:#X}, Entry Cluster: {:#X}, File Size: {:#X}",
            directory_name_ascii,
            attribute,
            entry_cluster,
            file_size
        );

        pointer += DIRECTORY_ENTRY_SIZE;
    }
    let _ = free_pages!(root_directory);
    return Ok(());
}
