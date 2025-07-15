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
use crate::kernel::memory_manager::MemoryError;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

/// This manager contains system/user stack/code segment pointers.
pub struct ContextManager {}

impl ContextManager {
    pub const DEFAULT_STACK_SIZE_OF_SYSTEM: usize = 0x200000;
    pub const IDLE_THREAD_STACK_SIZE: MSize = PAGE_SIZE;
    pub const DEFAULT_STACK_SIZE_OF_USER: usize = 0x8000;
    pub const DEFAULT_INTERRUPT_STACK_SIZE: MSize = MSize::new(0x2000);
    pub const STACK_ALIGN_ORDER: usize = 6; /* size = 2^6 = 64 */

    /// Create Context Manager with invalid data.
    pub const fn new() -> Self {
        Self {}
    }

    /// Init Context Manager with system code/stack segment and user code/stack segment.
    pub fn init(&mut self) {}

    /// Create system [`ContextData`]
    ///
    /// This function makes [`ContextData`] with the system code/stack segment.
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
            (stack_address + stack_size).to_usize(),
        ))
    }

    /// Create system [`ContextData`] from 'original_context_data'
    ///
    /// This function makes [`ContextData`] with the system code/stack segment.
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
            (stack_address + stack_size).to_usize(),
        ))
    }

    /// Create user [`ContextData`]
    ///
    /// This function makes [`ContextData`] with the user code/stack segment.
    ///
    /// `entry_address` must not return.
    pub fn create_user_context(
        &self,
        entry_address: usize,
        stack_address: VAddress,
        arguments: &[usize],
    ) -> Result<ContextData, MemoryError> {
        Ok(ContextData::create_context_data_for_user(
            entry_address,
            stack_address.to_usize(),
            arguments,
        ))
    }

    /// Jump to specific context data.
    ///
    /// This function **does not** save current process data.
    /// This is used when OS starts the task management system.
    ///
    /// **`context` must be aligned by 64bit**.
    pub unsafe fn jump_to_context(
        &self,
        context: &mut ContextData,
        allow_interrupt_after_jump: bool,
    ) {
        assert_eq!(core::mem::align_of_val(context), 64);
        if allow_interrupt_after_jump {
            context.registers.spsr &= !(cpu::SPSR_I | cpu::SPSR_F);
        }
        unsafe { cpu::run_task(context as *const _) }
    }

    /// Jump to next_context with saving current context into old_context.
    ///
    /// This function does not return until another context jumps to this context.
    /// Each [`ContextData`] must be aligned by 64bit (otherwise this function will panic).
    pub unsafe fn switch_context(
        &self,
        old_context: &mut ContextData,
        next_context: &mut ContextData,
        allow_interrupt_after_switch: bool,
    ) {
        assert_eq!(core::mem::align_of_val(old_context), 64);
        assert_eq!(core::mem::align_of_val(next_context), 64);
        if allow_interrupt_after_switch {
            next_context.registers.spsr &= !(cpu::SPSR_I | cpu::SPSR_F);
        }
        old_context.registers.spsr = cpu::get_daif() | cpu::get_kernel_spsr_m();
        unsafe { cpu::task_switch(next_context as *mut _, old_context as *mut _) }
    }
}
