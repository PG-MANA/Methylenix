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
}

pub struct File<'a> {
    driver: &'a mut dyn FileOperationDriver,
    descriptor: FileDescriptor,
}

impl FileDescriptor {
    pub fn new(data: usize, device_index: usize) -> Self {
        Self {
            data,
            position: 0,
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

    pub fn read(&mut self, buffer: VAddress, length: usize) -> Result<usize, ()> {
        self.driver.read(&mut self.descriptor, buffer, length)
    }

    pub fn write(&mut self, buffer: VAddress, length: usize) -> Result<usize, ()> {
        self.driver.write(&mut self.descriptor, buffer, length)
    }

    pub fn seek(&mut self, offset: usize, origin: FileSeekOrigin) -> Result<usize, ()> {
        self.driver.seek(&mut self.descriptor, offset, origin)
    }

    pub fn close(self) {
        self.driver.close(self.descriptor)
    }
}