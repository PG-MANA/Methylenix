//!
//! File System
//!

pub mod elf;
mod fat32;
mod file_info;
mod gpt;
mod path_info;
mod vfs;
mod xfs;

use self::file_info::FileInfo;
pub use self::path_info::{PathInfo, PathInfoIter};
pub use self::vfs::{
    File, FileDescriptor, FileOperationDriver, FileSeekOrigin, FILE_PERMISSION_READ,
    FILE_PERMISSION_WRITE,
};

use crate::kernel::block_device::BlockDeviceError;
use crate::kernel::collections::ptr_linked_list::{
    offset_of_list_node, PtrLinkedList, PtrLinkedListNode,
};
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{MOffset, MSize, VAddress};
use crate::kernel::memory_manager::{alloc_non_linear_pages, free_pages, kmalloc, MemoryError};

use alloc::boxed::Box;

//#[derive(Clone)]
pub struct PartitionInfo {
    device_id: usize,
    starting_lba: u64,
    #[allow(dead_code)]
    ending_lba: u64,
    lba_block_size: u64,
}

pub struct Partition {
    list: PtrLinkedListNode<Self>,
    info: PartitionInfo,
    driver: Box<dyn PartitionManager>,
}

pub struct FileManager {
    partition_list: PtrLinkedList<Partition>,
    root: FileInfo,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum FileError {
    MemoryError(MemoryError),
    BadSignature,
    FileNotFound,
    InvalidFile,
    OperationNotPermitted,
    OperationNotSupported,
    DeviceError,
}

impl From<MemoryError> for FileError {
    fn from(m: MemoryError) -> Self {
        Self::MemoryError(m)
    }
}

impl From<BlockDeviceError> for FileError {
    fn from(b: BlockDeviceError) -> Self {
        if let BlockDeviceError::MemoryError(m) = b {
            Self::MemoryError(m)
        } else {
            Self::DeviceError
        }
    }
}

trait PartitionManager {
    fn get_root_node(
        &mut self,
        partition_info: &PartitionInfo,
        file_info: &mut FileInfo,
        is_writable: bool,
    ) -> Result<(), FileError>;

    fn search_file(
        &self,
        partition_info: &PartitionInfo,
        file_name: &str,
        current_directory: &mut FileInfo,
    ) -> Result<FileInfo, FileError>;

    fn get_file_size(
        &self,
        partition_info: &PartitionInfo,
        file_info: &FileInfo,
    ) -> Result<u64, FileError>;

    fn read_file(
        &self,
        partition_info: &PartitionInfo,
        file_info: &mut FileInfo,
        offset: MOffset,
        length: MSize,
        buffer: VAddress,
    ) -> Result<MSize, FileError>;

    fn close_file(&self, partition_info: &PartitionInfo, file_info: &mut FileInfo);
}

impl FileManager {
    pub fn new() -> Self {
        Self {
            partition_list: PtrLinkedList::new(),
            root: FileInfo::new_root(false),
        }
    }

    pub fn detect_partitions(&mut self, device_id: usize) {
        gpt::detect_file_system(self, device_id);
    }

    fn analysis_partition(&mut self, partition_info: PartitionInfo) {
        let first_block_data =
            match alloc_non_linear_pages!(
                MSize::new(partition_info.lba_block_size as usize).page_align_up()
            ) {
                Ok(a) => a,
                Err(e) => {
                    pr_err!("Failed to allocate memory: {:?}", e);
                    return;
                }
            };
        if let Err(e) = get_kernel_manager_cluster().block_device_manager.read_lba(
            partition_info.device_id,
            first_block_data,
            partition_info.starting_lba,
            1,
        ) {
            pr_err!("Failed to read data from disk: {:?}", e);
            return;
        }

        macro_rules! try_detect {
            ($fs:ident) => {
                match $fs::try_mount_file_system(&partition_info, first_block_data) {
                    Ok(driver) => {
                        match kmalloc!(
                            Partition,
                            Partition {
                                list: PtrLinkedListNode::new(),
                                info: partition_info,
                                driver: Box::new(driver)
                            }
                        ) {
                            Ok(i) => {
                                self.partition_list.insert_tail(&mut i.list);
                            }
                            Err(err) => {
                                pr_err!("Failed to allocate partition information: {:?}", err);
                            }
                        }
                        let _ = free_pages!(first_block_data);
                        return;
                    }
                    Err(FileError::BadSignature) => { /* Next FS */ }
                    Err(err) => {
                        pr_err!("Failed to detect the file system: {:?}", err);
                        let _ = free_pages!(first_block_data);
                        return;
                    }
                }
            };
        }

        try_detect!(fat32);
        try_detect!(xfs);

        pr_err!("Unknown File System");
        let _ = free_pages!(first_block_data);
        return;
    }

    fn get_file_size(&self, descriptor: &FileDescriptor) -> Result<u64, FileError> {
        Ok(unsafe { &*(descriptor.get_data() as *const FileInfo) }.get_file_size())
    }

