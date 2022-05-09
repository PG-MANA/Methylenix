//!
//! Virtual File System
//!

use crate::kernel::memory_manager::data_type::VAddress;

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
    fn read(&mut self, _: &mut FileDescriptor, _: VAddress, _: usize) -> Result<usize, ()> {
        Err(())
    }

    fn write(&mut self, _: &mut FileDescriptor, _: VAddress, _: usize) -> Result<usize, ()> {
        Err(())
    }

    fn seek(&mut self, _: &mut FileDescriptor, _: usize, _: FileSeekOrigin) -> Result<usize, ()> {
        Err(())
    }

    fn close(&mut self, _: FileDescriptor) {}
}

pub trait FileOperationDriver {
    fn read(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: usize,
    ) -> Result<usize, ()>;

    fn write(
        &mut self,
        descriptor: &mut FileDescriptor,
        buffer: VAddress,
        length: usize,
    ) -> Result<usize, ()>;

    fn seek(
        &mut self,
        descriptor: &mut FileDescriptor,
        offset: usize,
        origin: FileSeekOrigin,
    ) -> Result<usize, ()>;

    fn close(&mut self, descriptor: FileDescriptor);
}

pub struct FileDescriptor {
    data: usize,
    position: usize,
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
            position: 0,
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

    pub fn add_position(&mut self, position: usize) {
        self.position += position;
    }
    pub fn set_position(&mut self, position: usize) {
        self.position = position;
    }

    pub const fn get_position(&self) -> usize {
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

    pub fn is_invalid(&self) -> bool {
        self.descriptor.permission == 0
            && self.descriptor.data == 0
            && self.descriptor.device_index == 0
            && self.descriptor.position == 0
    }

    pub fn read(&mut self, buffer: VAddress, length: usize) -> Result<usize, ()> {
        if (self.descriptor.permission & FILE_PERMISSION_READ) == 0 {
            return Err(());
        }
        self.driver.read(&mut self.descriptor, buffer, length)
    }

    pub fn write(&mut self, buffer: VAddress, length: usize) -> Result<usize, ()> {
        if (self.descriptor.permission & FILE_PERMISSION_WRITE) == 0 {
            return Err(());
        }
        self.driver.write(&mut self.descriptor, buffer, length)
    }

    pub fn seek(&mut self, offset: usize, origin: FileSeekOrigin) -> Result<usize, ()> {
        self.driver.seek(&mut self.descriptor, offset, origin)
    }

    pub fn close(self) {
        self.driver.close(self.descriptor)
    }
}
