use crate::kernel::memory_manager::data_type::*;
use crate::memory::allocate_pages;

pub struct PhysicalMemoryManager {}

impl PhysicalMemoryManager {
    pub const fn new() -> Self {
        Self {}
    }

    pub fn alloc(&mut self, size: MSize, _order: MOrder) -> Result<PAddress, ()> {
        /* TODO: clean */
        allocate_pages(size.to_index().to_usize())
            .map(|a| PAddress::new(a))
            .ok_or(())
    }

    pub fn free(&mut self, _address: PAddress, _size: MSize, _: bool) -> Result<(), ()> {
        Err(())
    }
}
