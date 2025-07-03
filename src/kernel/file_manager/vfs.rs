//!
//! Virtual File System
//!

use super::FileError;

use crate::kernel::memory_manager::data_type::{MOffset, MSize, VAddress};

use core::any::Any;
use core::ptr::NonNull;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum FileSeekOrigin {
    SeekSet,
    SeekCur,
    SeekEnd,
}

pub const FILE_PERMISSION_READ: u8 = 1;
pub const FILE_PERMISSION_WRITE: u8 = 1 << 1;

pub trait FileOperationDriver {
    fn read(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, FileError>;

    fn write(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, FileError>;

    fn seek(
        &mut self,
        descriptor: &mut FileDescriptor,
        offset: MOffset,
        origin: FileSeekOrigin,
    ) -> Result<MOffset, FileError>;

    fn close(&mut self, descriptor: &mut FileDescriptor);
}

#[repr(transparent)]
struct StubDriver {}

static mut STUB_DRIVER: StubDriver = StubDriver {};

impl FileOperationDriver for StubDriver {
    fn read(&mut self, _: &mut FileDescriptor, _: VAddress, _: MSize) -> Result<MSize, FileError> {
        Err(FileError::OperationNotSupported)
    }

    fn write(&mut self, _: &mut FileDescriptor, _: VAddress, _: MSize) -> Result<MSize, FileError> {
        Err(FileError::OperationNotSupported)
    }

    fn seek(
        &mut self,
        _: &mut FileDescriptor,
        _: MOffset,
        _: FileSeekOrigin,
    ) -> Result<MOffset, FileError> {
        Err(FileError::OperationNotSupported)
    }

    fn close(&mut self, _: &mut FileDescriptor) {}
}

pub enum FileDescriptorData {
    Address(usize),
    Data(alloc::boxed::Box<dyn core::any::Any>),
}

pub struct FileDescriptor {
    data: FileDescriptorData,
    position: MOffset,
    device_index: usize,
    permission: u8,
}

pub struct File {
    driver: NonNull<dyn FileOperationDriver>,
    descriptor: FileDescriptor,
}

impl FileDescriptor {
    pub fn new(data: FileDescriptorData, device_index: usize, permission: u8) -> Self {
        Self {
            data,
            position: MOffset::new(0),
            permission,
            device_index,
        }
    }

    pub const fn get_data(&self) -> &FileDescriptorData {
        &self.data
    }

    pub const fn get_data_mut(&mut self) -> &mut FileDescriptorData {
        &mut self.data
    }

    pub const fn get_device_index(&self) -> usize {
        self.device_index
    }

    pub fn add_position(&mut self, position: MOffset) {
        self.position += position;
    }

    pub fn set_position(&mut self, position: MOffset) {
        self.position = position;
    }

    pub const fn get_position(&self) -> MOffset {
        self.position
    }
}

impl File {
    pub fn new(descriptor: FileDescriptor, driver: NonNull<dyn FileOperationDriver>) -> Self {
        Self { descriptor, driver }
    }

    pub fn invalid() -> Self {
        Self {
            descriptor: FileDescriptor::new(FileDescriptorData::Address(0), 0, 0),
            driver: unsafe { NonNull::new_unchecked(&raw mut STUB_DRIVER) },
        }
    }

    pub const fn is_invalid(&self) -> bool {
        self.descriptor.permission == 0
            && matches!(self.descriptor.data, FileDescriptorData::Address(0))
            && self.descriptor.device_index == 0
            && self.descriptor.position.is_zero()
    }

    pub const fn is_readable(&self) -> bool {
        (self.descriptor.permission & FILE_PERMISSION_READ) != 0
    }

    pub const fn is_writable(&self) -> bool {
        (self.descriptor.permission & FILE_PERMISSION_WRITE) != 0
    }

    pub const fn get_descriptor(&self) -> &FileDescriptor {
        &self.descriptor
    }

    pub const fn get_descriptor_mut(&mut self) -> &mut FileDescriptor {
        &mut self.descriptor
    }

    pub fn read(&mut self, buffer: VAddress, length: MSize) -> Result<MSize, FileError> {
        if !self.is_readable() {
            return Err(FileError::OperationNotPermitted);
        }
        unsafe { self.driver.as_mut() }.read(&mut self.descriptor, buffer, length)
    }

    pub fn get_driver(&self) -> NonNull<dyn FileOperationDriver> {
        self.driver
    }

    pub fn is_driver<T: 'static>(&self) -> bool {
        self.driver.type_id() == core::any::TypeId::of::<T>()
    }

    pub fn write(&mut self, buffer: VAddress, length: MSize) -> Result<MSize, FileError> {
        if !self.is_writable() {
            return Err(FileError::OperationNotPermitted);
        }
        unsafe { self.driver.as_mut() }.write(&mut self.descriptor, buffer, length)
    }

    pub fn seek(&mut self, offset: MOffset, origin: FileSeekOrigin) -> Result<MOffset, FileError> {
        unsafe { self.driver.as_mut() }.seek(&mut self.descriptor, offset, origin)
    }
}

impl Default for File {
    fn default() -> Self {
        Self::invalid()
    }
}

impl Drop for File {
    fn drop(&mut self) {
        if !self.is_invalid() {
            unsafe { self.driver.as_mut() }.close(&mut self.descriptor);
        }
    }
}
