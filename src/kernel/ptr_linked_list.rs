//!
//! Linked List
//!
//! This LinkedList only treat ptr from heap
//! Be careful about ptr accessibly and conflict of mutable access.

use core::marker::PhantomData;
use core::ptr::NonNull;

pub struct PtrLinkedList<T> {
    head: Option<NonNull<PtrLinkedListNode<T>>>,
}

pub struct PtrLinkedListIter<T> {
    head: Option<NonNull<PtrLinkedListNode<T>>>,
    phantom: PhantomData<PtrLinkedListNode<T>>,
}

pub struct PtrLinkedListIterMut<T> {
    head: Option<NonNull<PtrLinkedListNode<T>>>,
    phantom: PhantomData<PtrLinkedListNode<T>>,
}

pub struct PtrLinkedListNode<T> {
    prev: Option<NonNull<Self>>,
    next: Option<NonNull<Self>>,
    ptr: Option<NonNull<T>>,
}

impl<T> PtrLinkedList<T> {
    pub const fn new() -> Self {
        Self { head: None }
    }

    pub fn set_first_entry(&mut self, entry: Option<*mut PtrLinkedListNode<T>>) {
        if entry.is_some() {
            self.head = NonNull::new(entry.unwrap());
        } else {
            self.head = None;
        }
    }

