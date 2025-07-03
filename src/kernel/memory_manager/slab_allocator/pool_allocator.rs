//!
//! Pool Allocator
//!
//! An allocator for fixed size(in the future, maybe able to alloc several size...)
//! This allows nullptr for accessing Physical Address:0
//!

use core::mem::align_of;
use core::ptr::NonNull;

pub struct PoolAllocator<T> {
    linked_count: usize,
    object_size: usize,
    head: Option<NonNull<FreeList>>,
    offset: usize,
    phantom: core::marker::PhantomData<T>,
}

struct FreeList {
    next: Option<NonNull<FreeList>>,
}

/// PoolAllocator
///
/// This allocator is FILO (First In Last Out) to increase the probability of cache-hit.
impl<T> PoolAllocator<T> {
    const fn size_check() {
        let mut object_size = size_of::<T>();
        if object_size < align_of::<T>() {
            object_size = align_of::<T>();
        }
        let mut list_size = size_of::<FreeList>();
        if (object_size & (align_of::<FreeList>() - 1)) != 0 {
            list_size += align_of::<FreeList>() - (object_size & (align_of::<FreeList>() - 1));
        }
        assert!(
            object_size >= list_size,
            "PoolAllocator can process the struct bigger than FreeList only."
        );
    }

    pub const fn new() -> Self {
        Self::size_check();
        let mut object_size = size_of::<T>();
        if object_size < align_of::<T>() {
            object_size = align_of::<T>();
        }
        Self {
            linked_count: 0,
            object_size,
            offset: if (object_size & (align_of::<FreeList>() - 1)) != 0 {
                align_of::<FreeList>() - (object_size & (align_of::<FreeList>() - 1))
            } else {
                0
            },
            head: None,
            phantom: core::marker::PhantomData,
        }
    }

    pub const fn get_count(&self) -> usize {
        self.linked_count
    }

    pub unsafe fn add_pool(&mut self, mut pool_address: usize, mut pool_size: usize) {
        if (pool_address & (align_of::<T>() - 1)) != 0 {
            let padding = align_of::<T>() - (pool_address & (align_of::<T>() - 1));
            pool_address += padding;
            pool_size -= padding;
        }
        for _ in 0..(pool_size / self.object_size) {
            self.free_ptr(pool_address as *mut T);
            pool_address += self.object_size;
        }
    }

    pub fn alloc(&mut self) -> Result<&'static mut T, ()> {
        self.alloc_ptr().map(|p| unsafe { &mut *p })
    }

    pub fn alloc_ptr(&mut self) -> Result<*mut T, ()> {
        if self.linked_count == 0 {
            return Err(());
        }
        assert!(self.head.is_some());
        let mut e = self.head.unwrap();
        self.head = unsafe { e.as_mut().next };
        self.linked_count -= 1;
        Ok((e.as_ptr() as usize - self.offset) as *mut T)
    }

    pub fn free(&mut self, target: &'static mut T) {
        self.free_ptr(target as *mut T)
    }

    pub fn free_ptr(&mut self, target: *mut T) {
        /* Do not use target after free */
        assert!(self.linked_count < usize::MAX);
        let e = (target as usize + self.offset) as *mut FreeList;
        unsafe { (*e).next = self.head };
        self.head = NonNull::new(e);
        assert!(self.head.is_some());
        self.linked_count += 1;
    }
}
