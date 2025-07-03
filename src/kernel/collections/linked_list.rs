//!
//! Linked List
//!
//! This LinkedList allocates the memory by [`Allocator`].
//! The allocation or deallocation may be failed, but methods return errors instead of panic.
//!

use crate::kernel::{
    collections::init_struct,
    collections::ptr_linked_list,
    collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode},
    memory_manager::{
        MemoryError,
        data_type::{Address, MSize, VAddress},
        kfree, kmalloc,
        slab_allocator::{GlobalSlabAllocator, LocalSlabAllocator},
    },
};

use core::marker::PhantomData;
use core::mem::offset_of;
use core::ptr::NonNull;

pub trait Allocator<T> {
    fn alloc(&mut self) -> Result<NonNull<T>, MemoryError>;
    /// Do not drop `address`
    fn free(&mut self, address: NonNull<T>) -> Result<(), MemoryError>;
}

pub struct LinkedList<T, A: Allocator<Node<T>>> {
    list: PtrLinkedList<Node<T>>,
    allocator: A,
}

pub struct Node<T> {
    list: PtrLinkedListNode<Self>,
    data: T,
}

pub struct Iter<'a, T: 'a>(ptr_linked_list::Iter<'a, Node<T>>);
pub struct IterMut<'a, T: 'a>(ptr_linked_list::IterMut<'a, Node<T>>);
pub struct CursorMut<'a, T: 'a, A: Allocator<Node<T>>>(
    ptr_linked_list::CursorMut<'a, Node<T>>,
    PhantomData<A>,
);

impl<T, A: Allocator<Node<T>>> LinkedList<T, A> {
    const OFFSET: usize = core::mem::offset_of!(Node<T>, list);

    pub const fn new_with(allocator: A) -> Self {
        Self {
            list: PtrLinkedList::new(),
            allocator,
        }
    }

    pub fn get_allocator_mut(&mut self) -> &mut A {
        &mut self.allocator
    }

    pub fn iter(&self) -> Iter<'_, T> {
        Iter(unsafe { self.list.iter(Self::OFFSET) })
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut(unsafe { self.list.iter_mut(Self::OFFSET) })
    }

    pub fn cursor_front_mut(&mut self) -> CursorMut<'_, T, A> {
        CursorMut(
            unsafe { self.list.cursor_front_mut(Self::OFFSET) },
            PhantomData,
        )
    }

    pub const fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn clear(&mut self) -> Result<(), MemoryError> {
        while self.pop_front().map_err(|(err, _)| err)?.is_some() {}
        Ok(())
    }

    pub fn front(&self) -> Option<&T> {
        unsafe {
            self.list
                .get_first_entry(Self::OFFSET)
                .map(|n| &((&*n).data))
        }
    }

    pub fn front_mut(&mut self) -> Option<&mut T> {
        unsafe {
            self.list
                .get_first_entry_mut(Self::OFFSET)
                .map(|n| &mut ((&mut *n).data))
        }
    }

    pub fn back(&self) -> Option<&T> {
        unsafe {
            self.list
                .get_last_entry(Self::OFFSET)
                .map(|n| &((&*n).data))
        }
    }

    pub fn back_mut(&mut self) -> Option<&mut T> {
        unsafe {
            self.list
                .get_last_entry_mut(Self::OFFSET)
                .map(|n| &mut ((&mut *n).data))
        }
    }

    pub fn push_front(&mut self, data: T) -> Result<(), MemoryError> {
        self.allocator.alloc().map(|mut n| unsafe {
            let n = &mut *n.as_mut();
            init_struct!(*n, Node::new(data));
            self.list.insert_head(&mut n.list)
        })
    }

    pub fn pop_front(&mut self) -> Result<Option<T>, (MemoryError, T)> {
        if let Some(head) = self
            .list
            .get_first_entry_mut(Self::OFFSET)
            .map(|n| unsafe { &mut *n })
        {
            let d = unsafe { core::ptr::read(&head.data) };
            unsafe { self.list.remove(&mut head.list) };
            match self.allocator.free(NonNull::new(head).unwrap()) {
                Ok(_) => Ok(Some(d)),
                Err(err) => Err((err, d)),
            }
        } else {
            Ok(None)
        }
    }

    pub fn push_back(&mut self, data: T) -> Result<(), MemoryError> {
        self.allocator.alloc().map(|mut n| unsafe {
            let n = &mut *n.as_mut();
            init_struct!(*n, Node::new(data));
            self.list.insert_tail(&mut n.list);
        })
    }

    pub fn pop_back(&mut self) -> Result<Option<T>, (MemoryError, T)> {
        if let Some(back) = self
            .list
            .get_last_entry_mut(Self::OFFSET)
            .map(|n| unsafe { &mut *n })
        {
            let d = unsafe { core::ptr::read(&back.data) };
            unsafe { self.list.remove(&mut back.list) };
            match self.allocator.free(NonNull::new(back).unwrap()) {
                Ok(_) => Ok(Some(d)),
                Err(err) => Err((err, d)),
            }
        } else {
            Ok(None)
        }
    }
}

impl<T, A: Allocator<Node<T>>> Drop for LinkedList<T, A> {
    fn drop(&mut self) {
        if let Err(e) = self.clear() {
            pr_warn!(
                "{} may be failed to drop: {:?}",
                core::any::type_name::<Self>(),
                e
            );
        }
    }
}

impl<T> Node<T> {
    fn new(data: T) -> Self {
        Self {
            list: PtrLinkedListNode::new(),
            data,
        }
    }
}