    pub fn get_first_entry_as_ptr(&self) -> Option<*const T> {
        if let Some(e) = self.head {
            if let Some(ptr) = unsafe { e.as_ref().ptr } {
                Some(ptr.clone().as_ptr())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub unsafe fn get_first_entry(&self) -> Option<&'static T> {
        if let Some(e) = self.get_first_entry_as_ptr() {
            Some(&*e)
        } else {
            None
        }
    }

    pub fn get_first_entry_mut_as_ptr(&mut self) -> Option<*mut T> {
        if let Some(mut e) = self.head {
            if let Some(ptr) = unsafe { e.as_mut().ptr } {
                Some(ptr.clone().as_ptr())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub unsafe fn get_first_entry_mut(&mut self) -> Option<&'static mut T> {
        if let Some(e) = self.get_first_entry_mut_as_ptr() {
            Some(&mut *e)
        } else {
            None
        }
    }

    pub unsafe fn get_last_entry_mut(&mut self) -> Option<&'static mut T> {
        if self.head.is_none() {
            None
        } else {
            let mut e = self.head.clone().unwrap();
            while let Some(next) = e.as_mut().next.clone() {
                e = next;
            }
            Some(&mut *e.as_mut().get_ptr()?)
        }
    }

    pub fn iter(&self) -> PtrLinkedListIter<T> {
        PtrLinkedListIter {
            head: self.head,
            phantom: PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> PtrLinkedListIterMut<T> {
        PtrLinkedListIterMut {
            head: self.head,
            phantom: PhantomData,
        }
    }
}

impl<T> Iterator for PtrLinkedListIter<T> {
    type Item = *const T;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(e) = self.head {
            self.head = unsafe { e.as_ref().next };
            if let Some(ptr) = unsafe { e.as_ref().get_ptr() } {
                return Some(ptr);
            }
        }
        None
    }
}

impl<T> Iterator for PtrLinkedListIterMut<T> {
    type Item = *mut T;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(e) = self.head {
            self.head = unsafe { e.as_ref().next };
            if let Some(ptr) = unsafe { e.as_ref().get_ptr() } {
                return Some(ptr);
            }
        }
        None
    }
}

impl<T> PtrLinkedListNode<T> {
    pub const fn new() -> Self {
        Self {
            prev: None,
            next: None,
            ptr: None,
        }
    }

    pub fn get_ptr(&self) -> Option<*mut T> {
        if let Some(ptr) = self.ptr {
            Some(ptr.clone().as_ptr())
        } else {
            None
        }
    }

    pub fn set_ptr(&mut self, ptr: *mut T) {
        self.ptr = NonNull::new(ptr);
    }

    pub unsafe fn set_ptr_from_usize(&mut self, ptr: usize) {
        self.ptr = NonNull::new(ptr as *mut T)
    }

    pub fn is_invalid_ptr(&self) -> bool {
        self.ptr.is_none()
    }

    pub fn insert_after(&mut self, next: &mut Self) {
        assert!(next.ptr.is_some());
        assert!(next.get_next_as_ptr().is_none());
        assert!(next.get_prev_as_ptr().is_none());
        let old_next = self.next;
        self.next = NonNull::new(next);
        next.prev = NonNull::new(self);
        next.next = old_next;
        if let Some(mut e) = &mut next.next {
            unsafe { e.as_mut() }.prev = NonNull::new(next);
        }
    }

    pub fn insert_before(&mut self, prev: &mut Self) {
        assert!(prev.ptr.is_some());
        let old_prev = self.prev;
        self.prev = NonNull::new(prev);
        prev.next = NonNull::new(self);
        prev.prev = old_prev;
        if let Some(mut e) = &mut prev.prev {
            unsafe { e.as_mut() }.next = NonNull::new(prev);
        }
    }

    pub fn setup_to_be_root(&mut self, old_root: &mut Self) {
        assert!(self.get_next_as_ptr().is_none());
        assert!(old_root.get_prev_as_ptr().is_none());
        self.terminate_prev_entry();
        self.next = NonNull::new(old_root);
        old_root.prev = NonNull::new(self);
    }

    pub fn terminate_prev_entry(&mut self) {
        self.prev = None;
    }

    pub fn remove_from_list(&mut self, list: &mut PtrLinkedList<T>) {
        if let Some(mut next) = self.next {
            unsafe { next.as_mut() }.prev = self.prev;
        }
        if let Some(mut prev) = self.prev {
            unsafe { prev.as_mut() }.next = self.next;
        } else {
            /* self is root */
            if self.next.is_none() {
                list.set_first_entry(None);
            } else {
                list.set_first_entry(Some(self.next.unwrap().as_ptr()));
            }
        }
        self.prev = None;
        self.next = None;
    }

    pub fn get_next_as_ptr(&self) -> Option<*const T> {
        if let Some(e) = self.next {
            if let Some(p) = unsafe { e.as_ref().ptr.clone() } {
                return Some(unsafe { &*p.as_ptr() });
            }
        }
        return None;
    }

    pub unsafe fn get_next(&self) -> Option<&'static T> {
        if let Some(e) = self.get_next_as_ptr() {
            Some(&*e)
        } else {
            None
        }
    }

    pub fn get_next_mut_as_ptr(&mut self) -> Option<*mut T> {
        if let Some(mut e) = self.next {
            if let Some(p) = unsafe { e.as_mut().ptr.clone() } {
                return Some(p.as_ptr());
            }
        }
        return None;
    }

    pub unsafe fn get_next_mut(&mut self) -> Option<&'static mut T> {
        if let Some(e) = self.get_next_mut_as_ptr() {
            Some(&mut *e)
        } else {
            None
        }
    }

    pub fn get_prev_as_ptr(&self) -> Option<*const T> {
        if let Some(e) = self.prev {
            if let Some(p) = unsafe { e.as_ref().ptr.clone() } {
                return Some(p.as_ptr());
            }
        }
        return None;
    }

    pub unsafe fn get_prev(&self) -> Option<&'static T> {
        if let Some(e) = self.get_prev_as_ptr() {
            Some(&*e)
        } else {
            None
        }
    }

    pub fn get_prev_mut_as_ptr(&mut self) -> Option<*mut T> {
        if let Some(mut e) = self.prev {
            if let Some(p) = unsafe { e.as_mut().ptr.clone() } {
                return Some(p.as_ptr());
            }
        }
        return None;
    }

    pub unsafe fn get_prev_mut(&mut self) -> Option<&'static mut T> {
        if let Some(e) = self.get_prev_mut_as_ptr() {
            Some(&mut *e)
        } else {
            None
        }
    }
}
