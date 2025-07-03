//!
//! Pointer Linked List
//!
//! This LinkedList only treats ptr from heap
//! Before using [`PtrLinkedList`], please consider using [`crate::kernel::collections::LinkedList`] instead.
//! Be careful about ptr accessibly and conflict of mutable access.

use core::marker::PhantomData;
use core::ptr::NonNull;

pub struct PtrLinkedList<T> {
    head: Option<NonNull<PtrLinkedListNode<T>>>,
    tail: Option<NonNull<PtrLinkedListNode<T>>>,
}

pub struct PtrLinkedListNode<T> {
    prev: Option<NonNull<Self>>,
    next: Option<NonNull<Self>>,
    phantom: PhantomData<T>,
}

pub struct Iter<'a, T: 'a> {
    next: Option<NonNull<PtrLinkedListNode<T>>>,
    offset: usize,
    phantom: PhantomData<&'a T>,
}

pub struct IterMut<'a, T: 'a> {
    next: Option<NonNull<PtrLinkedListNode<T>>>,
    offset: usize,
    phantom: PhantomData<&'a T>,
}

pub struct CursorMut<'a, T: 'a> {
    current: Option<NonNull<PtrLinkedListNode<T>>>,
    offset: usize,
    list: &'a mut PtrLinkedList<T>,
}

impl<T> PtrLinkedList<T> {
    pub const fn new() -> Self {
        Self {
            head: None,
            tail: None,
        }
    }

    pub const unsafe fn insert_head(&mut self, entry: &mut PtrLinkedListNode<T>) {
        if let Some(mut head) = self.head {
            let head = unsafe { head.as_mut() };
            assert!(head.prev.is_none());
            entry.unset_prev_and_next();
            head.prev = NonNull::new(entry);
            entry.next = self.head;
            self.head = head.prev;
        } else {
            assert!(self.tail.is_none());
            entry.unset_prev_and_next();
            self.head = NonNull::new(entry);
            self.tail = self.head;
        }
        assert!(self.head.is_some());
    }

    pub const unsafe fn insert_tail(&mut self, entry: &mut PtrLinkedListNode<T>) {
        if let Some(mut tail) = self.tail {
            let tail = unsafe { tail.as_mut() };
            assert!(tail.next.is_none());
            tail.next = NonNull::new(entry);
            entry.prev = self.tail;
            self.tail = tail.next;
            entry.next = None;
        } else {
            assert!(self.head.is_none());
            unsafe { self.insert_head(entry) };
        }
        assert!(self.tail.is_some());
    }

    pub unsafe fn insert_after(
        &mut self,
        entry: &mut PtrLinkedListNode<T>,
        next: &mut PtrLinkedListNode<T>,
    ) {
        if entry.next.is_none() {
            unsafe { self.insert_tail(next) };
        } else {
            next.unset_prev_and_next();
            unsafe { entry.insert_after(next) };
        }
    }

    pub unsafe fn insert_before(
        &mut self,
        entry: &mut PtrLinkedListNode<T>,
        prev: &mut PtrLinkedListNode<T>,
    ) {
        if entry.prev.is_none() {
            unsafe { self.insert_head(prev) };
        } else {
            prev.unset_prev_and_next();
            unsafe { entry.insert_before(prev) };
        }
    }

    pub unsafe fn remove(&mut self, entry: &mut PtrLinkedListNode<T>) {
        assert!(self.head.is_some());
        if let Some(mut prev) = entry.prev
            && let Some(mut next) = entry.next
        {
            unsafe { prev.as_mut() }.next = entry.next;
            unsafe { next.as_mut() }.prev = entry.prev;
        } else if let Some(mut prev) = entry.prev {
            assert!(core::ptr::eq(self.tail.unwrap().as_ptr(), entry));
            unsafe { prev.as_mut() }.next = None;
            self.tail = entry.prev;
        } else if let Some(mut next) = entry.next {
            unsafe { next.as_mut() }.prev = None;
            self.head = entry.next;
        } else {
            self.head = None;
            self.tail = None;
        }
        entry.unset_prev_and_next();
    }

    pub unsafe fn take_first_entry(&mut self, offset: usize) -> Option<*mut T> {
        if self.is_empty() {
            None
        } else {
            let result = self.get_first_entry_mut(offset).unwrap();
            let mut head_ptr = self.head.unwrap();
            let head = unsafe { head_ptr.as_mut() };
            unsafe { self.remove(head) };
            Some(result)
        }
    }

    pub fn get_first_entry(&self, offset: usize) -> Option<*const T> {
        if let Some(e) = self.head {
            let head = e.as_ptr();
            Some((head as usize - offset) as *const T)
        } else {
            None
        }
    }

    pub fn get_first_entry_mut(&mut self, offset: usize) -> Option<*mut T> {
        if let Some(e) = self.head {
            let head = e.as_ptr();
            Some((head as usize - offset) as *mut T)
        } else {
            None
        }
    }

