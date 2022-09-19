//!
//! XFS
//!

use super::{file_info::FileInfo, FileError, PartitionInfo, PartitionManager};

use crate::kernel::collections::guid::Guid;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MOffset, MSize, VAddress};
use crate::kernel::memory_manager::{alloc_non_linear_pages, free_pages};

type XfsRfsBlock = u64;
type XfsRtBlock = u64;
type XfsIno = u64;
type XfsFsBlock = u64;
type XfsAgBlock = u32;
type XfsAgNumber = u32;
type XfsExtLen = u32;
type XfsLSN = i64;
type XfsFSize = i64;
type XfsTimestamp = u64;
type XfsExtNum = u32;
type XfsAExtNum = u16;

#[repr(C)]
struct SuperBlock {
    magic_number: [u8; 4],
    block_size: u32,
    dblocks: XfsRfsBlock,
    rblocks: XfsRfsBlock,
    rextents: XfsRtBlock,
    uuid: [u8; 16],
    logstart: XfsFsBlock,
    root_inode: XfsIno,
    rbmino: XfsIno,
    rsumino: XfsIno,
    rextsize: XfsAgBlock,
    ag_blocks: XfsAgBlock,
    agcount: XfsAgNumber,
    rbmblocks: XfsExtLen,
    logblocks: XfsExtLen,
    version: u16,
    sectsize: u16,
    inodesize: u16,
    inopblock: u16,
    file_system_name: [u8; 12],
    blocklog: u8,
    sectlog: u8,
    inodelog: u8,
    inopblog: u8,
    agblklog: u8,
    rextslog: u8,
    inprogress: u8,
    imax_pct: u8,
    icount: u64,
    ifree: u64,
    fdblocks: u64,
    frextents: u64,
    uquotino: XfsIno,
    gquotino: XfsIno,
    qflags: u16,
    flags: u8,
    shared_vn: u8,
    inoalignmt: XfsExtLen,
    unit: u32,
    width: u32,
    dirblklog: u8,
    logsectlog: u8,
    logsectsize: u16,
    logsunit: u32,
    features2: u32,
    bad_features2: u32,
    features_compat: u32,
    features_ro_compat: u32,
    features_incompat: u32,
    features_log_incompat: u32,
    crc: u32,
    spino_align: XfsExtLen,
    pquotino: XfsIno,
    lsn: XfsLSN,
    meta_uuid: [u8; 16],
    rrmapino: XfsIno,
}

#[repr(C, packed)]
struct DInodeCore {
    magic: [u8; 2],
    mode: u16,
    version: u8,
    format: u8,
    on_link: u16,
    uid: u32,
    gid: u32,
    n_link: u32,
    projid: u16,
    projid_hi: u16,
    pad: [u8; 6],
    flush_iter: u16,
    atime: XfsTimestamp,
    mtime: XfsTimestamp,
    ctime: XfsTimestamp,
    size: XfsFSize,
    n_blocks: XfsRfsBlock,
    ext_size: XfsExtLen,
    next_entries: XfsExtNum,
    a_next_entries: XfsAExtNum,
    fork_off: u8,
    a_format: i8,
    d_mev_mask: u32,
    d_m_state: u16,
    flags: u16,
    gen: u32,
    be_next_unlinked: u32,
    crc: u32,
    be_change_count: u64,
    be_lsn: u64,
    be_flags2: u64,
    be_cow_ext_size: u32,
    pad2: [u8; 12],
    crtime: XfsTimestamp,
    be_ino: u64,
    uuid: [u8; 16],
}

#[repr(C, packed)]
struct XfsDir2SfHdr {
    count: u8,
    i8_count: u8,
    //parent: XfsIno,
}

const XFS_SB_SIGNATURE: [u8; 4] = *b"XFSB";
const XFS_SB_VERSION_NUMBITS: u16 = 0x000f;
const XFS_SB_VERSION_5: u16 = 5;

const XFS_D_INODE_CORE_SIGNATURE: [u8; 2] = *b"IN";
const XFS_D_INODE_CORE_VERSION_V3: u8 = 3;

