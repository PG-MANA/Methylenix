//!
//! FAT32
//!

use super::{FileError, FileInfo, PartitionInfo, PartitionManager};
use alloc::string::String;

use crate::kernel::collections::guid::Guid;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MOffset, MSize, VAddress};
use crate::kernel::memory_manager::{alloc_non_linear_pages, free_pages};

const FAT32_SIGNATURE: [u8; 8] = [b'F', b'A', b'T', b'3', b'2', b' ', b' ', b' '];

const BYTES_PER_SECTOR_OFFSET: usize = 11;
const SECTORS_PER_CLUSTER_OFFSET: usize = 13;
const NUM_OF_RESERVED_CLUSTER_OFFSET: usize = 14;
const NUM_OF_FATS_OFFSET: usize = 16;
const FAT_SIZE_OFFSET: usize = 36;
const ROOT_CLUSTER_OFFSET: usize = 44;
const FAT32_VOLUME_ID_OFFSET: usize = 67;
const FAT32_SIGNATURE_OFFSET: usize = 82;

const FAT32_ATTRIBUTE_DIRECTORY: u8 = 0x10;
const FAT32_ATTRIBUTE_VOLUME_ID: u8 = 0x08;
const FAT32_ATTRIBUTE_LONG_FILE_NAME: u8 = 0x0F;

const DIRECTORY_ENTRY_SIZE: usize = 32;

pub(super) struct Fat32Driver {
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    fat_sectors: u32,
    number_of_fats: u16,
    root_cluster: u32,
    fat: VAddress,
}

struct Fat32EntryInfo {
    entry_cluster: u32,
    attribute: u8,
    file_size: u32,
    file_name: String,
}

