//!
//! Context Manager
//!
//! This manager is the backend of task management system.
//! This treats arch-specific processes.
//!

pub mod context_data;
pub mod memory_layout;

use self::context_data::ContextData;

use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SIZE};
use crate::kernel::manager_cluster::get_cpu_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};
use crate::kernel::memory_manager::MemoryError;

/// This manager contains system/user stack/code segment pointer.
pub struct ContextManager {
    system_ss: u16,
    system_cs: u16,
    #[allow(dead_code)]
    user_ss: u16,
    #[allow(dead_code)]
    user_cs: u16,
    system_page_table_address: usize,
}

impl ContextManager {
    pub const DEFAULT_STACK_SIZE_OF_SYSTEM: usize = 0x200000;
    pub const IDLE_THREAD_STACK_SIZE: MSize = PAGE_SIZE;
    pub const DEFAULT_STACK_SIZE_OF_USER: usize = 0x8000;
    pub const DEFAULT_INTERRUPT_STACK_SIZE: MSize = MSize::new(0x2000);
    pub const STACK_ALIGN_ORDER: usize = 6; /*size = 2^6 = 64*/

    /// Create Context Manager with invalid data.
    pub const fn new() -> Self {
        Self {
            system_cs: 0,
            system_ss: 0,
            user_cs: 0,
            user_ss: 0,
            system_page_table_address: 0,
        }
    }

    /// Init Context Manager with system code/stack segment and user code/stack segment.
    pub fn init(
        &mut self,
        system_cs: u16,
        system_ss: u16,
        user_cs: u16,
        user_ss: u16,
        system_page_table_address: usize,
    ) {
        const { memory_layout::check_memory_layout() };

        self.system_cs = system_cs;
        self.system_ss = system_ss;
        self.user_ss = user_ss;
        self.user_cs = user_cs;
        self.system_page_table_address = system_page_table_address;
    }

    /// Create system context data
    ///
    /// This function makes a context data with system code/stack segment.
    ///
    /// `entry_address` must not return.
    pub fn create_system_context(
        &self,
        entry_address: fn() -> !,
        stack_size: Option<MSize>,
    ) -> Result<ContextData, MemoryError> {
        let stack_size = stack_size.unwrap_or(MSize::new(Self::DEFAULT_STACK_SIZE_OF_SYSTEM));
        if (stack_size & !PAGE_MASK) != 0 {
            return Err(MemoryError::NotAligned);
        }

        let stack_address = get_cpu_manager_cluster()
            .memory_allocator
            .kmalloc(stack_size)?;

        Ok(ContextData::create_context_data_for_system(
            entry_address as *const fn() as usize,
            (stack_address + stack_size).to_usize() - 8, /* For SystemV ABI Stack Alignment */
            self.system_cs as u64,
            self.system_ss as u64,
            //self.system_page_table_address,
        ))
    }

    /// Create system context data from 'original_context_data'
    ///
    /// This function makes a context data with system code/stack segment.
    ///
    /// `entry_address` must not return.
    pub fn fork_system_context(
        &self,
        original_context_data: &ContextData,
        entry_address: fn() -> !,
        stack_size: Option<MSize>,
    ) -> Result<ContextData, MemoryError> {
        let stack_size = stack_size.unwrap_or(MSize::new(Self::DEFAULT_STACK_SIZE_OF_SYSTEM));
        if (stack_size & !PAGE_MASK) != 0 {
            return Err(MemoryError::NotAligned);
        }

        let stack_address = get_cpu_manager_cluster()
            .memory_allocator
            .kmalloc(stack_size)?;

        Ok(ContextData::fork_context_data(
            original_context_data,
            entry_address as *const fn() as usize,
            (stack_address + stack_size).to_usize() - 8, /* For SystemV ABI Stack Alignment */
        ))
    }

    /// Create user context data
    ///
    /// This function makes a context data with user code/stack segment.
    ///
    /// `entry_address` must not return.
    pub fn create_user_context(
        &self,
        entry_address: usize,
        stack_address: VAddress,
        arguments: &[usize],
        //pg_manager: &PageManager,
    ) -> Result<ContextData, MemoryError> {
        Ok(ContextData::create_context_data_for_user(
            entry_address,
            stack_address.to_usize(),
            self.user_cs as u64,
            self.user_ss as u64,
            arguments,
            //pg_manager.get_page_table_address().to_usize(),
        ))
    }

    /// Jump to specific context data.
    ///
    /// This function **does not** save current process data.
    /// This is used when OS starts task management system.
    ///
    /// **ContextData must be aligned by 64bit**.
    pub unsafe fn jump_to_context(
        &self,
        context: &mut ContextData,
        allow_interrupt_after_jump: bool,
    ) {
        assert_eq!(core::mem::align_of_val(context), 64);
        if allow_interrupt_after_jump {
            context.registers.rflags |= 0x0200;
        }
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
        allow_interrupt_after_switch: bool,
    ) {
        assert_eq!(core::mem::align_of_val(old_context), 64);
        assert_eq!(core::mem::align_of_val(next_context), 64);
        if allow_interrupt_after_switch {
            next_context.registers.rflags |= 0x0200;
        }
        cpu::task_switch(next_context as *mut _, old_context as *mut _);
    }
}