const XFS_D_INODE_CORE_FORMAT_LOCAL: u8 = 1;
const XFS_D_INODE_CORE_FORMAT_EXTENTS: u8 = 2;

const XFS_DIR3_FT_DIR: u8 = 2;

pub struct XfsDriver {
    root_inode: u64,
    ag_block_log2: u8,
    inode_per_block_log2: u8,
    ag_block_size: u32,
    block_size_log2: u8,
    inode_size_log2: u8,
}

impl XfsDriver {
    pub fn get_ag(&self, inode_number: u64) -> u64 {
        inode_number >> (self.inode_per_block_log2 + self.ag_block_log2)
    }

    #[allow(dead_code)]
    pub fn get_relative_inode_number(&self, inode_number: u64) -> u64 {
        inode_number & ((1 << (self.ag_block_log2 + self.inode_per_block_log2)) - 1)
    }

    fn get_inode_block_number(&self, inode_number: u64) -> u64 {
        (inode_number >> self.inode_per_block_log2) & ((1 << self.ag_block_log2) - 1)
    }

    fn get_inode_offset_number(&self, inode_number: u64) -> u64 {
        inode_number & ((1 << self.inode_per_block_log2) - 1)
    }

    pub fn get_inode_block(&self, inode_number: u64) -> u64 {
        (self.get_inode_block_number(inode_number)
            + (self.ag_block_size as u64) * self.get_ag(inode_number))
            << self.block_size_log2
    }

    pub fn get_inode_offset(&self, inode_number: u64) -> u64 {
        self.get_inode_offset_number(inode_number) << self.inode_size_log2
    }

    fn list_files(&self, partition_info: &PartitionInfo, inode_number: u64, indent: usize) {
        let inode_block = self.get_inode_block(inode_number);
        let inode_offset = self.get_inode_offset(inode_number);
        pr_debug!(
            "Inode: Block: {:#X}, Offset: {:#X}, AG: {:#X}",
            inode_block,
            inode_offset,
            self.get_ag(inode_number)
        );
        let inode_buffer =
            match alloc_non_linear_pages!(
                MSize::new((1 << self.inode_size_log2) as usize).page_align_up()
            ) {
                Ok(a) => a,
                Err(e) => {
                    pr_err!("Failed to allocate memory for directory entries: {:?}", e);
                    return;
                }
            };
        let block_lba = inode_block as u64 / partition_info.lba_block_size;
        let block_offset = inode_block - (block_lba * partition_info.lba_block_size);
        if let Err(e) = get_kernel_manager_cluster().block_device_manager.read_lba(
            partition_info.device_id,
            inode_buffer,
            partition_info.starting_lba + block_lba,
            ((block_offset + (1 << self.block_size_log2)) / partition_info.lba_block_size).max(1),
        ) {
            pr_err!("Failed to read inode block: {:?}", e);
            let _ = free_pages!(inode_buffer);
            return;
        }
        let inode = unsafe {
            &*((inode_buffer + MSize::new((block_offset + inode_offset) as usize)).to_usize()
                as *const DInodeCore)
        };
        if inode.magic != XFS_D_INODE_CORE_SIGNATURE {
            pr_err!("Invalid inode(number: {:#X})", inode_number);
            let _ = free_pages!(inode_buffer);
            return;
        } else if inode.version != XFS_D_INODE_CORE_VERSION_V3 {
            pr_err!(
                "Invalid inode version(number: {:#X}, Version: {:#X})",
                inode_number,
                inode.version
            );
            let _ = free_pages!(inode_buffer);
            return;
        }
        if inode.format != XFS_D_INODE_CORE_FORMAT_LOCAL {
            pr_err!("inode is not the directory format.");
            let _ = free_pages!(inode_buffer);
            return;
        }

        let inode_sf_hdr = unsafe {
            &*((inode as *const _ as usize + core::mem::size_of::<DInodeCore>())
                as *const XfsDir2SfHdr)
        };
        pr_debug!(
            "Number of entries: {:#X}(Is64bit: {:#X})",
            inode_sf_hdr.count,
            inode_sf_hdr.i8_count
        );

        let dir_entries_base_address = inode_sf_hdr as *const _ as usize
            + core::mem::size_of::<XfsDir2SfHdr>()
            + if inode_sf_hdr.i8_count != 0 {
                core::mem::size_of::<u64>()
            } else {
                core::mem::size_of::<u32>()
            };
        let mut pointer = dir_entries_base_address;
        for _ in 0..inode_sf_hdr.count {
            let name_len = unsafe { *(pointer as *const u8) };
            pointer += core::mem::size_of::<u8>();
            let offset = u16::from_be(unsafe { *(pointer as *const u16) });
            pointer += core::mem::size_of::<u16>();
            let name = core::str::from_utf8(unsafe {
                core::slice::from_raw_parts(pointer as *const u8, name_len as usize)
            })
            .unwrap_or("????");
            pointer += name_len as usize;
            let file_type = unsafe { *(pointer as *const u8) };
            pointer += core::mem::size_of::<u8>();
            let entry_inode_number = if inode_sf_hdr.i8_count != 0 {
                let p = pointer;
                pointer += core::mem::size_of::<u64>();
                u64::from_be(unsafe { *(p as *const u64) })
            } else {
                let p = pointer;
                pointer += core::mem::size_of::<u32>();
                u32::from_be(unsafe { *(p as *const u32) }) as u64
            };
            for _ in 0..indent {
                kprint!(" ");
            }
            kprintln!(
                "|- {}: Offset: {:#X}, FileType: {:#X}, Inode: {:#X}",
                name,
                offset,
                file_type,
                entry_inode_number
            );
            if (file_type & XFS_DIR3_FT_DIR) != 0 && name.len() != 0 {
                self.list_files(partition_info, entry_inode_number, indent + 1);
            }
        }
        let _ = free_pages!(inode_buffer);
    }

