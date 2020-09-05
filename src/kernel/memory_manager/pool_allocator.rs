//!
//! Pool Allocator
//! An allocator for fixed size(in future, maybe able to alloc several size...)
//! This allows nullptr for accessing Physical Address:0
//!

pub struct PoolAllocator<T> {
    linked_count: usize,
    object_size: usize,
    head: Option<*mut FreeList>,
    phantom: core::marker::PhantomData<T>,
}

struct FreeList {
    next: Option<*mut FreeList>,
}

/// PoolAllocator
///
/// This allocator is FILO(First In Last Out) to increase the probability of cache-hit.
impl<T> PoolAllocator<T> {
    const SIZE_CHECK_HOOK: () = Self::size_check();

    const fn size_check() {
        if core::mem::size_of::<T>() < core::mem::size_of::<FreeList>() {
            panic!("PoolAllocator can process the struct bigger than FreeList only.");
            //static_assert
        }
    }

    pub const fn new() -> Self {
        let _c = Self::SIZE_CHECK_HOOK;
        Self {
            linked_count: 0,
            object_size: core::mem::size_of::<T>(),
            head: None,
            phantom: core::marker::PhantomData,
        }
    }

    pub const fn new_with_align(align: usize) -> Self {
        let _c = Self::SIZE_CHECK_HOOK;
        let size = if align < core::mem::size_of::<T>() {
            core::mem::size_of::<T>()
        } else {
            align
        };
        Self {
            linked_count: 0,
            object_size: size,
            head: None,
            phantom: core::marker::PhantomData,
        }
    }

    pub const fn get_count(&self) -> usize {
        self.linked_count
    }

    pub unsafe fn set_initial_pool(&mut self, pool_address: usize, pool_size: usize) {
        assert_eq!(self.linked_count, 0);
        let mut address = pool_address;
        let mut prev_entry = address as *mut FreeList;
        (*prev_entry).next = None;
        self.head = Some(prev_entry.clone());
        self.linked_count = 1;
        address += self.object_size;
        for _i in 1..(pool_size / self.object_size) {
            let entry = address as *mut FreeList;
            (*entry).next = None;
            (*prev_entry).next = Some(entry.clone());
            self.linked_count += 1;
            address += self.object_size;
            prev_entry = entry;
        }
    }

    pub fn alloc(&mut self) -> Result<&'static mut T, ()> {
        if let Ok(ptr) = self.alloc_ptr() {
            Ok(unsafe { &mut *ptr })
        } else {
            Err(())
        }
    }

    pub fn alloc_ptr(&mut self) -> Result<*mut T, ()> {
        if self.linked_count == 0 {
            return Err(());
        }
        //assert!(self.head.is_some());
        let e = self.head.unwrap().clone();
        self.head = unsafe { (&mut *e).next };
        self.linked_count -= 1;
        Ok(e as usize as *mut T)
    }

    pub fn free(&mut self, target: &'static mut T) {
        self.free_ptr(target as *mut T)
    }

    pub fn free_ptr(&mut self, target: *mut T) {
        /*do not use target after free */
        assert!(self.linked_count < core::usize::MAX);
        let e = target as usize as *mut FreeList;
        unsafe { (&mut *e).next = self.head };
        self.head = Some(e);
        self.linked_count += 1;
    }
}
