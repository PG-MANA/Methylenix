//!
//! Context Entry
//!
//! This entry contains arch-depending data.
//!

use crate::arch::target_arch::cpu::{SPSR_M_EL0T, SPSR_M_EL1H};

#[repr(C, align(64))]
#[derive(Clone)]
pub struct ContextData {
    pub registers: Registers,
}

#[repr(C)]
#[derive(Default, Clone)]
pub struct Registers {
    pub x0: u64,    /* +  0 */
    pub x1: u64,    /* +  1 */
    pub x2: u64,    /* +  2 */
    pub x3: u64,    /* +  3 */
    pub x4: u64,    /* +  4 */
    pub x5: u64,    /* +  5 */
    pub x6: u64,    /* +  6 */
    pub x7: u64,    /* +  7 */
    pub x8: u64,    /* +  8 */
    pub x9: u64,    /* +  9 */
    pub x10: u64,   /* + 10 */
    pub x11: u64,   /* + 11 */
    pub x12: u64,   /* + 12 */
    pub x13: u64,   /* + 13 */
    pub x14: u64,   /* + 14 */
    pub x15: u64,   /* + 15 */
    pub x16: u64,   /* + 16 */
    pub x17: u64,   /* + 17 */
    pub x18: u64,   /* + 18 */
    pub x19: u64,   /* + 19 */
    pub x20: u64,   /* + 20 */
    pub x21: u64,   /* + 21 */
    pub x22: u64,   /* + 22 */
    pub x23: u64,   /* + 23 */
    pub x24: u64,   /* + 24 */
    pub x25: u64,   /* + 25 */
    pub x26: u64,   /* + 26 */
    pub x27: u64,   /* + 27 */
    pub x28: u64,   /* + 28 */
    pub x29: u64,   /* + 29 */
    pub x30: u64,   /* + 30 */
    padding: u64,   /* + 31 */
    pub sp: u64,    /* + 32 */
    pub tpidr: u64, /* + 33 */
    pub elr: u64,   /* + 34 */
    pub spsr: u64,  /* + 35 */
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
        if core::mem::size_of::<Registers>() != 36 * core::mem::size_of::<u64>() {
            panic!("GeneralRegisters was changed.\nYou must check task_switch function and interrupt handler.");
        } else if (core::mem::size_of::<Registers>() / core::mem::size_of::<u64>()) & 1 != 0 {
            panic!("GeneralRegisters is not 16byte aligned.");
        }
        core::mem::size_of::<Registers>() / core::mem::size_of::<u64>()
    }

    /// Create ContextData by setting all registers to zero.
    pub fn new() -> Self {
        Self {
            registers: Registers::default(),
        }
    }

    /// Create ContextData for system.
    ///
    /// ContextData's rflags is set as 0x202(allow interrupt).
    pub fn create_context_data_for_system(entry_address: usize, stack: usize) -> Self {
        let mut data = Self::new();
        data.registers.elr = entry_address as u64;
        data.registers.sp = stack as u64;
        data.registers.spsr = SPSR_M_EL1H;
        data
    }

    /// Create ContextData for user.
    ///
    /// ContextData's rflags is set as 0x202(allow interrupt).
    pub fn create_context_data_for_user(
        entry_address: usize,
        stack: usize,
        arguments: &[usize],
    ) -> Self {
        let mut data = Self::new();
        data.registers.elr = entry_address as u64;
        data.registers.spsr = SPSR_M_EL0T;
        data.registers.sp = stack as u64;
        if arguments.len() > 0 {
            data.registers.x0 = arguments[0] as u64;
        }
        if arguments.len() > 1 {
            data.registers.x1 = arguments[1] as u64;
        }
        if arguments.len() > 2 {
            data.registers.x2 = arguments[2] as u64;
        }
        if arguments.len() > 3 {
            data.registers.x3 = arguments[3] as u64;
        }
        if arguments.len() > 4 {
            data.registers.x4 = arguments[4] as u64;
        }
        if arguments.len() > 5 {
            data.registers.x5 = arguments[5] as u64;
        }
        if arguments.len() > 6 {
            data.registers.x6 = arguments[6] as u64;
        }
        if arguments.len() > 7 {
            data.registers.x7 = arguments[7] as u64;
        }
        if arguments.len() > 8 {
            pr_err!("Too many arguments.");
        }
        return data;
    }
    /// Create ContextData for system from 'original_context'.
    ///
    /// ContextData's rflags is set as 0x202(allow interrupt).
    pub fn fork_context_data(original_context: &Self, entry_address: usize, stack: usize) -> Self {
        let mut forked_data = Self::new();
        forked_data.registers.sp = stack as u64;
        forked_data.registers.elr = entry_address as u64;
        forked_data.registers.spsr = original_context.registers.spsr;
        return forked_data;
    }

    pub fn set_function_call_arguments(&mut self, arguments: &[u64]) {
        if arguments.len() > 0 {
            self.registers.x0 = arguments[0] as u64;
        }
        if arguments.len() > 1 {
            self.registers.x1 = arguments[1] as u64;
        }
        if arguments.len() > 2 {
            self.registers.x2 = arguments[2] as u64;
        }
        if arguments.len() > 3 {
            self.registers.x3 = arguments[3] as u64;
        }
        if arguments.len() > 4 {
            self.registers.x4 = arguments[4] as u64;
        }
        if arguments.len() > 5 {
            self.registers.x5 = arguments[5] as u64;
        }
        if arguments.len() > 6 {
            self.registers.x6 = arguments[6] as u64;
        }
        if arguments.len() > 7 {
            self.registers.x7 = arguments[7] as u64;
        }
        if arguments.len() > 8 {
            pr_err!("Too many arguments.");
        }
    }

    pub fn get_system_call_arguments(&self, index: usize) -> Option<u64> {
        if index > 8 {
            None
        } else {
            Some(unsafe { (*(self.registers.x0 as *const u64 as *const [u64; 8]))[index] })
        }
    }

    pub fn set_system_call_return_value(&mut self, v: u64) {
        self.registers.x0 = v;
    }
}