    fn read_inode(
        &self,
        partition_info: &PartitionInfo,
        inode_number: u64,
    ) -> Result<(VAddress, MOffset), FileError> {
        let inode_block = self.get_inode_block(inode_number);
        let inode_offset = self.get_inode_offset(inode_number);
        let inode_buffer =
            match alloc_non_linear_pages!(
                MSize::new((1 << self.inode_size_log2) as usize).page_align_up()
            ) {
                Ok(a) => a,
                Err(err) => {
                    pr_err!("Failed to allocate memory for directory entries: {:?}", err);
                    return Err(FileError::MemoryError(err));
                }
            };
        let block_lba = inode_block as u64 / partition_info.lba_block_size;
        let block_offset = inode_block - (block_lba * partition_info.lba_block_size);
        if let Err(err) = get_kernel_manager_cluster().block_device_manager.read_lba(
            partition_info.device_id,
            inode_buffer,
            partition_info.starting_lba + block_lba,
            ((block_offset + (1 << self.block_size_log2)) / partition_info.lba_block_size).max(1),
        ) {
            pr_err!("Failed to read inode block: {:?}", err);
            let _ = free_pages!(inode_buffer);
            return Err(FileError::DeviceError);
        }
        Ok((
            inode_buffer,
            MSize::new((block_offset + inode_offset) as usize),
        ))
    }

    fn search_file_local_inode(
        &self,
        inode_sf_hdr: &XfsDir2SfHdr,
        name: &str,
        current_directory: &mut FileInfo,
    ) -> Result<(FileInfo, u8 /* File Type */), FileError> {
        let dir_entries_base_address = inode_sf_hdr as *const _ as usize
            + core::mem::size_of::<XfsDir2SfHdr>()
            + if inode_sf_hdr.i8_count != 0 {
                core::mem::size_of::<u64>()
            } else {
                core::mem::size_of::<u32>()
            };

        let count = if inode_sf_hdr.count != 0 {
            inode_sf_hdr.count
        } else {
            inode_sf_hdr.i8_count
        };
        let mut pointer = dir_entries_base_address;

        for _ in 0..count {
            let name_len = unsafe { *(pointer as *const u8) };
            pointer += core::mem::size_of::<u8>();
            let _offset = u16::from_be(unsafe { *(pointer as *const u16) });
            pointer += core::mem::size_of::<u16>();
            let entry_name = core::str::from_utf8(unsafe {
                core::slice::from_raw_parts(pointer as *const u8, name_len as usize)
            })
            .unwrap_or("N/A");
            pointer += name_len as usize;
            let file_type = unsafe { *(pointer as *const u8) };
            pointer += core::mem::size_of::<u8>();
            let entry_inode_number = if inode_sf_hdr.i8_count != 0 {
                let p = pointer;
                pointer += core::mem::size_of::<u64>();
                u64::from_be(unsafe { *(p as *const u64) })
            } else {
                let p = pointer;
                pointer += core::mem::size_of::<u32>();
                u32::from_be(unsafe { *(p as *const u32) }) as u64
            };
            if name.len() == name_len as usize && name == entry_name {
                let mut file_info = FileInfo::new(current_directory);
                file_info.set_file_name_str(entry_name);
                file_info.set_inode_number(entry_inode_number);
                if (file_type & XFS_DIR3_FT_DIR) != 0 {
                    file_info.set_attribute_directory();
                }
                return Ok((file_info, file_type));
            }
        }
        return Err(FileError::FileNotFound);
    }

