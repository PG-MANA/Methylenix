//!
//! Virtual File System
//!

use crate::kernel::memory_manager::data_type::{MOffset, MSize, VAddress};

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum FileSeekOrigin {
    SeekSet,
    SeekCur,
    SeekEnd,
}

pub const FILE_PERMISSION_READ: u8 = 1;
pub const FILE_PERMISSION_WRITE: u8 = 1 << 1;

#[repr(transparent)]
struct FakeDriver {}
static mut FAKE_DRIVER: FakeDriver = FakeDriver {};

impl FileOperationDriver for FakeDriver {
    fn read(&mut self, _: &mut FileDescriptor, _: VAddress, _: MSize) -> Result<MSize, ()> {
        Err(())
    }

    fn write(&mut self, _: &mut FileDescriptor, _: VAddress, _: MSize) -> Result<MSize, ()> {
        Err(())
    }

    fn seek(
        &mut self,
        _: &mut FileDescriptor,
        _: MOffset,
        _: FileSeekOrigin,
    ) -> Result<MOffset, ()> {
        Err(())
    }

    fn close(&mut self, _: FileDescriptor) {}
}

pub trait FileOperationDriver {
    fn read(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, ()>;

    fn write(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: MSize,
    ) -> Result<MSize, ()>;

    fn seek(
        &mut self,
        descriptor: &mut FileDescriptor,
        offset: MOffset,
        origin: FileSeekOrigin,
    ) -> Result<MOffset, ()>;

    fn close(&mut self, descriptor: FileDescriptor);
}

pub struct FileDescriptor {
    data: usize,
    position: MOffset,
    device_index: usize,
    permission: u8,
}

pub struct File<'a> {
    driver: &'a mut dyn FileOperationDriver,
    descriptor: FileDescriptor,
}

impl FileDescriptor {
    pub fn new(data: usize, device_index: usize, permission: u8) -> Self {
        Self {
            data,
            position: MOffset::new(0),
            permission,
            device_index,
        }
    }

    pub const fn get_data(&self) -> usize {
        self.data
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

impl<'a> File<'a> {
    pub fn new(descriptor: FileDescriptor, driver: &'a mut dyn FileOperationDriver) -> Self {
        Self { descriptor, driver }
    }

    pub fn new_invalid() -> Self {
        Self {
            descriptor: FileDescriptor::new(0, 0, 0),
            driver: unsafe { &mut FAKE_DRIVER },
        }
    }

    pub const fn is_invalid(&self) -> bool {
        self.descriptor.permission == 0
            && self.descriptor.data == 0
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

    pub fn get_driver_address(&self) -> usize {
        (self.driver as *const dyn FileOperationDriver)
            .to_raw_parts()
            .0 as usize
    }

    pub fn read(&mut self, buffer: VAddress, length: MSize) -> Result<MSize, ()> {
        if !self.is_readable() {
            return Err(());
        }
        self.driver.read(&mut self.descriptor, buffer, length)
    }

    pub fn write(&mut self, buffer: VAddress, length: MSize) -> Result<MSize, ()> {
        if !self.is_writable() {
            return Err(());
        }
        self.driver.write(&mut self.descriptor, buffer, length)
    }

    pub fn seek(&mut self, offset: MOffset, origin: FileSeekOrigin) -> Result<MOffset, ()> {
        self.driver.seek(&mut self.descriptor, offset, origin)
    }

    pub fn close(self) {
        self.driver.close(self.descriptor)
    }
}
