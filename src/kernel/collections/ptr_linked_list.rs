//!
//! Linked List
//!
//! This LinkedList only treat ptr from heap
//! Be careful about ptr accessibly and conflict of mutable access.

use core::marker::PhantomData;
use core::ptr::NonNull;

#[macro_export]
macro_rules! offset_of {
    ($struct_type:ty, $member: tt) => {{
        use crate::kernel::collections::ptr_linked_list::PtrLinkedListNode;
        let target_struct: core::mem::MaybeUninit<$struct_type> =
            core::mem::MaybeUninit::<$struct_type>::uninit();
        let target_struct_ptr: *const $struct_type = target_struct.as_ptr();
        #[allow(unused_unsafe)]
        let target_member_ptr: *const PtrLinkedListNode<$struct_type> =
            unsafe { core::ptr::addr_of!((*target_struct_ptr).$member) };
        #[allow(unused_unsafe)]
        unsafe {
            (target_member_ptr as *const u8).offset_from(target_struct_ptr as *const u8) as usize
        }
    }};
}

pub struct PtrLinkedList<T> {
    head: Option<NonNull<PtrLinkedListNode<T>>>,
    tail: Option<NonNull<PtrLinkedListNode<T>>>,
}

pub struct PtrLinkedListIter<T> {
    next: Option<NonNull<PtrLinkedListNode<T>>>,
    offset: usize,
}

pub struct PtrLinkedListIterMut<T> {
    next: Option<NonNull<PtrLinkedListNode<T>>>,
    offset: usize,
}

pub struct PtrLinkedListNode<T> {
    prev: Option<NonNull<Self>>,
    next: Option<NonNull<Self>>,
    phantom: PhantomData<T>,
}

impl<T> PtrLinkedList<T> {
    pub const fn new() -> Self {
        Self {
            head: None,
            tail: None,
        }
    }

    pub fn insert_head(&mut self, entry: &mut PtrLinkedListNode<T>) {
        if self.head.is_none() {
            assert!(self.tail.is_none());
            entry.unset_prev_and_next();
            self.head = NonNull::new(entry);
            self.tail = self.head;
        } else {
            let mut current_head_ptr = self.head.clone().unwrap();
            let current_head = unsafe { current_head_ptr.as_mut() };
            assert!(current_head.prev.is_none());
            entry.unset_prev_and_next();
            current_head.prev = NonNull::new(entry);
            entry.next = self.head;
            self.head = current_head.prev;
        }
        assert!(self.head.is_some());
    }

    pub fn insert_tail(&mut self, entry: &mut PtrLinkedListNode<T>) {
        if self.tail.is_none() {
            assert!(self.head.is_none());
            self.insert_head(entry);
        } else {
            let mut current_tail_ptr = self.tail.clone().unwrap();
            let current_tail = unsafe { current_tail_ptr.as_mut() };
            assert!(current_tail.next.is_none());
            entry.unset_prev_and_next();
            current_tail.next = NonNull::new(entry);
            entry.prev = self.tail;
            self.tail = current_tail.next;
        }
        assert!(self.tail.is_some());
    }

    pub fn insert_after(
        &mut self,
        list_entry: &mut PtrLinkedListNode<T>,
        entry: &mut PtrLinkedListNode<T>,
    ) {
        if list_entry.next.is_none() {
            self.insert_tail(entry);
        } else {
            entry.unset_prev_and_next();
            list_entry.insert_after(entry);
        }
    }

    pub fn insert_before(
        &mut self,
        list_entry: &mut PtrLinkedListNode<T>,
        entry: &mut PtrLinkedListNode<T>,
    ) {
        if list_entry.prev.is_none() {
            self.insert_head(entry);
        } else {
            entry.unset_prev_and_next();
            list_entry.insert_before(entry);
        }
    }

    pub fn remove(&mut self, list_entry: &mut PtrLinkedListNode<T>) {
        assert!(self.head.is_some());
        if list_entry.prev.is_none() {
            assert_eq!(self.head.unwrap().as_ptr(), list_entry as *mut _);
            if list_entry.next.is_none() {
                self.head = None;
                self.tail = None;
            } else {
                let mut new_head_ptr = list_entry.next.clone().unwrap();
                let new_head = unsafe { new_head_ptr.as_mut() };
                new_head.prev = None;
                self.head = list_entry.next;
            }
        } else if list_entry.next.is_none() {
            assert_eq!(self.tail.unwrap().as_ptr(), list_entry as *mut _);
            let mut new_tail_ptr = list_entry.prev.clone().unwrap();
            let new_tail = unsafe { new_tail_ptr.as_mut() };
            new_tail.next = None;
            self.tail = list_entry.prev;
        } else {
            let mut prev_ptr = list_entry.prev.clone().unwrap();
            let mut next_ptr = list_entry.next.clone().unwrap();
            let prev = unsafe { prev_ptr.as_mut() };
            let next = unsafe { next_ptr.as_mut() };
            prev.next = list_entry.next;
            next.prev = list_entry.prev;
        }
        list_entry.unset_prev_and_next();
    }

