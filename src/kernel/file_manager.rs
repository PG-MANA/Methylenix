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

use alloc::boxed::Box;
use alloc::sync::Arc;

use core::ptr::NonNull;

pub mod elf;
mod fat32;
mod file_info;
mod gpt;
mod path_info;
mod vfs;
mod xfs;

pub type FInfo = Arc<Mutex<FileInfo>>;

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
        current_directory: &FileInfo,
    ) -> Result<FileInfo, FileError>;

    fn get_file_size(
        &self,
        partition_info: &PartitionInfo,
        file_info: &FileInfo,
    ) -> Result<u64, FileError>;

    fn read_file(
        &self,
        partition_info: &PartitionInfo,
        file_info: &FileInfo,
        offset: MOffset,
        length: MSize,
        buffer: VAddress,
    ) -> Result<MSize, FileError>;

    fn write_file(
        &self,
        partition_info: &PartitionInfo,
        file_info: &FileInfo,
        offset: MOffset,
        length: MSize,
        buffer: VAddress,
    ) -> Result<MSize, FileError>;

    fn close_file(&self, partition_info: &PartitionInfo, file_info: &FileInfo);
}

impl Default for FileManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FileManager {
    pub fn new() -> Self {
        Self {
            partition_list: GeneralLinkedList::new(),
            root: Arc::new(Mutex::new(FileInfo::new_root(false))),
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
                let mut root = self.root.lock().unwrap();
                e.driver
                    .get_root_node(&e.info, &mut root, is_writable)
                    .expect("Failed to create root");
                root.partition = e;
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
        let FileDescriptorData::Data(b) = descriptor.get_data() else {
            return Err(FileError::OperationNotSupported);
        };
        let f_i = b
            .as_ref()
            .downcast_ref::<FInfo>()
            .ok_or(FileError::OperationNotSupported)?;
        let f_i = f_i.lock().unwrap();
        let partition = unsafe { &*(f_i.partition) };

        partition.driver.get_file_size(&partition.info, &f_i)
    }

    fn _open_file_info(
        &mut self,
        file_name: &str,
        current_directory: FInfo,
        permission_and_flags: u16,
    ) -> Result<FInfo, FileError> {
        if file_name == "." || file_name.is_empty() {
            return Ok(current_directory.clone());
        } else if file_name == ".." {
            let f_i = current_directory.lock().unwrap();
            return match &f_i.parent {
                Some(p) => p.upgrade().ok_or(FileError::FileNotFound),
                None => Ok(current_directory.clone()),
            };
        }
        let mut f_i = current_directory.lock().unwrap();
        if (f_i.permission_and_flags & FileInfo::FLAGS_VOLATILE) == 0 {
            for e in f_i.child.iter_mut() {
                let locked_e = e.lock().unwrap();
                if locked_e.get_file_name() == file_name
                    && (locked_e.permission_and_flags & permission_and_flags
                        == permission_and_flags)
                {
                    drop(locked_e);
                    return Ok(e.clone());
                }
            }
        }

        let partition = unsafe { &mut *(f_i.partition) };
        let mut f = partition
            .driver
            .search_file(&partition.info, file_name, &f_i)?;
        if f.permission_and_flags & permission_and_flags != permission_and_flags {
            return Err(FileError::OperationNotPermitted);
        }
        if f.partition.is_null() {
            f.partition = f_i.partition;
        }
        f.parent = Some(Arc::downgrade(&current_directory));
        let f = Arc::try_new(Mutex::new(f))
            .map_err(|e| FileError::MemoryError(MemoryError::from(e)))?;
        f_i.child.push_back(f.clone())?;
        Ok(f)
    }

    pub fn open_file_info(
        &mut self,
        file_name: &PathInfo,
        current_directory: Option<&FInfo>,
        _permission: u8,
    ) -> Result<FInfo, FileError> {
        let mut dir = if file_name.is_absolute_path() {
            self.root.clone()
        } else {
            current_directory.ok_or(FileError::FileNotFound)?.clone()
        };
        let permission_and_flags = 0; //permission
        for e in file_name.iter() {
            dir = self._open_file_info(e, dir, permission_and_flags)?;
        }
        Ok(dir.clone())
    }

    pub fn open_file_info_as_file(
        &mut self,
        info: &FInfo,
        permission: u8,
    ) -> Result<File, FileError> {
        let f_i = info.lock().unwrap();
        if (f_i.permission_and_flags & FileInfo::FLAGS_DIRECTORY) != 0
            || (f_i.permission_and_flags & FileInfo::FLAGS_META_DARA) != 0
        {
            return Err(FileError::InvalidFile);
        }
        /* TODO: permission check based on user/group */

        drop(f_i);
        Ok(File::new(
            FileDescriptor::new(
                FileDescriptorData::Data(Box::new(info.clone())),
                0,
                permission,
            ),
            NonNull::new(self).unwrap(),
        ))
    }

    pub fn open_file(
        &mut self,
        file_name: &PathInfo,
        current_directory: Option<&FInfo>,
        permission: u8,
    ) -> Result<File, FileError> {
        let file_info = self.open_file_info(file_name, current_directory, permission)?;
        self.open_file_info_as_file(&file_info, permission)
    }
}

impl FileOperationDriver for FileManager {
    fn read(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, FileError> {
        let FileDescriptorData::Data(b) = descriptor.get_data() else {
            return Err(FileError::OperationNotSupported);
        };
        let f_i = b
            .as_ref()
            .downcast_ref::<FInfo>()
            .ok_or(FileError::OperationNotSupported)?;
        let f_i = f_i.lock().unwrap();
        let partition = unsafe { &*(f_i.partition) };

        let result = partition.driver.read_file(
            &partition.info,
            &f_i,
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
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, FileError> {
        let FileDescriptorData::Data(b) = descriptor.get_data() else {
            return Err(FileError::OperationNotSupported);
        };
        let f_i = b
            .as_ref()
            .downcast_ref::<FInfo>()
            .ok_or(FileError::OperationNotSupported)?;
        let f_i = f_i.lock().unwrap();
        let partition = unsafe { &*(f_i.partition) };

        let result = partition.driver.write_file(
            &partition.info,
            &f_i,
            descriptor.get_position(),
            length,
            buffer,
        );

        if let Ok(s) = result {
            descriptor.add_position(s);
        }
        result
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

    fn close(&mut self, descriptor: &mut FileDescriptor) {
        let FileDescriptorData::Data(b) = descriptor.get_data() else {
            return;
        };
        let Some(f_i) = b.as_ref().downcast_ref::<FInfo>() else {
            return;
        };
        let f_i = f_i.lock().unwrap();
        let partition = unsafe { &*(f_i.partition) };

        partition.driver.close_file(&partition.info, &f_i);
    }
}
