//!
//! Context Manager
//!
//! This manager is the backend of task management system.
//! This treats arch-specific processes.
//!

pub mod context_data;

use self::context_data::ContextData;

use crate::arch::target_arch::device::cpu;
use crate::arch::x86_64::paging::PAGE_MASK;
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize};
use crate::kernel::memory_manager::MemoryError;

/// This manager contains system/user stack/code segment pointer.
pub struct ContextManager {
    system_ss: u16,
    system_cs: u16,
    user_ss: u16,
    user_cs: u16,
}

impl ContextManager {
    pub const DEFAULT_STACK_SIZE_OF_SYSTEM: usize = 0x8000;
    pub const DEFAULT_STACK_SIZE_OF_USER: usize = 0x1000;
    pub const STACK_ALIGN_ORDER: usize = 6; /*size = 2^6 = 64*/

    /// Create Context Manager with invalid data.
    ///
    /// This function is const fn.
    pub const fn new() -> Self {
        Self {
            system_cs: 0,
            system_ss: 0,
            user_cs: 0,
            user_ss: 0,
        }
    }

    /// Init Context Manager with system code/stack segment and user code/stack segment.
    pub fn init(&mut self, system_cs: u16, system_ss: u16, user_cs: u16, user_ss: u16) {
        self.system_cs = system_cs;
        self.system_ss = system_ss;
        self.user_ss = user_ss;
        self.user_cs = user_cs;
    }

    /// Create system context data
    ///
    /// This function makes a context data with system code/stack segment.
    pub fn create_system_context(
        &self,
        entry_address: usize,
        stack_size: Option<MSize>,
        pml4_address: usize,
    ) -> Result<ContextData, MemoryError> {
        let stack_size = stack_size.unwrap_or(MSize::new(Self::DEFAULT_STACK_SIZE_OF_SYSTEM));
        if (stack_size & !PAGE_MASK) != 0 {
            return Err(MemoryError::SizeNotAligned);
        }

        let stack_address = get_kernel_manager_cluster()
            .object_allocator
            .lock()
            .unwrap()
            .alloc(stack_size, &get_kernel_manager_cluster().memory_manager)?;

        Ok(ContextData::create_context_data_for_system(
            entry_address,
            (stack_address + stack_size).to_usize() - 8, /* For SystemV ABI Stack Alignment */
            self.system_cs as u64,
            self.system_ss as u64,
            pml4_address,
        ))
    }

    /// Create system context data
    ///
    /// This function makes a context data with kernel code/stack segment.
    /// Returned context shares paging table with kernel_context.
    pub fn create_kernel_context(
        &self,
        entry_address: usize,
        stack_size: Option<MSize>,
        kernel_context: &ContextData,
    ) -> Result<ContextData, MemoryError> {
        self.create_system_context(
            entry_address,
            stack_size,
            kernel_context.get_paging_table_address(),
        )
    }

    /// Jump to specific context data.
    ///
    /// This function **does not** save current process data.
    /// This is used when OS starts task management system.
    /// ContextData must be aligned by 64bit
    pub unsafe fn jump_to_context(&self, context: &mut ContextData) {
        assert_eq!(core::mem::align_of_val(context), 64);
        cpu::run_task(context as *mut _);
    }

    /// Jump to next_context with saving current context into old_context.
    ///
    /// This function does not return until another context jumps to this context.
    /// each context must be aligned by 64bit (otherwise this function will panic).  
    pub unsafe fn switch_context(
        &self,
        old_context: &mut ContextData,
        next_context: &mut ContextData,
    ) {
        assert_eq!(core::mem::align_of_val(old_context), 64);
        assert_eq!(core::mem::align_of_val(next_context), 64);
        cpu::task_switch(next_context as *mut _, old_context as *mut _);
    }
}