    fn read_file_extents_inode(
        &self,
        partition_info: &PartitionInfo,
        _file_info: &mut FileInfo,
        inode: &DInodeCore,
        mut offset: MOffset,
        mut length: MSize,
        buffer: VAddress,
    ) -> Result<MSize, FileError> {
        let extent_list_base = inode as *const _ as usize + core::mem::size_of::<DInodeCore>();
        let number_of_extent_records = u32::from_be(inode.next_entries) as usize;
        let mut buffer_pointer = MSize::new(0);
        let mut page_buffer = VAddress::new(0);
        let mut page_buffer_size = MSize::new(0);

        for i in 0..number_of_extent_records {
            let record = unsafe { &*((extent_list_base + i * (128 / 8)) as *const [u8; 16]) };
            //let flag = record[0] >> 7;
            let block_offset =
                (u64::from_be_bytes(unsafe { *(record[0..7].as_ptr() as *const [u8; 8]) })
                    & !(1 << 63))
                    >> (73 - 64);
            let block_number =
                (u64::from_be_bytes(unsafe { *(record[0..7].as_ptr() as *const [u8; 8]) })
                    & ((1 << (73 - 64)) - 1))
                    | (u64::from_be_bytes(unsafe { *(record[8..15].as_ptr() as *const [u8; 8]) })
                        >> 21);
            let number_of_blocks =
                u64::from_be_bytes(unsafe { *(record[8..15].as_ptr() as *const [u8; 8]) })
                    & ((1 << 21) - 1);

            if offset < MSize::new((block_offset << self.block_size_log2) as usize) {
                let hole_size =
                    MSize::new((block_offset << self.block_size_log2) as usize) - offset;
                let size_to_zero_clear = hole_size.min(length);
                pr_debug!(
                    "TODO: Debug: hole size: {hole_size}, offset: {offset}, length: {length}"
                );
                unsafe {
                    core::ptr::write_bytes(
                        (buffer + buffer_pointer).to_usize() as *mut u8,
                        0,
                        size_to_zero_clear.to_usize(),
                    )
                };
                buffer_pointer += size_to_zero_clear;
                length -= size_to_zero_clear;
                offset = MSize::new(0);
                if length.is_zero() {
                    break;
                }
            }
            if offset >= MSize::new((number_of_blocks << self.block_size_log2) as usize) {
                offset -= MSize::new((number_of_blocks << self.block_size_log2) as usize);
                continue;
            }

            let read_length = length.min(MSize::new(
                (number_of_blocks << self.block_size_log2) as usize,
            ));
            let block = ((block_number >> self.ag_block_log2) * self.ag_block_size as u64)
                + (block_number & ((1 << self.ag_block_log2) - 1));
            let lba_base = if (1 << self.block_size_log2) > partition_info.lba_block_size {
                block * ((1 << self.block_size_log2) / partition_info.lba_block_size)
            } else {
                block / (partition_info.lba_block_size / (1 << self.block_size_log2))
            } + (offset.to_usize() as u64 / partition_info.lba_block_size);
            let data_offset = ((1 << self.block_size_log2) + offset.to_usize() as u64)
                % partition_info.lba_block_size;
            let total_read_size = read_length + MSize::new(data_offset as usize);
            let number_of_blocks =
                (total_read_size.to_usize() as u64 - 1) / partition_info.lba_block_size + 1;

            if total_read_size > page_buffer_size {
                let new_allocation_size = total_read_size.page_align_up();
                if !page_buffer_size.is_zero() {
                    let _ = free_pages!(page_buffer);
                }
                page_buffer = match alloc_non_linear_pages!(new_allocation_size) {
                    Ok(a) => a,
                    Err(err) => {
                        pr_err!("Failed to allocate memory for read: {:?}", err);
                        return Err(FileError::MemoryError(err));
                    }
                };
                page_buffer_size = new_allocation_size;
            }
            get_kernel_manager_cluster().block_device_manager.read_lba(
                partition_info.device_id,
                page_buffer,
                partition_info.starting_lba + lba_base,
                number_of_blocks,
            )?;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (page_buffer.to_usize() + data_offset as usize) as *const u8,
                    (buffer + buffer_pointer).to_usize() as *mut u8,
                    read_length.to_usize(),
                )
            };
            buffer_pointer += read_length;
            length -= read_length;
            offset = MSize::new(0);
            if length.is_zero() {
                break;
            }
        }

        if !page_buffer_size.is_zero() {
            let _ = free_pages!(page_buffer);
        }
        return Ok(buffer_pointer);
    }

    fn analysis_file_and_set_file_info(
        &self,
        partition_info: &PartitionInfo,
        file_info: &mut FileInfo,
    ) -> Result<(), FileError> {
        let (inode_buffer, inode_offset) =
            self.read_inode(partition_info, file_info.get_inode_number())?;
        let inode = unsafe { &*((inode_buffer + inode_offset).to_usize() as *const DInodeCore) };
        if inode.magic != XFS_D_INODE_CORE_SIGNATURE {
            pr_err!("Invalid inode(number: {:#X})", file_info.get_inode_number());
            let _ = free_pages!(inode_buffer);
            return Err(FileError::BadSignature);
        } else if inode.version != XFS_D_INODE_CORE_VERSION_V3 {
            pr_err!(
                "Invalid inode version(number: {:#X}, Version: {:#X})",
                file_info.get_inode_number(),
                inode.version
            );
            let _ = free_pages!(inode_buffer);
            return Err(FileError::BadSignature);
        }

        file_info.set_file_size(i64::from_be(inode.size) as u64);
        file_info.set_permission_by_mode(u16::from_be(inode.mode));
        file_info.set_uid(u32::from_be(inode.uid));
        file_info.set_gid(u32::from_be(inode.gid));

        let _ = free_pages!(inode_buffer);
        Ok(())
    }
}

