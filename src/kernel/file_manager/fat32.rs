//!
//! FAT32
//!

use super::{PartitionInfo, PartitionManager, PathInfo};

use crate::free_pages;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, VAddress};

use alloc::boxed::Box;
use core::any::Any;
use core::mem::MaybeUninit;

const FAT32_SIGNATURE: [u8; 8] = [b'F', b'A', b'T', b'3', b'2', b' ', b' ', b' '];

const BYTES_PER_SECTOR_OFFSET: usize = 11;
const SECTORS_PER_CLUSTER_OFFSET: usize = 13;
const NUM_OF_RESERVED_CLUSTER_OFFSET: usize = 14;
const NUM_OF_FATS_OFFSET: usize = 16;
const FAT_SIZE_OFFSET: usize = 36;
const ROOT_CLUSTER_OFFSET: usize = 44;
const FAT32_SIGNATURE_OFFSET: usize = 82;

const FAT32_ATTRIBUTE_DIRECTORY: u8 = 0x10;
const FAT32_ATTRIBUTE_VOLUME_ID: u8 = 0x08;
const FAT32_ATTRIBUTE_LONG_FILE_NAME: u8 = 0x0F;

const DIRECTORY_ENTRY_SIZE: usize = 32;

pub(super) struct Fat32Info {
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    fat_size: u32,
    number_of_fats: u16,
    root_cluster: u32,
    fat: VAddress,
}

struct Fat32EntryInfo {
    entry_cluster: u32,
    attribute: u8,
    file_size: u32,
}

pub(super) fn try_detect_file_system(
    partition_info: &PartitionInfo,
    first_4k_data: VAddress,
) -> Result<Fat32Info, ()> {
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

    pr_debug!(
        "LBA Block Size: {:#X}, FAT Size: {:#X}, SectorsPerCluster: {:#X}",
        partition_info.lba_block_size,
        fat_size,
        sectors_per_cluster
    );

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
        root_cluster,
        fat: fat_data,
    };

    fat32_info.list_files(partition_info, root_cluster, 0);
    return Ok(fat32_info);
}

impl PartitionManager for Fat32Info {
    fn search_file(
        &self,
        partition_info: &PartitionInfo,
        file_name: &PathInfo,
    ) -> Result<Box<dyn Any>, ()> {
        let mut entry_info = Fat32EntryInfo {
            entry_cluster: self.root_cluster,
            attribute: FAT32_ATTRIBUTE_DIRECTORY,
            file_size: 0,
        };
        for e in file_name.iter() {
            if e.len() == 0 || e == "/" {
                continue;
            }
            if (entry_info.attribute & FAT32_ATTRIBUTE_DIRECTORY) == 0 {
                pr_debug!("Failed to search {}", file_name.as_str());
                return Err(());
            }
            match self.find_entry(partition_info, entry_info.entry_cluster, e) {
                Ok(entry) => {
                    entry_info = entry;
                }
                Err(_) => {
                    pr_debug!("Failed to search: {}(In {})", file_name.as_str(), e);
                    return Err(());
                }
            }
        }
        return Ok(Box::new(entry_info));
    }

    fn get_file_size(&self, _: &PartitionInfo, file_info: &Box<dyn Any>) -> Result<usize, ()> {
        file_info
            .downcast_ref::<Fat32EntryInfo>()
            .and_then(|info| Some(info.file_size as usize))
            .ok_or(())
    }

    fn read_file(
        &self,
        partition_info: &PartitionInfo,
        file_info: &Box<dyn Any>,
        offset: usize,
        mut length: usize,
        buffer: VAddress,
    ) -> Result<(), ()> {
        let entry_info = file_info.downcast_ref::<Fat32EntryInfo>().unwrap();
        if (entry_info.attribute
            & (FAT32_ATTRIBUTE_DIRECTORY
                | FAT32_ATTRIBUTE_VOLUME_ID
                | FAT32_ATTRIBUTE_LONG_FILE_NAME))
            != 0
        {
            pr_err!("Invalid File");
            return Err(());
        }
        if offset + length > entry_info.file_size as usize {
            pr_err!(
                "offset({:#X}) and length({:#X}) is exceeded the file size({:#X})",
                offset,
                length,
                entry_info.file_size
            );
            return Err(());
        }
        let bytes_per_cluster = self.sectors_per_cluster as usize * self.bytes_per_sector as usize;
        let number_of_clusters_to_skip = offset / bytes_per_cluster;
        let mut memory_offset = offset - number_of_clusters_to_skip * bytes_per_cluster;
        //let num_of_clusters = ((length + memory_offset) / bytes_per_cluster).max(1);

        /* TODO: avoid memory copy */

        macro_rules! next_cluster {
            ($c:expr) => {
                match self.get_next_cluster($c) {
                    Some(n) => n,
                    None => {
                        pr_err!("Failed to get next cluster");
                        return Err(());
                    }
                }
            };
        }

        let mut reading_cluster = entry_info.entry_cluster;
        for _ in 0..number_of_clusters_to_skip {
            reading_cluster = next_cluster!(reading_cluster);
        }
        let mut buffer_pointer = 0usize;
        loop {
            let data =
                match self.read_sector(partition_info, self.cluster_to_sector(reading_cluster)) {
                    Ok(address) => address,
                    Err(e) => {
                        pr_err!("Failed to read data from disk: {:?}", e);
                        return Err(());
                    }
                };
            let copy_size = bytes_per_cluster.min(length);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (data.to_usize() + memory_offset) as *const u8,
                    (buffer.to_usize() + buffer_pointer) as *mut u8,
                    copy_size,
                )
            };
            let _ = free_pages!(data);
            memory_offset = 0;
            buffer_pointer += copy_size;
            length -= copy_size;
            if length == 0 {
                break;
            }
            reading_cluster = next_cluster!(reading_cluster);
        }
        return Ok(());
    }
}

impl Fat32Info {
    fn find_entry(
        &self,
        partition_info: &PartitionInfo,
        mut cluster: u32,
        target_entry_name: &str,
    ) -> Result<Fat32EntryInfo, ()> {
        loop {
            let directory_list_data =
                match self.read_sector(partition_info, self.cluster_to_sector(cluster)) {
                    Ok(address) => address,
                    Err(e) => {
                        pr_err!("Failed to read data from disk: {:?}", e);
                        return Err(());
                    }
                };

            let limit = (self.bytes_per_sector as usize) * self.sectors_per_cluster as usize;
            let mut pointer = 0;
            while limit > pointer {
                let entry_base = directory_list_data.to_usize() + pointer;
                let attribute = unsafe { *((entry_base + 11) as *const u8) };
                if (attribute & 0x3F) == FAT32_ATTRIBUTE_LONG_FILE_NAME
                    || (attribute & FAT32_ATTRIBUTE_VOLUME_ID) != 0
                {
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

                if entry_name_ascii == target_entry_name {
                    let _ = free_pages!(directory_list_data);
                    return Ok(Fat32EntryInfo {
                        entry_cluster,
                        attribute,
                        file_size,
                    });
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
        return Err(());
    }

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
            let limit = (self.bytes_per_sector as usize) * self.sectors_per_cluster as usize;
            let mut pointer = 0;
            while limit > pointer {
                let entry_base = directory_list_data.to_usize() + pointer;
                let attribute = unsafe { *((entry_base + 11) as *const u8) };
                if (attribute & 0x3F) == FAT32_ATTRIBUTE_LONG_FILE_NAME {
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

                if (attribute & FAT32_ATTRIBUTE_DIRECTORY) != 0
                    && entry_name_ascii != "."
                    && entry_name_ascii != ".."
                {
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