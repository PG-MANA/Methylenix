//!
//! AML Notify function list
//!

use super::{AmlVariable, NameString};

use crate::kernel::sync::spin_lock::Mutex;

use alloc::vec::Vec;

pub struct NotifyList {
    list: Mutex<Vec<(NameString, fn(AmlVariable))>>,
}

impl NotifyList {
    pub fn new() -> Self {
        Self {
            list: Mutex::new(Vec::new()),
        }
    }

    pub fn register_function(&self, notify_name: &NameString, hook: fn(AmlVariable)) {
        let list = &mut self.list.lock().unwrap();
        if let Some(e) = list.iter_mut().find(|e| &e.0 == notify_name) {
            pr_debug!("{} is already exists, will be overwritten.", e.0);
            e.1 = hook;
        } else {
            list.push((notify_name.clone(), hook))
        }
    }

    pub fn notify(&self, notify_name: &NameString, value: AmlVariable) -> bool {
        let list = self.list.lock().unwrap();
        if let Some(e) = list.iter().find(|e| &e.0 == notify_name) {
            let func = e.1;
            drop(list);
            func(value);
            true
        } else {
            false
        }
    }
}