pub(super) fn try_mount_file_system(
    partition_info: &PartitionInfo,
    first_4k_data: VAddress,
) -> Result<(Fat32Driver, Guid), FileError> {
    if unsafe { *((first_4k_data.to_usize() + FAT32_SIGNATURE_OFFSET) as *const [u8; 8]) }
        != FAT32_SIGNATURE
    {
        return Err(FileError::BadSignature);
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
    let fat_sectors =
        u32::from_le(unsafe { *((first_4k_data.to_usize() + FAT_SIZE_OFFSET) as *const u32) });
    let fat_size = (fat_sectors as usize) * (bytes_per_sector as usize);
    let root_cluster =
        u32::from_le(unsafe { *((first_4k_data.to_usize() + ROOT_CLUSTER_OFFSET) as *const u32) });

    let lba_aligned_fat_size = ((fat_size - 1) & (!(partition_info.lba_block_size as usize - 1)))
        + partition_info.lba_block_size as usize;

    pr_debug!(
        "LBA Block Size: {:#X}, FAT Size: {:#X}(Aligned; {:#X}), SectorsPerCluster: {:#X}",
        partition_info.lba_block_size,
        fat_size,
        lba_aligned_fat_size,
        sectors_per_cluster
    );

    let fat = match alloc_non_linear_pages!(MSize::new(lba_aligned_fat_size).page_align_up()) {
        Ok(a) => a,
        Err(err) => {
            pr_err!("Failed to allocate memory for FAT: {:?}", err);
            Err(err)?
        }
    };
    if let Err(err) = get_kernel_manager_cluster().block_device_manager.read_lba(
        partition_info.device_id,
        fat,
        partition_info.starting_lba
            + (number_of_reserved_sectors as u64) * (bytes_per_sector as u64)
                / (partition_info.lba_block_size),
        (lba_aligned_fat_size as u64 / partition_info.lba_block_size).max(1),
    ) {
        bug_on_err!(free_pages!(fat));
        pr_err!("Failed to read FAT from disk: {:?}", err);
        Err(err)?;
    }
    let fat32_driver = Fat32Driver {
        bytes_per_sector,
        sectors_per_cluster,
        reserved_sectors: number_of_reserved_sectors,
        fat_sectors,
        number_of_fats,
        root_cluster,
        fat,
    };

    fat32_driver.list_files(partition_info, root_cluster, 0); // Debug
    Ok((
        fat32_driver,
        Guid::new(
            u16::from_le(unsafe {
                *((first_4k_data.to_usize() + FAT32_VOLUME_ID_OFFSET + 2) as *const u16)
            }) as u32,
            u16::from_le(unsafe {
                *((first_4k_data.to_usize() + FAT32_VOLUME_ID_OFFSET) as *const u16)
            }),
            0,
            0,
            0,
        ),
    ))
}

impl PartitionManager for Fat32Driver {
    fn get_root_node(
        &mut self,
        _partition_info: &PartitionInfo,
        file_info: &mut FileInfo,
        _is_writable: bool,
    ) -> Result<(), FileError> {
        file_info.set_inode_number(self.root_cluster as _);
        file_info.set_file_size(
            (self.bytes_per_sector as usize * self.sectors_per_cluster as usize) as _,
        );

        let all_permission = FileInfo::PERMISSION_FLAG_EXECUTE
            | FileInfo::PERMISSION_FLAG_WRITE
            | FileInfo::PERMISSION_FLAG_READ;
        file_info.set_permission(all_permission, all_permission, all_permission);
        file_info.set_attribute_directory();

        Ok(())
    }

    fn search_file(
        &self,
        partition_info: &PartitionInfo,
        file_name: &str,
        current_directory: &mut FileInfo,
    ) -> Result<FileInfo, FileError> {
        let entry = self.find_entry(
            partition_info,
            current_directory.get_inode_number() as u32,
            file_name,
        )?;

        let mut file_info = FileInfo::new(current_directory);

        file_info.set_inode_number(entry.entry_cluster as _);
        file_info.set_file_size(entry.file_size as _);
        file_info.set_file_name(entry.file_name);
        file_info.driver = current_directory.driver;

        let all_permission = FileInfo::PERMISSION_FLAG_EXECUTE
            | FileInfo::PERMISSION_FLAG_WRITE
            | FileInfo::PERMISSION_FLAG_READ;
        file_info.set_permission(all_permission, all_permission, all_permission);

        if (entry.attribute & FAT32_ATTRIBUTE_DIRECTORY) != 0 {
            file_info.set_attribute_directory();
        }
        if (entry.attribute & FAT32_ATTRIBUTE_VOLUME_ID) != 0
            || (entry.attribute & FAT32_ATTRIBUTE_LONG_FILE_NAME) != 0
        {
            file_info.set_attribute_meta_file();
        }

        Ok(file_info)
    }

    fn get_file_size(
        &self,
        _partition_info: &PartitionInfo,
        file_info: &FileInfo,
    ) -> Result<u64, FileError> {
        Ok(file_info.get_file_size())
    }

    fn read_file(
        &self,
        partition_info: &PartitionInfo,
        file_info: &mut FileInfo,
        offset: MOffset,
        mut length: MSize,
        buffer: VAddress,
    ) -> Result<MSize, FileError> {
        if offset + length > MSize::new(file_info.get_file_size() as usize) {
            if offset >= MSize::new(file_info.get_file_size() as usize) {
                return Ok(MSize::new(0));
            }
            length -= MSize::new(file_info.get_file_size() as usize) - offset;
        }
        let length = length.to_usize();

        macro_rules! next_cluster {
            ($c:expr) => {
                match self.get_next_cluster($c) {
                    Some(n) => n,
                    None => {
                        pr_err!("Failed to get next cluster");
                        return Err(FileError::InvalidFile);
                    }
                }
            };
        }

        let bytes_per_cluster = self.sectors_per_cluster as usize * self.bytes_per_sector as usize;
        let number_of_clusters_to_skip = offset.to_usize() / bytes_per_cluster;
        let mut page_buffer_offset =
            offset.to_usize() - number_of_clusters_to_skip * bytes_per_cluster;
        let mut reading_cluster = file_info.get_inode_number() as u32;
        let mut buffer_pointer = 0usize;

        for _ in 0..number_of_clusters_to_skip {
            reading_cluster = next_cluster!(reading_cluster);
        }
        loop {
            /* try to read max continued sectors */
            let mut number_of_sectors = 0;
            let mut read_bytes = 0;
            let first_cluster = reading_cluster;

            let page_buffer_offset_backup = page_buffer_offset;
            loop {
                if (length - read_bytes + page_buffer_offset) <= bytes_per_cluster {
                    number_of_sectors +=
                        (1 + ((length - read_bytes + page_buffer_offset).max(1) - 1)
                            / self.bytes_per_sector as usize) as u32;
                    read_bytes += length - read_bytes;
                    break;
                } else {
                    number_of_sectors += self.sectors_per_cluster as u32;
                    read_bytes += bytes_per_cluster - page_buffer_offset;
                }
                let next_cluster = next_cluster!(reading_cluster);
                if next_cluster != reading_cluster + 1 {
                    break;
                }
                page_buffer_offset = 0;
                reading_cluster = next_cluster;
            }
            page_buffer_offset = page_buffer_offset_backup;

            let block_aligned_buffer_size = MSize::new(
                (((number_of_sectors as usize) * (self.bytes_per_sector as usize))
                    & (!(partition_info.lba_block_size as usize - 1)))
                    + partition_info.lba_block_size as usize,
            )
            .page_align_up();
            let page_buffer = match alloc_non_linear_pages!(block_aligned_buffer_size) {
                Ok(a) => a,
                Err(err) => {
                    pr_err!("Failed to allocate memory for read: {:?}", err);
                    return Err(FileError::MemoryError(err));
                }
            };

            if let Err(err) = self.read_sectors(
                partition_info,
                page_buffer,
                self.cluster_to_sector(first_cluster),
                number_of_sectors,
            ) {
                pr_err!("Failed to read data from disk: {:?}", err);
                bug_on_err!(free_pages!(page_buffer));
                return Err(err);
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (page_buffer.to_usize() + page_buffer_offset) as *const u8,
                    (buffer.to_usize() + buffer_pointer) as *mut u8,
                    read_bytes,
                )
            };
            bug_on_err!(free_pages!(page_buffer));
            buffer_pointer += read_bytes;
            page_buffer_offset = 0;
            if length == buffer_pointer {
                break;
            }
            reading_cluster = next_cluster!(reading_cluster);
        }
        Ok(MSize::new(buffer_pointer))
    }

    fn close_file(&self, _: &PartitionInfo, _file_info: &mut FileInfo) {}
}