pub(super) fn try_mount_file_system(
    partition_info: &PartitionInfo,
    first_4k_data: VAddress,
) -> Result<(XfsDriver, Guid), FileError> {
    let super_block = unsafe { &*(first_4k_data.to_usize() as *const SuperBlock) };
    if super_block.magic_number != XFS_SB_SIGNATURE
        || (super_block.version & XFS_SB_VERSION_NUMBITS.to_be()) != XFS_SB_VERSION_5.to_be()
    {
        return Err(FileError::BadSignature);
    }
    pr_debug!("XFS UUID: {}", Guid::new_be(&super_block.uuid));
    pr_debug!(
        "File System Name: {}",
        core::str::from_utf8(&super_block.file_system_name).unwrap_or("")
    );

    let xfs_info = XfsDriver {
        root_inode: u64::from_be(super_block.root_inode),
        ag_block_log2: super_block.agblklog,
        inode_per_block_log2: super_block.inopblog,
        ag_block_size: u32::from_be(super_block.ag_blocks),
        block_size_log2: super_block.blocklog,
        inode_size_log2: super_block.inodelog,
    };

    pr_debug!(
        "Block Size: {}(AG Block Size: {:#X})",
        u32::from_be(super_block.block_size),
        xfs_info.ag_block_size
    );
    pr_debug!("Root inode: {}", xfs_info.root_inode);

    xfs_info.list_files(partition_info, xfs_info.root_inode, 0);
    return Ok((xfs_info, Guid::new_be(&super_block.uuid)));
}

