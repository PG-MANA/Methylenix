//!
//! File System
//!

use self::file_info::FileInfo;
pub use self::{
    path_info::PathInfo,
    vfs::{
        FILE_PERMISSION_READ, FILE_PERMISSION_WRITE, File, FileDescriptor, FileDescriptorData,
        FileOperationDriver, FileSeekOrigin,
    },
};
use crate::kernel::{
    block_device::BlockDeviceError,
    collections::{guid::Guid, linked_list::GeneralLinkedList},
    manager_cluster::get_kernel_manager_cluster,
    memory_manager::{
        MemoryError, alloc_non_linear_pages,
        data_type::{MOffset, MSize, VAddress},
        free_pages,
    },
    sync::spin_lock::Mutex,
};

pub mod elf;
mod fat32;
mod file_info;
mod gpt;
mod path_info;
mod vfs;
mod xfs;

//#[derive(Clone)]
pub struct PartitionInfo {
    device_id: usize,
    starting_lba: u64,
    #[allow(dead_code)]
    ending_lba: u64,
    lba_block_size: u64,
}

pub struct Partition {
    info: PartitionInfo,
    uuid: Guid,
    driver: Box<dyn PartitionManager>,
}

pub struct FileManager {
    partition_list: GeneralLinkedList<Partition>,
    root: FInfo,
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

    #[allow(dead_code)]
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

impl Default for FileManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FileManager {
    pub fn new() -> Self {
        Self {
            root: FileInfo::new_root(false),
            partition_list: GeneralLinkedList::new(),
        }
    }

    pub fn detect_partitions(&mut self, device_id: usize) {
        gpt::detect_file_system(self, device_id);
    }

    fn analysis_partition(&mut self, partition_info: PartitionInfo) {
        let first_block_data = match alloc_non_linear_pages!(
            MSize::new(partition_info.lba_block_size as usize).page_align_up()
        ) {
            Ok(a) => a,
            Err(err) => {
                pr_err!("Failed to allocate memory: {:?}", err);
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
                    Ok((driver, uuid)) => {
                        pr_debug!("Add: Partition(UUID: {uuid})");
                        bug_on_err!(self.partition_list.push_back(Partition {
                            info: partition_info,
                            uuid,
                            driver: Box::new(driver),
                        }));
                        bug_on_err!(free_pages!(first_block_data));
                        return;
                    }
                    Err(FileError::BadSignature) => { /* Next FS */ }
                    Err(err) => {
                        pr_err!("Failed to detect the file system: {:?}", err);
                        bug_on_err!(free_pages!(first_block_data));
                        return;
                    }
                }
            };
        }

        try_detect!(fat32);
        try_detect!(xfs);

        pr_err!("Unknown File System");
        bug_on_err!(free_pages!(first_block_data));
    }

    pub fn mount_root(&mut self, root_uuid: Guid, is_writable: bool) {
        for e in self.partition_list.iter_mut() {
            if root_uuid == e.uuid {
                e.driver
                    .get_root_node(&e.info, &mut self.root, is_writable)
                    .expect("Failed to create root");
                self.root.driver = e as *mut _;
                return;
            }
        }
        pr_err!("Root is not found");
    }

    /// Temporary function for [`crate::kernel::initialization::mount_root_file_system`]
    pub fn get_first_uuid(&self) -> Option<Guid> {
        self.partition_list.front().map(|e| e.uuid)
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
        if file_name == "." || file_name.is_empty() {
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
            for e in unsafe { current_directory.child.iter_mut(offset_of!(FileInfo, list)) } {
                if e.get_file_name() == file_name
                    && (e.permission_and_flags & permission_and_flags == permission_and_flags)
                {
                    return Ok(e);
                }
            }
        }

        if current_directory.driver.is_null() {
            return Err(FileError::FileNotFound);
        }
        let driver = unsafe { &mut *(current_directory.driver) };
        let f = driver
            .driver
            .search_file(&driver.info, file_name, current_directory)?;
        let file_info = match kmalloc!(FileInfo, f) {
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
        result
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
        Ok(descriptor.get_position())
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