    pub unsafe fn take_first_entry(&mut self, offset: usize) -> Option<&'static mut T> {
        if self.is_empty() {
            None
        } else {
            let result = self.get_first_entry_mut(offset).unwrap();
            let mut head_ptr = self.head.clone().unwrap();
            let head = head_ptr.as_mut();
            self.remove(head);
            Some(result)
        }
    }

    pub unsafe fn get_first_entry(&self, offset: usize) -> Option<&'static T> {
        if let Some(e) = self.head {
            let head = e.as_ptr();
            Some(&*((head as usize - offset) as *const T))
        } else {
            None
        }
    }

    pub unsafe fn get_first_entry_mut(&mut self, offset: usize) -> Option<&'static mut T> {
        if let Some(e) = self.head {
            let head = e.as_ptr();
            Some(&mut *((head as usize - offset) as *mut T))
        } else {
            None
        }
    }

    pub unsafe fn get_last_entry(&self, offset: usize) -> Option<&'static T> {
        if let Some(e) = self.tail {
            let tail = e.as_ptr();
            Some(&*((tail as usize - offset) as *const T))
        } else {
            None
        }
    }

    pub unsafe fn get_last_entry_mut(&mut self, offset: usize) -> Option<&'static mut T> {
        if let Some(e) = self.tail {
            let tail = e.as_ptr();
            Some(&mut *((tail as usize - offset) as *mut T))
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    pub unsafe fn iter(&self, offset: usize) -> PtrLinkedListIter<T> {
        PtrLinkedListIter {
            next: self.head,
            offset,
        }
    }

    pub unsafe fn iter_mut(&mut self, offset: usize) -> PtrLinkedListIterMut<T> {
        PtrLinkedListIterMut::<T> {
            next: self.head,
            offset,
        }
    }
}

impl<T: 'static> Iterator for PtrLinkedListIter<T> {
    type Item = &'static T;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(n) = self.next {
            self.next = unsafe { n.as_ref() }.next;
            let head = n.as_ptr();
            Some(unsafe { &*((head as usize - self.offset) as *const T) })
        } else {
            None
        }
    }
}

impl<T: 'static> Iterator for PtrLinkedListIterMut<T> {
    type Item = &'static mut T;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(n) = self.next {
            self.next = unsafe { n.as_ref() }.next;
            let head = n.as_ptr();
            Some(unsafe { &mut *((head as usize - self.offset) as *mut T) })
        } else {
            None
        }
    }
}

impl<T> PtrLinkedListNode<T> {
    pub const fn new() -> Self {
        Self {
            prev: None,
            next: None,
            phantom: PhantomData,
        }
    }

    fn insert_after(&mut self, next: &mut Self) {
        let old_next = self.next;
        self.next = NonNull::new(next);
        next.prev = NonNull::new(self);
        next.next = old_next;
        if let Some(mut e) = &mut next.next {
            unsafe { e.as_mut() }.prev = NonNull::new(next);
        }
    }

    fn insert_before(&mut self, prev: &mut Self) {
        let old_prev = self.prev;
        self.prev = NonNull::new(prev);
        prev.next = NonNull::new(self);
        prev.prev = old_prev;
        if let Some(mut e) = &mut prev.prev {
            unsafe { e.as_mut() }.next = NonNull::new(prev);
        }
    }

    fn unset_prev_and_next(&mut self) {
        self.prev = None;
        self.next = None;
    }

    pub const fn has_next(&self) -> bool {
        self.next.is_some()
    }

    pub const fn has_prev(&self) -> bool {
        self.prev.is_some()
    }

    pub unsafe fn get_next(&self, offset: usize) -> Option<&'static T> {
        if let Some(e) = self.next {
            Some(&*((e.as_ptr() as usize - offset) as *const T))
        } else {
            None
        }
    }

    pub unsafe fn get_next_mut(&self, offset: usize) -> Option<&'static mut T> {
        if let Some(e) = self.next {
            Some(&mut *((e.as_ptr() as usize - offset) as *mut T))
        } else {
            None
        }
    }

    pub unsafe fn get_prev(&self, offset: usize) -> Option<&'static T> {
        if let Some(e) = self.prev {
            Some(&*((e.as_ptr() as usize - offset) as *const T))
        } else {
            None
        }
    }

    pub unsafe fn get_prev_mut(&self, offset: usize) -> Option<&'static mut T> {
        if let Some(e) = self.prev {
            Some(&mut *((e.as_ptr() as usize - offset) as *mut T))
        } else {
            None
        }
    }
}