    fn _open_file_info(
        &mut self,
        file_name: &str,
        current_directory: &mut FileInfo,
        permission_and_flags: u16,
    ) -> Result<&'static mut FileInfo, FileError> {
        if file_name == "." || file_name.len() == 0 {
            return Ok(unsafe { &mut *(current_directory as *mut _) });
        } else if file_name == ".." {
            return if current_directory.parent.is_null() {
                assert_ne!(current_directory as *mut _, &mut self.root as *mut _);
                Ok(unsafe { &mut *(current_directory as *mut _) })
            } else {
                Ok(unsafe { &mut *(current_directory.parent) })
            };
        }
        let _lock = current_directory.lock.lock();
        if (current_directory.permission_and_flags & FileInfo::FLAGS_VOLATILE) == 0 {
            for e in unsafe {
                current_directory
                    .child
                    .iter_mut(offset_of_list_node!(FileInfo, list))
            } {
                if e.get_file_name() == file_name
                    && (e.permission_and_flags & permission_and_flags == permission_and_flags)
                {
                    return Ok(e);
                }
            }
        }

        let driver = unsafe { &mut *(current_directory.driver) };
        let f = driver
            .driver
            .search_file(&driver.info, file_name, current_directory)?;
        let mut file_info = match kmalloc!(FileInfo, f) {
            Ok(i) => i,
            Err(err) => {
                pr_err!("Failed to allocate FileInfo: {:?}", err);
                return Err(FileError::MemoryError(err));
            }
        };

        current_directory.reference_counter += 1;
        file_info.parent = current_directory;
        file_info.list = PtrLinkedListNode::new();
        if file_info.driver.is_null() {
            file_info.driver = driver;
        }
        current_directory.child.insert_tail(&mut file_info.list);

        if file_info.permission_and_flags & permission_and_flags == permission_and_flags {
            Ok(file_info)
        } else {
            Err(FileError::OperationNotPermitted)
        }
    }

    pub fn open_file_info(
        &mut self,
        file_name: &PathInfo,
        current_directory: &mut FileInfo,
        _permission: u8,
    ) -> Result<&'static mut FileInfo, FileError> {
        let mut dir = if file_name.is_absolute_path() {
            unsafe { &mut *(&mut self.root as *mut _) }
        } else {
            unsafe { &mut *(current_directory as *mut _) }
        };
        let permission_and_flags = 0; //permission
        for e in file_name.iter() {
            dir = self._open_file_info(e, dir, permission_and_flags)?;
        }
        Ok(dir)
    }

    pub fn open_file_info_as_file(
        &mut self,
        info: &mut FileInfo,
        permission: u8,
    ) -> Result<File, FileError> {
        let _lock = info.lock.lock();
        if (info.permission_and_flags & FileInfo::FLAGS_DIRECTORY) != 0
            || (info.permission_and_flags & FileInfo::FLAGS_META_DARA) != 0
        {
            return Err(FileError::InvalidFile);
        }
        /* TODO: permission check based on user/group */

        info.reference_counter += 1;

        Ok(File::new(
            FileDescriptor::new(info as *mut _ as usize, 0, permission),
            self,
        ))
    }

    pub fn open_file(
        &mut self,
        file_name: &PathInfo,
        current_directory: Option<&mut FileInfo>,
        permission: u8,
    ) -> Result<File, FileError> {
        let current_directory =
            current_directory.unwrap_or(unsafe { &mut *(&mut self.root as *mut _) });
        let file_info = self.open_file_info(file_name, current_directory, permission)?;
        self.open_file_info_as_file(file_info, permission)
    }
}

impl FileOperationDriver for FileManager {
    fn read(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, FileError> {
        let file_info = unsafe { &mut *(descriptor.get_data() as *mut FileInfo) };
        let _lock = file_info.lock.lock();

        let partition_info = unsafe { &mut *(file_info.driver) };

        let result = partition_info.driver.read_file(
            &mut partition_info.info,
            file_info,
            descriptor.get_position(),
            length,
            buffer,
        );

        if let Ok(s) = result {
            descriptor.add_position(s);
        }
        return result;
    }

    fn write(
        &mut self,
        _descriptor: &mut FileDescriptor,
        _buffer: VAddress,
        _length: MSize,
    ) -> Result<MSize, FileError> {
        Err(FileError::OperationNotSupported)
    }

    fn seek(
        &mut self,
        descriptor: &mut FileDescriptor,
        offset: MOffset,
        origin: FileSeekOrigin,
    ) -> Result<MOffset, FileError> {
        match origin {
            FileSeekOrigin::SeekSet => descriptor.set_position(offset),
            FileSeekOrigin::SeekCur => descriptor.add_position(offset),
            FileSeekOrigin::SeekEnd => {
                let pos = self.get_file_size(descriptor)? as usize;
                descriptor.set_position(MOffset::new(pos));
            }
        }
        return Ok(descriptor.get_position());
    }

    fn close(&mut self, descriptor: FileDescriptor) {
        let file_info = unsafe { &mut *(descriptor.get_data() as *mut FileInfo) };
        let _lock = file_info.lock.lock();
        let partition_info = unsafe { &mut *(file_info.driver) };

        partition_info
            .driver
            .close_file(&mut partition_info.info, file_info);

        file_info.reference_counter -= 1;
        if file_info.reference_counter == 0 { /*TODO: delete file info */ }
    }
}