    pub fn get_last_entry(&self, offset: usize) -> Option<*const T> {
        if let Some(e) = self.tail {
            let tail = e.as_ptr();
            Some((tail as usize - offset) as *const T)
        } else {
            None
        }
    }

    pub fn get_last_entry_mut(&mut self, offset: usize) -> Option<*mut T> {
        if let Some(e) = self.tail {
            let tail = e.as_ptr();
            Some((tail as usize - offset) as *mut T)
        } else {
            None
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    pub unsafe fn iter(&self, offset: usize) -> Iter<'_, T> {
        Iter {
            next: self.head,
            offset,
            phantom: PhantomData,
        }
    }

    pub unsafe fn iter_mut(&mut self, offset: usize) -> IterMut<'_, T> {
        IterMut {
            next: self.head,
            offset,
            phantom: PhantomData,
        }
    }

    pub unsafe fn cursor_front_mut(&mut self, offset: usize) -> CursorMut<'_, T> {
        CursorMut {
            current: self.head,
            offset,
            list: self,
        }
    }
}

impl<'a, T: 'a> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        self.next.map(|n| {
            self.next = unsafe { n.as_ref() }.next;
            unsafe { &*((n.as_ptr() as usize - self.offset) as *const T) }
        })
    }
}

impl<'a, T: 'a> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<Self::Item> {
        self.next.map(|n| {
            self.next = unsafe { n.as_ref() }.next;
            unsafe { &mut *((n.as_ptr() as usize - self.offset) as *mut T) }
        })
    }
}

impl<'a, T: 'a> CursorMut<'a, T> {
    pub const fn is_valid(&self) -> bool {
        self.current.is_some()
    }

    pub unsafe fn move_next(&mut self) {
        match self.current.take() {
            Some(current) => {
                self.current = unsafe { current.as_ref().next };
            }
            None => {
                self.current = self.list.head;
            }
        }
    }

    pub unsafe fn move_prev(&mut self) {
        match self.current.take() {
            Some(current) => {
                self.current = unsafe { current.as_ref().prev };
            }
            None => {
                self.current = self.list.tail;
            }
        }
    }

    pub fn current(&mut self) -> Option<*mut T> {
        self.current
            .map(|n| (n.as_ptr() as usize - self.offset) as *mut T)
    }

    pub fn as_list(&'a self) -> &'a PtrLinkedList<T> {
        self.list
    }

    pub unsafe fn insert_after(&mut self, next: &mut PtrLinkedListNode<T>) {
        match self.current {
            Some(mut current) => {
                unsafe { self.list.insert_after(current.as_mut(), next) };
            }
            None => {
                unsafe { self.list.insert_head(next) };
            }
        }
    }

    pub unsafe fn insert_before(&mut self, prev: &mut PtrLinkedListNode<T>) {
        match self.current {
            Some(mut current) => {
                unsafe { self.list.insert_before(current.as_mut(), prev) };
            }
            None => {
                unsafe { self.list.insert_tail(prev) };
            }
        }
    }

    pub unsafe fn remove_current(&mut self) {
        if let Some(mut current) = self.current {
            self.current = unsafe { current.as_ref().next };
            unsafe { self.list.remove(current.as_mut()) };
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

    unsafe fn insert_after(&mut self, next: &mut Self) {
        let old_next = self.next;
        self.next = NonNull::new(next);
        next.prev = NonNull::new(self);
        next.next = old_next;
        if let Some(mut e) = next.next {
            unsafe { e.as_mut() }.prev = NonNull::new(next);
        }
    }

    unsafe fn insert_before(&mut self, prev: &mut Self) {
        let old_prev = self.prev;
        self.prev = NonNull::new(prev);
        prev.next = NonNull::new(self);
        prev.prev = old_prev;
        if let Some(mut e) = prev.prev {
            unsafe { e.as_mut() }.next = NonNull::new(prev);
        }
    }

    const fn unset_prev_and_next(&mut self) {
        self.prev = None;
        self.next = None;
    }

    pub const fn has_next(&self) -> bool {
        self.next.is_some()
    }

    pub const fn has_prev(&self) -> bool {
        self.prev.is_some()
    }

    pub fn get_next(&self, offset: usize) -> Option<*const T> {
        if let Some(e) = self.next {
            Some((e.as_ptr() as usize - offset) as *const T)
        } else {
            None
        }
    }

    pub fn get_next_mut(&mut self, offset: usize) -> Option<*mut T> {
        if let Some(e) = self.next {
            Some((e.as_ptr() as usize - offset) as *mut T)
        } else {
            None
        }
    }

    pub fn get_prev(&self, offset: usize) -> Option<*const T> {
        if let Some(e) = self.prev {
            Some((e.as_ptr() as usize - offset) as *const T)
        } else {
            None
        }
    }

    pub fn get_prev_mut(&mut self, offset: usize) -> Option<*mut T> {
        if let Some(e) = self.prev {
            Some((e.as_ptr() as usize - offset) as *mut T)
        } else {
            None
        }
    }
}
