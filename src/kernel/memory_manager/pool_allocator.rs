/*
 * Pool Allocator
 * allocator fo fixed size(in future, maybe able to alloc several size...)
 */

/*TODO: think about mutex*/

use core::marker::PhantomData;
use core::mem::size_of;
use core::ptr::NonNull;

pub struct PoolAllocator<T> {
    linked_count: usize,
    object_size: usize,
    head: Option<NonNull<FreeList>>,
    phantom: PhantomData<T>,
}

struct FreeList {
    prev: Option<NonNull<FreeList>>,
    next: Option<NonNull<FreeList>>,
}

impl<T> PoolAllocator<T> {
    pub const fn new() -> Self {
        if size_of::<T>() < size_of::<FreeList>() {
            panic!("PoolAllocator can process the struct bigger than FreeList only.");
            //static_assert
        }
        Self {
            linked_count: 0,
            object_size: size_of::<T>(),
            head: None,
            phantom: PhantomData,
        }
    }

    pub unsafe fn set_initial_pool(&mut self, pool_address: usize, pool_size: usize) {
        assert_eq!(self.linked_count, 0);
        let mut address = pool_address;
        let prev_entry = address as *mut FreeList;
        (*prev_entry).prev = None;
        (*prev_entry).next = None;
        let mut prev_entry = NonNull::new(prev_entry);
        self.head = prev_entry.clone();
        self.linked_count = 1;
        address += self.object_size;
        for _i in 1..(pool_size / self.object_size) {
            let entry = address as *mut FreeList;
            (*entry).prev = prev_entry;
            (*entry).next = None;
            let entry = NonNull::new(entry);
            (*prev_entry.unwrap().as_mut()).next = entry.clone();
            self.linked_count += 1;
            address += self.object_size;
            prev_entry = entry;
        }
    }

    pub fn alloc(&mut self) -> Result<&'static mut T, ()> {
        if self.linked_count == 0 {
            /*add: alloc page from manager*/
            return Err(());
        }
        //assert!(self.head.is_some());
        let mut e = self.head.unwrap();
        if let Some(mut next) = unsafe { e.as_mut() }.next {
            unsafe { next.as_mut() }.prev = None;
            self.head = Some(next);
        } else {
            assert_eq!(self.linked_count, 1);
            self.head = None;
        }
        self.linked_count -= 1;
        Ok(unsafe { &mut *(e.as_mut() as *mut _ as usize as *mut T) })
    }

    pub fn alloc_ptr(&mut self) -> Result<*mut T, ()> {
        if self.linked_count == 0 {
            /*add: alloc page from manager*/
            return Err(());
        }
        //assert!(self.head.is_some());
        let mut e = self.head.unwrap();
        if let Some(mut next) = unsafe { e.as_mut() }.next {
            unsafe { next.as_mut() }.prev = None;
            self.head = Some(next);
        } else {
            assert_eq!(self.linked_count, 1);
            self.head = None;
        }
        self.linked_count -= 1;
        Ok(unsafe { e.as_mut() as *mut _ as usize as *mut T })
    }

    pub fn free(&mut self, target: &'static mut T) {
        /*do not use target after free */
        use core::usize;
        assert!(self.linked_count < usize::MAX);
        let e = target as *mut _ as usize as *mut FreeList;
        if let Some(mut first_entry) = self.head {
            unsafe {
                (*e).next = Some(first_entry);
                let e = NonNull::new(e);
                first_entry.as_mut().prev = e.clone();
                self.head = e;
            }
        } else {
            assert_eq!(self.linked_count, 0);
            unsafe {
                (*e).prev = None;
                (*e).next = None;
            }
            self.head = NonNull::new(e);
        }
        self.linked_count += 1;
    }
}