impl Fat32Driver {
    fn find_entry(
        &self,
        partition_info: &PartitionInfo,
        mut cluster: u32,
        target_entry_name: &str,
    ) -> Result<Fat32EntryInfo, FileError> {
        let directory_list_data = match alloc_non_linear_pages!(
            MSize::new(self.bytes_per_sector as usize).page_align_up()
        ) {
            Ok(a) => a,
            Err(err) => {
                pr_err!("Failed to allocate memory for directory entries: {:?}", err);
                return Err(FileError::MemoryError(err));
            }
        };

        loop {
            if let Err(err) = self.read_sectors(
                partition_info,
                directory_list_data,
                self.cluster_to_sector(cluster),
                1,
            ) {
                pr_err!("Failed to read data from disk: {:?}", err);
                return Err(err);
            }

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
                let mut entry_name: [u8; 12] = [0; 12];
                entry_name[0] = if directory_name[0] == 0x05 {
                    0xe5
                } else {
                    directory_name[0]
                };
                let mut p = 1;
                for index in 1..11 {
                    if directory_name[index] == b' ' {
                        continue;
                    }
                    if index == 8 {
                        entry_name[p] = b'.';
                        p += 1;
                    }
                    entry_name[p] = directory_name[index];
                    p += 1;
                }
                let entry_name_ascii = core::str::from_utf8(&entry_name[0..p]).unwrap_or("N/A");
                let file_size = u32::from_le(unsafe { *((entry_base + 28) as *const u32) });

                let entry_cluster =
                    ((u16::from_le(unsafe { *((entry_base + 20) as *const u16) }) as u32) << 16)
                        | u16::from_le(unsafe { *((entry_base + 26) as *const u16) }) as u32;

                let compare_str = |entry_name: &str, target_name: &str| -> bool {
                    if !target_name.is_ascii() {
                        false
                    } else if target_name.len() != entry_name.len() {
                        false
                    } else {
                        for (a, b) in entry_name
                            .as_bytes()
                            .iter()
                            .zip(target_name.as_bytes().iter())
                        {
                            if a.to_ascii_uppercase() != b.to_ascii_uppercase() {
                                return false;
                            }
                        }
                        true
                    }
                };
                if compare_str(entry_name_ascii, target_entry_name) {
                    bug_on_err!(free_pages!(directory_list_data));
                    return Ok(Fat32EntryInfo {
                        entry_cluster,
                        attribute,
                        file_size,
                        file_name: String::from(entry_name_ascii),
                    });
                }

                pointer += DIRECTORY_ENTRY_SIZE;
            }
            bug_on_err!(free_pages!(directory_list_data));
            if limit <= pointer {
                if let Some(next) = self.get_next_cluster(cluster) {
                    cluster = next;
                    continue;
                }
            }
            break;
        }
        Err(FileError::FileNotFound)
    }

    fn list_files(&self, partition_info: &PartitionInfo, mut cluster: u32, indent: usize) {
        let directory_list_data = match alloc_non_linear_pages!(
            MSize::new(self.bytes_per_sector as usize).page_align_up()
        ) {
            Ok(a) => a,
            Err(err) => {
                pr_err!("Failed to allocate memory for directory entries: {:?}", err);
                return;
            }
        };

        loop {
            if let Err(err) = self.read_sectors(
                partition_info,
                directory_list_data,
                self.cluster_to_sector(cluster),
                1,
            ) {
                pr_err!("Failed to read data from disk: {:?}", err);
                return;
            }
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
                let mut entry_name: [u8; 12] = [0; 12];
                entry_name[0] = if directory_name[0] == 0x05 {
                    0xe5
                } else {
                    directory_name[0]
                };
                let mut p = 1;
                for index in 1..11 {
                    if directory_name[index] == b' ' {
                        continue;
                    }
                    if index == 8 {
                        entry_name[p] = b'.';
                        p += 1;
                    }
                    entry_name[p] = directory_name[index];
                    p += 1;
                }
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
            bug_on_err!(free_pages!(directory_list_data));
            if limit <= pointer {
                if let Some(next) = self.get_next_cluster(cluster) {
                    cluster = next;
                    continue;
                }
            }
            break;
        }
    }

    fn read_sectors(
        &self,
        partition_info: &PartitionInfo,
        buffer: VAddress,
        base_sector: u32,
        number_of_sectors: u32,
    ) -> Result<(), FileError> {
        Ok(get_kernel_manager_cluster().block_device_manager.read_lba(
            partition_info.device_id,
            buffer,
            partition_info.starting_lba
                + (base_sector as u64) * (self.bytes_per_sector as u64)
                    / partition_info.lba_block_size,
            (((number_of_sectors as u64) * (self.bytes_per_sector as u64))
                / partition_info.lba_block_size)
                .max(1),
        )?)
    }

    fn cluster_to_sector(&self, cluster: u32) -> u32 {
        (self.reserved_sectors as u32)
            + (self.number_of_fats as u32) * self.fat_sectors
            + (cluster - 2) * (self.sectors_per_cluster as u32)
    }

    fn get_next_cluster(&self, cluster: u32) -> Option<u32> {
        let n =
            unsafe { *((self.fat.to_usize() + cluster as usize * size_of::<u32>()) as *const u32) };
        if n >= 0x0ffffff8 { None } else { Some(n) }
    }
}
