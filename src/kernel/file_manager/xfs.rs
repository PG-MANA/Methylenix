//!
//! XFS
//!

use crate::kernel::collections::guid::Guid;
use crate::kernel::file_manager::{FileError, PartitionInfo, PartitionManager, PathInfo};
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
    uuid: Guid,
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
    meta_uuid: Guid,
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
    uuid: Guid,
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
const XFS_D_INODE_CORE_FORMAT_NORMAL: u8 = 1;

const XFS_DIR3_FT_DIR: u8 = 2;

pub struct XfsInfo {
    root_inode: u64,
    ag_block_log2: u8,
    inode_per_block_log2: u8,
    ag_blocks: u32,
    block_size_log2: u8,
    inode_size_log2: u8,
}

impl XfsInfo {
    pub fn get_ag(&self, inode_number: u64) -> u64 {
        inode_number >> (self.inode_per_block_log2 + self.ag_block_log2)
    }

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
            + (self.ag_blocks as u64) * self.get_ag(inode_number))
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
        if inode.format != XFS_D_INODE_CORE_FORMAT_NORMAL {
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
}

pub(super) fn try_detect_file_system(
    partition_info: &PartitionInfo,
    first_4k_data: VAddress,
) -> Result<XfsInfo, FileError> {
    let super_block = unsafe { &*(first_4k_data.to_usize() as *const SuperBlock) };
    if super_block.magic_number != XFS_SB_SIGNATURE
        || (super_block.version & XFS_SB_VERSION_NUMBITS.to_be()) != XFS_SB_VERSION_5.to_be()
    {
        return Err(FileError::BadSignature);
    }
    pr_debug!("XFS UUID: {}", super_block.uuid);
    pr_debug!(
        "File System Name: {}",
        core::str::from_utf8(&super_block.file_system_name).unwrap_or("")
    );

    let xfs_info = XfsInfo {
        root_inode: u64::from_be(super_block.root_inode),
        ag_block_log2: super_block.agblklog,
        inode_per_block_log2: super_block.inopblog,
        ag_blocks: u32::from_be(super_block.ag_blocks),
        block_size_log2: super_block.blocklog,
        inode_size_log2: super_block.inodelog,
    };

    pr_debug!(
        "Block Size: {}(AG Blocks: {:#X})",
        u32::from_be(super_block.block_size),
        xfs_info.ag_blocks
    );
    pr_debug!("Root inode: {}", xfs_info.root_inode);

    xfs_info.list_files(partition_info, xfs_info.root_inode, 0);
    return Ok(xfs_info);
}

impl PartitionManager for XfsInfo {
    fn search_file(
        &self,
        partition_info: &PartitionInfo,
        file_name: &PathInfo,
    ) -> Result<usize, FileError> {
        todo!()
    }

    fn get_file_size(
        &self,
        partition_info: &PartitionInfo,
        file_info: usize,
    ) -> Result<usize, FileError> {
        todo!()
    }

    fn read_file(
        &self,
        partition_info: &PartitionInfo,
        file_info: usize,
        offset: MOffset,
        length: MSize,
        buffer: VAddress,
    ) -> Result<MSize, FileError> {
        todo!()
    }

    fn close_file(&self, partition_info: &PartitionInfo, file_info: usize) {
        todo!()
    }
}
