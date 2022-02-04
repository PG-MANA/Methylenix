//!
//! FAT32
//!

use super::PartitionInfo;

use crate::free_pages;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, VAddress};

use core::mem::MaybeUninit;

const FAT32_SIGNATURE: [u8; 8] = [b'F', b'A', b'T', b'3', b'2', b' ', b' ', b' '];

const BYTES_PER_SECTOR_OFFSET: usize = 11;
const SECTORS_PER_CLUSTER_OFFSET: usize = 13;
const NUM_OF_RESERVED_CLUSTER_OFFSET: usize = 14;
const NUM_OF_FATS_OFFSET: usize = 16;
const FAT_SIZE_OFFSET: usize = 36;
const ROOT_CLUSTER_OFFSET: usize = 44;
const FAT32_SIGNATURE_OFFSET: usize = 82;

struct Fat32Info {
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    fat_size: u32,
    number_of_fats: u16,
    fat: VAddress,
}

pub(super) fn try_detect_file_system(
    partition_info: &PartitionInfo,
    first_4k_data: VAddress,
) -> Result<(), ()> {
    if unsafe { *((first_4k_data.to_usize() + FAT32_SIGNATURE_OFFSET) as *const [u8; 8]) }
        != FAT32_SIGNATURE
    {
        return Err(());
    }
    let bytes_per_sector = u16::from_le(unsafe {
        *((first_4k_data.to_usize() + BYTES_PER_SECTOR_OFFSET) as *const u16)
    });
    let sectors_per_cluster =
        unsafe { *((first_4k_data.to_usize() + SECTORS_PER_CLUSTER_OFFSET) as *const u8) };
    let number_of_reserved_sectors = u16::from_le(unsafe {
        *((first_4k_data.to_usize() + NUM_OF_RESERVED_CLUSTER_OFFSET) as *const u16)
    });
    let number_of_fats =
        u16::from_le(unsafe { *((first_4k_data.to_usize() + NUM_OF_FATS_OFFSET) as *const u16) });
    let fat_size =
        u32::from_le(unsafe { *((first_4k_data.to_usize() + FAT_SIZE_OFFSET) as *const u32) });
    let root_cluster =
        u32::from_le(unsafe { *((first_4k_data.to_usize() + ROOT_CLUSTER_OFFSET) as *const u32) });

    let fat_data = match get_kernel_manager_cluster()
        .block_device_manager
        .read_by_lba(
            partition_info.device_id,
            partition_info.starting_lba
                + number_of_reserved_sectors as usize * bytes_per_sector as usize
                    / partition_info.lba_block_size,
            (fat_size as usize / partition_info.lba_block_size).max(1),
        ) {
        Ok(address) => address,
        Err(e) => {
            pr_err!("Failed to read FAT from disk: {:?}", e);
            return Err(());
        }
    };
    let fat32_info = Fat32Info {
        bytes_per_sector,
        sectors_per_cluster,
        reserved_sectors: number_of_reserved_sectors,
        fat_size,
        number_of_fats,
        fat: fat_data,
    };

    fat32_info.list_files(partition_info, root_cluster, 0);
    return Ok(());
}

impl Fat32Info {
    fn list_files(&self, partition_info: &PartitionInfo, mut cluster: u32, indent: usize) {
        loop {
            let directory_list_data =
                match self.read_sector(partition_info, self.cluster_to_sector(cluster)) {
                    Ok(address) => address,
                    Err(e) => {
                        pr_err!("Failed to read data from disk: {:?}", e);
                        return;
                    }
                };
            const DIRECTORY_ENTRY_SIZE: usize = 32;
            let limit = (self.bytes_per_sector as usize) * self.sectors_per_cluster as usize;
            let mut pointer = 0;
            while limit > pointer {
                let entry_base = directory_list_data.to_usize() + pointer;
                let attribute = unsafe { *((entry_base + 11) as *const u8) };
                if (attribute & 0x3F) == 0x0F {
                    pointer += DIRECTORY_ENTRY_SIZE;
                    continue;
                }
                let directory_name = unsafe { &*(entry_base as *const [u8; 11]) };
                if directory_name[0] == 0 {
                    break;
                } else if directory_name[0] == 0xE5 || directory_name[0] == 0x08 {
                    pointer += DIRECTORY_ENTRY_SIZE;
                    continue;
                }
                let mut entry_name: [MaybeUninit<u8>; 12] = MaybeUninit::uninit_array();
                entry_name[0].write(if directory_name[0] == 0x05 {
                    0xe5
                } else {
                    directory_name[0]
                });
                let mut p = 1;
                for index in 1..11 {
                    if directory_name[index] == b' ' {
                        continue;
                    }
                    if index == 8 {
                        entry_name[p].write(b'.');
                        p += 1;
                    }
                    entry_name[p].write(directory_name[index]);
                    p += 1;
                }
                let entry_name = unsafe { MaybeUninit::array_assume_init(entry_name) };
                let entry_name_ascii = core::str::from_utf8(&entry_name[0..p]).unwrap_or("N/A");
                let file_size = u32::from_le(unsafe { *((entry_base + 28) as *const u32) });

                let entry_cluster =
                    ((u16::from_le(unsafe { *((entry_base + 20) as *const u16) }) as u32) << 16)
                        | u16::from_le(unsafe { *((entry_base + 26) as *const u16) }) as u32;
                for _ in 0..indent {
                    kprint!(" ");
                }
                kprintln!(
                    "|- {}: A: {:#X}, FS: {:#X}",
                    entry_name_ascii,
                    attribute,
                    file_size
                );

                if (attribute & 0x10) != 0 && entry_name_ascii != "." && entry_name_ascii != ".." {
                    self.list_files(partition_info, entry_cluster, indent + 1);
                }
                pointer += DIRECTORY_ENTRY_SIZE;
            }
            let _ = free_pages!(directory_list_data);
            if limit <= pointer {
                if let Some(next) = self.get_next_cluster(cluster) {
                    cluster = next;
                    continue;
                }
            }
            break;
        }
        return;
    }

    fn read_sector(&self, partition_info: &PartitionInfo, sector: u32) -> Result<VAddress, ()> {
        get_kernel_manager_cluster()
            .block_device_manager
            .read_by_lba(
                partition_info.device_id,
                partition_info.starting_lba + sector as usize,
                (self.sectors_per_cluster as usize * self.bytes_per_sector as usize
                    / partition_info.lba_block_size)
                    .max(1),
            )
    }

    fn cluster_to_sector(&self, cluster: u32) -> u32 {
        (self.reserved_sectors as u32)
            + (self.number_of_fats as u32) * self.fat_size
            + (cluster - 2) * (self.sectors_per_cluster as u32)
    }

    fn get_next_cluster(&self, cluster: u32) -> Option<u32> {
        let n = unsafe {
            *((self.fat.to_usize() + cluster as usize * core::mem::size_of::<u32>()) as *const u32)
        };
        if n >= 0x0ffffff8 {
            None
        } else {
            Some(n)
        }
    }
}