impl PartitionManager for XfsDriver {
    fn get_root_node(
        &mut self,
        partition_info: &PartitionInfo,
        file_info: &mut FileInfo,
        _is_writable: bool,
    ) -> Result<(), FileError> {
        file_info.set_inode_number(self.root_inode);
        self.analysis_file_and_set_file_info(partition_info, file_info)?;
        Ok(())
    }

    fn search_file(
        &self,
        partition_info: &PartitionInfo,
        file_name: &str,
        current_directory: &mut FileInfo,
    ) -> Result<FileInfo, FileError> {
        let (inode_buffer, inode_offset) =
            self.read_inode(partition_info, current_directory.get_inode_number())?;
        let inode = unsafe { &*((inode_buffer + inode_offset).to_usize() as *const DInodeCore) };
        if inode.magic != XFS_D_INODE_CORE_SIGNATURE {
            pr_err!(
                "Invalid inode(number: {:#X})",
                current_directory.get_inode_number()
            );
            let _ = free_pages!(inode_buffer);
            return Err(FileError::BadSignature);
        } else if inode.version != XFS_D_INODE_CORE_VERSION_V3 {
            pr_err!(
                "Invalid inode version(number: {:#X}, Version: {:#X})",
                current_directory.get_inode_number(),
                inode.version
            );
            let _ = free_pages!(inode_buffer);
            return Err(FileError::BadSignature);
        }

        let mut file_info: FileInfo;
        let _file_type: u8;

        match inode.format {
            XFS_D_INODE_CORE_FORMAT_LOCAL => {
                let inode_sf_hdr = unsafe {
                    &*((inode as *const _ as usize + core::mem::size_of::<DInodeCore>())
                        as *const XfsDir2SfHdr)
                };
                let result =
                    self.search_file_local_inode(inode_sf_hdr, file_name, current_directory);
                if let Err(err) = result {
                    let _ = free_pages!(inode_buffer);
                    return Err(err);
                }
                (file_info, _file_type) = result.unwrap();
            }
            format => {
                pr_err!("Unsupported Format: {:#X}", format);
                let _ = free_pages!(inode_buffer);
                return Err(FileError::OperationNotSupported);
            }
        }

        self.analysis_file_and_set_file_info(partition_info, &mut file_info)?;
        file_info.driver = current_directory.driver;

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
        if file_info.is_directory() {
            return Err(FileError::OperationNotPermitted);
        }
        if offset + length > MSize::new(file_info.get_file_size() as usize) {
            if offset >= MSize::new(file_info.get_file_size() as usize) {
                return Ok(MSize::new(0));
            }
            length -= MSize::new(file_info.get_file_size() as usize) - offset;
        }
        let length = length;

        let (inode_buffer, inode_offset) =
            self.read_inode(partition_info, file_info.get_inode_number())?;
        let inode = unsafe { &*((inode_buffer + inode_offset).to_usize() as *const DInodeCore) };

        if inode.magic != XFS_D_INODE_CORE_SIGNATURE {
            pr_err!("Invalid inode(number: {:#X})", file_info.get_inode_number());
            let _ = free_pages!(inode_buffer);
            return Err(FileError::BadSignature);
        } else if inode.version != XFS_D_INODE_CORE_VERSION_V3 {
            pr_err!(
                "Invalid inode version(number: {:#X}, Version: {:#X})",
                file_info.get_inode_number(),
                inode.version
            );
            let _ = free_pages!(inode_buffer);
            return Err(FileError::BadSignature);
        }

        match inode.format {
            XFS_D_INODE_CORE_FORMAT_EXTENTS => {
                let result = self.read_file_extents_inode(
                    partition_info,
                    file_info,
                    inode,
                    offset,
                    length,
                    buffer,
                );
                let _ = free_pages!(inode_buffer);
                result
            }
            format => {
                pr_err!("Unsupported Format: {:#X}", format);
                let _ = free_pages!(inode_buffer);
                Err(FileError::OperationNotSupported)
            }
        }
    }

    fn close_file(&self, _partition_info: &PartitionInfo, _file_info: &mut FileInfo) {}
}