impl<'a, T: 'a> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|n| &n.data)
    }
}

impl<'a, T: 'a> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|n| &mut n.data)
    }
}

impl<'a, T, A: Allocator<Node<T>>> CursorMut<'a, T, A> {
    /* From `ptr_linked_list.rs` */
    pub fn is_valid(&self) -> bool {
        self.0.is_valid()
    }

    pub fn move_next(&mut self) {
        unsafe { self.0.move_next() }
    }

    pub fn move_prev(&mut self) {
        unsafe { self.0.move_prev() }
    }

    pub fn as_list(&self) -> &LinkedList<T, A> {
        unsafe {
            &*((self.0.as_list() as *const _ as usize - offset_of!(LinkedList<T,A>, list))
                as *const _)
        }
    }

    fn as_list_mut(&mut self) -> &mut LinkedList<T, A> {
        unsafe {
            &mut *((self.0.as_list() as *const _ as usize - offset_of!(LinkedList<T,A>, list))
                as *mut _)
        }
    }

    pub fn current(&mut self) -> Option<&'a mut T> {
        self.0.current().map(|n| unsafe { &mut (*n).data })
    }

    pub fn insert_after(&mut self, data: T) -> Result<(), MemoryError> {
        let next = unsafe { self.as_list_mut().allocator.alloc()?.as_mut() };
        init_struct!(*next, Node::new(data));
        unsafe { self.0.insert_after(&mut next.list) };
        Ok(())
    }

    pub fn insert_before(&mut self, data: T) -> Result<(), MemoryError> {
        let prev = unsafe { self.as_list_mut().allocator.alloc()?.as_mut() };
        init_struct!(*prev, Node::new(data));
        unsafe { self.0.insert_after(&mut prev.list) };
        Ok(())
    }

    pub fn remove_current(&mut self) -> Result<Option<T>, (MemoryError, T)> {
        if let Some(current) = self.0.current().map(|n| unsafe { &mut *n }) {
            let d = unsafe { core::ptr::read(&current.data) };
            unsafe { self.0.remove_current() };
            match self
                .as_list_mut()
                .allocator
                .free(NonNull::new(current).unwrap())
            {
                Ok(_) => Ok(Some(d)),
                Err(err) => Err((err, d)),
            }
        } else {
            Ok(None)
        }
    }

    pub fn remove_current_drop(&mut self) -> Result<(), MemoryError> {
        if let Some(current) = self.0.current().map(|n| unsafe { &mut *n }) {
            unsafe {
                self.0.remove_current();
                core::ptr::drop_in_place(current);
            }
            self.as_list_mut()
                .allocator
                .free(NonNull::new(current).unwrap())
        } else {
            Ok(())
        }
    }
}

#[repr(transparent)]
pub struct GeneralAllocator<T> {
    marker: PhantomData<T>,
}

impl<T> Allocator<T> for GeneralAllocator<T> {
    fn alloc(&mut self) -> Result<NonNull<T>, MemoryError> {
        kmalloc!(MSize::new(size_of::<T>()))
            .and_then(|e| NonNull::new(e.to_usize() as *mut T).ok_or(MemoryError::InvalidAddress))
    }

    fn free(&mut self, address: NonNull<T>) -> Result<(), MemoryError> {
        kfree!(
            VAddress::new(address.as_ptr() as usize),
            MSize::new(size_of::<T>())
        )
    }
}

/// The LinkedList with  [`crate::kernel::memory_manager::kmalloc`]  and  [`crate::kernel::memory_manager::kfree`].
pub type GeneralLinkedList<T> = LinkedList<T, GeneralAllocator<Node<T>>>;

impl<T> GeneralLinkedList<T> {
    pub const fn new() -> Self {
        Self::new_with(GeneralAllocator {
            marker: PhantomData,
        })
    }
}

/// The LinkedList with [`crate::kernel::memory_manager::memory_allocator::LocalSlabAllocator`].
pub type LocalSlabAllocLinkedList<T> = LinkedList<T, LocalSlabAllocator<Node<T>>>;

impl<T> LocalSlabAllocLinkedList<T> {
    pub const fn new() -> Self {
        Self::new_with(LocalSlabAllocator::new())
    }

    pub fn init(&mut self) -> Result<(), MemoryError> {
        self.allocator.init()
    }
}

impl<T> Allocator<T> for LocalSlabAllocator<T> {
    fn alloc(&mut self) -> Result<NonNull<T>, MemoryError> {
        self.alloc()
    }

    fn free(&mut self, address: NonNull<T>) -> Result<(), MemoryError> {
        Ok(self.free(address))
    }
}

/// The LinkedList with [`crate::kernel::memory_manager::memory_allocator::GlobalSlabAllocator`].
pub type GlobalSlabAllocLinkedList<T> = LinkedList<T, GlobalSlabAllocator<Node<T>>>;

impl<T> GlobalSlabAllocLinkedList<T> {
    pub const fn new() -> Self {
        Self::new_with(GlobalSlabAllocator::new())
    }

    pub fn init(&mut self) -> Result<(), MemoryError> {
        self.allocator.init()
    }
}

impl<T> Allocator<T> for GlobalSlabAllocator<T> {
    fn alloc(&mut self) -> Result<NonNull<T>, MemoryError> {
        self.alloc()
    }

    fn free(&mut self, address: NonNull<T>) -> Result<(), MemoryError> {
        Ok(self.free(address))
    }
}
