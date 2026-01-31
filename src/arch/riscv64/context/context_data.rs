//!
//! Context Entry
//!
//! This entry contains arch-depending data.
//!

use crate::arch::target_arch::device::cpu::*;

#[repr(C, align(64))]
#[derive(Clone)]
pub struct ContextData {
    pub registers: Registers,
}

#[repr(C)]
#[derive(Default, Clone)]
pub struct Registers {
    /* x0 is the zero register */
    pub x1: u64,       /* +  0 */
    pub x2: u64,       /* +  1 (stack pointer) */
    pub x3: u64,       /* +  2 */
    pub x4: u64,       /* +  3 */
    pub x5: u64,       /* +  4 */
    pub x6: u64,       /* +  5 */
    pub x7: u64,       /* +  6 */
    pub x8: u64,       /* +  7 */
    pub x9: u64,       /* +  8 */
    pub x10: u64,      /* +  9 */
    pub x11: u64,      /* + 10 */
    pub x12: u64,      /* + 11 */
    pub x13: u64,      /* + 12 */
    pub x14: u64,      /* + 13 */
    pub x15: u64,      /* + 14 */
    pub x16: u64,      /* + 15 */
    pub x17: u64,      /* + 16 */
    pub x18: u64,      /* + 17 */
    pub x19: u64,      /* + 18 */
    pub x20: u64,      /* + 19 */
    pub x21: u64,      /* + 20 */
    pub x22: u64,      /* + 21 */
    pub x23: u64,      /* + 22 */
    pub x24: u64,      /* + 23 */
    pub x25: u64,      /* + 24 */
    pub x26: u64,      /* + 25 */
    pub x27: u64,      /* + 26 */
    pub x28: u64,      /* + 27 */
    pub x29: u64,      /* + 28 */
    pub x30: u64,      /* + 29 */
    pub x31: u64,      /* + 30 */
    pub sstatus: u64,  /* + 31 */
    pub sepc: u64,     /* + 32 */
    pub sscratch: u64, /* + 33 */
}

impl ContextData {
    /// This const value is the number of Registers' members.
    /// This is also used to const assert.
    pub const NUM_OF_REGISTERS: usize = Self::check_registers_size();

    /// Operate const assert(static_assert)
    ///
    /// This function will eval while compiling.
    /// Check if the size of Registers was changed.
    /// if you changed, you must review assembly code like context_switch and fix this function.
    const fn check_registers_size() -> usize {
        if size_of::<Registers>() != 34 * size_of::<u64>() {
            panic!(
                "GeneralRegisters was changed.\nYou must check task_switch function and interrupt handler."
            );
        } else if (size_of::<Registers>() / size_of::<u64>()) & 1 != 0 {
            panic!("GeneralRegisters is not 16byte aligned.");
        }
        size_of::<Registers>() / size_of::<u64>()
    }

    /// Create ContextData by setting all registers to zero.
    pub fn new() -> Self {
        Self {
            registers: Registers::default(),
        }
    }

    /// Create ContextData for system.
    ///
    pub fn create_context_data_for_system(entry_address: usize, stack: usize) -> Self {
        let mut data = Self::new();
        data.registers.sstatus = SSTATUS_SPP | SSTATUS_SIE;
        data.registers.sepc = entry_address as u64;
        data.registers.x2 = stack as u64;
        data
    }

    /// Create ContextData for user.
    pub fn create_context_data_for_user(
        entry_address: usize,
        stack: usize,
        arguments: &[usize],
    ) -> Self {
        let mut data = Self::new();
        data.registers.sstatus = SSTATUS_SIE;
        data.registers.sepc = entry_address as u64;
        data.registers.x2 = stack as u64;
        data.set_function_call_arguments(arguments);
        data
    }

    /// Create ContextData for system from 'original_context'.
    pub fn fork_context_data(original_context: &Self, entry_address: usize, stack: usize) -> Self {
        let mut forked_data = Self::new();
        forked_data.registers.x2 = stack as u64;
        forked_data.registers.sstatus = original_context.registers.sstatus;
        forked_data.registers.sepc = entry_address as u64;
        forked_data
    }

    pub fn set_function_call_arguments<T: TryInto<u64> + Clone>(&mut self, arguments: &[T]) {
        /* x[10..17] are a[0..7] */
        if !arguments.is_empty() {
            self.registers.x10 = arguments[0]
                .clone()
                .try_into()
                .map_err(|_| pr_warn!("Failed to set argument."))
                .unwrap_or(0);
        }
        if arguments.len() > 1 {
            self.registers.x11 = arguments[1]
                .clone()
                .try_into()
                .map_err(|_| pr_warn!("Failed to set argument."))
                .unwrap_or(0);
        }
        if arguments.len() > 2 {
            self.registers.x12 = arguments[2]
                .clone()
                .try_into()
                .map_err(|_| pr_warn!("Failed to set argument."))
                .unwrap_or(0);
        }
        if arguments.len() > 3 {
            self.registers.x13 = arguments[3]
                .clone()
                .try_into()
                .map_err(|_| pr_warn!("Failed to set argument."))
                .unwrap_or(0);
        }
        if arguments.len() > 4 {
            self.registers.x14 = arguments[4]
                .clone()
                .try_into()
                .map_err(|_| pr_warn!("Failed to set argument."))
                .unwrap_or(0);
        }
        if arguments.len() > 5 {
            self.registers.x15 = arguments[5]
                .clone()
                .try_into()
                .map_err(|_| pr_warn!("Failed to set argument."))
                .unwrap_or(0);
        }
        if arguments.len() > 6 {
            self.registers.x16 = arguments[6]
                .clone()
                .try_into()
                .map_err(|_| pr_warn!("Failed to set argument."))
                .unwrap_or(0);
        }
        if arguments.len() > 7 {
            self.registers.x17 = arguments[7]
                .clone()
                .try_into()
                .map_err(|_| pr_warn!("Failed to set argument."))
                .unwrap_or(0);
        }
        if arguments.len() > 8 {
            pr_err!("Too many arguments.");
        }
    }

    pub fn get_system_call_arguments(&self, index: usize) -> Option<u64> {
        if index > 8 {
            None
        } else {
            Some(unsafe { (*(self.registers.x10 as *const u64 as *const [u64; 8]))[index] })
        }
    }

    pub fn set_system_call_return_value(&mut self, v: u64) {
        self.registers.x10 = v;
    }
}
