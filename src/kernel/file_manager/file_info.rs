//!
//! File Information containing inode number and file name
//!

use crate::kernel::{
    collections::linked_list::GeneralLinkedList, file_manager::Partition, sync::spin_lock::Mutex,
};

use alloc::string::String;

pub type InodeNumber = u64;

pub struct FileInfo {
    inode_number: InodeNumber,
    file_name: String,
    file_size: u64,
    pub permission_and_flags: u16,
    uid: u32,
    gid: u32,
    pub driver: *mut Partition,
    pub parent: *mut FileInfo,
    pub child: GeneralLinkedList<Arc<Mutex<Self>>>,
}

impl FileInfo {
    pub const PERMISSION_USER_OFFSET: u16 = 6;
    pub const PERMISSION_GROUP_OFFSET: u16 = 3;
    pub const PERMISSION_OTHER_OFFSET: u16 = 0;

    pub const PERMISSION_FLAG_READ: u16 = 1 << 2;
    pub const PERMISSION_FLAG_WRITE: u16 = 1 << 1;
    pub const PERMISSION_FLAG_EXECUTE: u16 = 1 << 0;
    pub const FLAGS_DIRECTORY: u16 = 1 << 9;
    pub const FLAGS_META_DARA: u16 = 1 << 10;
    pub const FLAGS_VOLATILE: u16 = 1 << 15;

    pub const fn new(parent: &mut Self) -> Self {
        Self {
            inode_number: 0,
            file_name: String::new(),
            file_size: 0,
            permission_and_flags: 0,
            uid: 0,
            gid: 0,
            driver: core::ptr::null_mut(),
            parent,
            child: GeneralLinkedList::new(),
        }
    }

    pub const fn new_root(is_root_writable: bool) -> Self {
        let permission = Self::PERMISSION_FLAG_EXECUTE
            | (if is_root_writable {
                Self::PERMISSION_FLAG_WRITE
            } else {
                0
            })
            | Self::PERMISSION_FLAG_READ;

        Self {
            inode_number: 0,
            file_name: String::new(),
            file_size: 0,
            permission_and_flags: (permission << Self::PERMISSION_USER_OFFSET)
                | (permission << Self::PERMISSION_GROUP_OFFSET)
                | (permission << Self::PERMISSION_OTHER_OFFSET)
                | Self::FLAGS_DIRECTORY,
            uid: 0,
            gid: 0,
            driver: core::ptr::null_mut(),
            parent: core::ptr::null_mut(),
            child: GeneralLinkedList::new(),
        }
    }

    pub const fn get_inode_number(&self) -> InodeNumber {
        self.inode_number
    }

    pub fn set_inode_number(&mut self, inode_number: InodeNumber) {
        self.inode_number = inode_number;
    }

    pub fn get_file_size(&self) -> u64 {
        self.file_size
    }

    pub fn set_file_size(&mut self, file_size: u64) {
        self.file_size = file_size;
    }

    pub fn get_file_name(&self) -> &str {
        self.file_name.as_str()
    }

    pub fn set_file_name(&mut self, name: String) {
        self.file_name = name;
    }

    pub fn set_file_name_str(&mut self, name: &str) {
        self.file_name = String::from(name);
    }

    pub const fn is_directory(&self) -> bool {
        (self.permission_and_flags & Self::FLAGS_DIRECTORY) != 0
    }

    pub fn set_attribute_directory(&mut self) {
        self.permission_and_flags |= Self::FLAGS_DIRECTORY;
    }

    pub fn set_attribute_meta_file(&mut self) {
        self.permission_and_flags |= Self::FLAGS_META_DARA;
    }

    pub fn set_permission(&mut self, user: u16, group: u16, other: u16) {
        let all_permission = Self::PERMISSION_FLAG_EXECUTE
            | Self::PERMISSION_FLAG_WRITE
            | Self::PERMISSION_FLAG_READ;
        self.permission_and_flags &= !((all_permission << Self::PERMISSION_USER_OFFSET)
            | (all_permission << Self::PERMISSION_GROUP_OFFSET)
            | (all_permission << Self::PERMISSION_OTHER_OFFSET));

        self.permission_and_flags |= (user << Self::PERMISSION_USER_OFFSET)
            | (group << Self::PERMISSION_GROUP_OFFSET)
            | (other << Self::PERMISSION_OTHER_OFFSET);
    }

    pub fn set_permission_by_mode(&mut self, mode: u16) {
        let mode = mode & 0b111111111;
        let all_permission = Self::PERMISSION_FLAG_EXECUTE
            | Self::PERMISSION_FLAG_WRITE
            | Self::PERMISSION_FLAG_READ;
        self.permission_and_flags &= !((all_permission << Self::PERMISSION_USER_OFFSET)
            | (all_permission << Self::PERMISSION_GROUP_OFFSET)
            | (all_permission << Self::PERMISSION_OTHER_OFFSET));

        self.permission_and_flags |= mode;
    }

    pub fn get_uid(&self) -> u32 {
        self.uid
    }

    pub fn set_uid(&mut self, uid: u32) {
        self.uid = uid;
    }

    pub fn get_gid(&self) -> u32 {
        self.gid
    }

    pub fn set_gid(&mut self, gid: u32) {
        self.gid = gid;
    }
}
