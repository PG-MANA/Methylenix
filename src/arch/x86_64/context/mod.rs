/*
 * Context Manager
 * This manager is the backend of task management system.
 * This manager treats arch-specific processes.
 */

pub mod context_data;

use self::context_data::ContextData;
use arch::target_arch::device::cpu;

use core::mem;

pub struct ContextManager {
    system_ss: usize,
    system_cs: usize,
    user_ss: usize,
    user_cs: usize,
}

impl ContextManager {
    pub const DEFAULT_STACK_SIZE_OF_SYSTEM: usize = 0x1000;
    pub const DEFAULT_STACK_SIZE_OF_USER: usize = 0x400;
    pub const STACK_ALIGN_ORDER: usize = 6; /*size = 64*/

    pub const fn new() -> Self {
        Self {
            system_cs: 0,
            system_ss: 0,
            user_cs: 0,
            user_ss: 0,
        }
    }

    pub fn init(&mut self, system_cs: usize, system_ss: usize, user_cs: usize, user_ss: usize) {
        self.system_cs = system_cs;
        self.system_ss = system_ss;
        self.user_ss = user_ss;
        self.user_cs = user_cs;
    }

    pub fn create_system_context(
        &mut self,
        entry_address: usize,
        stack_address: usize,
    ) -> ContextData {
        ContextData::create_context_data_for_system(
            entry_address,
            stack_address,
            self.system_cs,
            self.system_ss,
        )
    }

    pub unsafe fn jump_to_context(&mut self, context: &mut ContextData) {
        /* for init process */
        assert_eq!(mem::align_of_val(context), 64);
        cpu::run_task(context as *mut _);
    }

    /* ContextSwitch */
    pub unsafe fn switch_context(
        &mut self,
        old_context: &mut ContextData,
        next_context: &mut ContextData,
    ) {
        assert_eq!(mem::align_of_val(old_context), 64);
        assert_eq!(mem::align_of_val(next_context), 64);
        cpu::task_switch(next_context as *mut _, old_context as *mut _);
    }
}
