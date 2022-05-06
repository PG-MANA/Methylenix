//!
//! Context Entry
//!
//! This entry contains arch-depending data.
//!

#[repr(C, align(64))]
#[derive(Clone)]
pub struct ContextData {
    fx_save: [u8; 512],
    pub registers: Registers,
}

#[repr(C)]
#[derive(Default, Clone)]
pub struct Registers {
    pub rax: u64,     /* +  0 */
    pub rdx: u64,     /* +  1 */
    pub rcx: u64,     /* +  2 */
    pub rbx: u64,     /* +  3 */
    pub rbp: u64,     /* +  4 */
    pub rsi: u64,     /* +  5 */
    pub rdi: u64,     /* +  6 */
    pub r8: u64,      /* +  7 */
    pub r9: u64,      /* +  8 */
    pub r10: u64,     /* +  9 */
    pub r11: u64,     /* + 10 */
    pub r12: u64,     /* + 11 */
    pub r13: u64,     /* + 12 */
    pub r14: u64,     /* + 13 */
    pub r15: u64,     /* + 14 */
    pub ds: u64,      /* + 15 */
    pub fs: u64,      /* + 16 */
    pub fs_base: u64, /* + 17 */
    pub gs: u64,      /* + 18 */
    pub gs_base: u64, /* + 19 */
    pub es: u64,      /* + 20 */
    pub ss: u64,      /* + 21 */
    pub rsp: u64,     /* + 22 */
    pub rflags: u64,  /* + 23 */
    pub cs: u64,      /* + 24 */
    pub rip: u64,     /* + 25 */
    pub cr3: u64,     /* + 26 */
    pub padding: u64,
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
        if core::mem::size_of::<Registers>() != 28 * core::mem::size_of::<u64>() {
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
            fx_save: [0; 512],
        }
    }

    /// Create ContextData for system.
    ///
    /// ContextData's rflags is set as 0x202(allow interrupt).
    pub fn create_context_data_for_system(
        entry_address: usize,
        stack: usize,
        cs: u64,
        ss: u64,
        //cr3: usize,
    ) -> Self {
        let mut data = Self::new();
        data.registers.rip = entry_address as u64;
        data.registers.cs = cs;
        data.registers.ss = ss;
        data.registers.rflags = 0x202;
        data.registers.rsp = stack as u64;
        //data.registers.cr3 = cr3 as u64;
        return data;
    }

    /// Create ContextData for user.
    ///
    /// ContextData's rflags is set as 0x202(allow interrupt).
    pub fn create_context_data_for_user(
        entry_address: usize,
        stack: usize,
        cs: u64,
        ss: u64,
        arguments: &[usize],
        //cr3: usize,
    ) -> Self {
        let mut data = Self::new();
        data.registers.rip = entry_address as u64;
        data.registers.cs = cs;
        data.registers.ss = ss;
        data.registers.rflags = 0x202;
        data.registers.rsp = stack as u64;
        if arguments.len() > 0 {
            data.registers.rdi = arguments[0] as u64;
        }
        if arguments.len() > 1 {
            data.registers.rsi = arguments[1] as u64;
        }
        if arguments.len() > 2 {
            data.registers.rdx = arguments[2] as u64;
        }
        if arguments.len() > 3 {
            data.registers.rcx = arguments[3] as u64;
        }
        if arguments.len() > 4 {
            data.registers.r8 = arguments[4] as u64;
        }
        if arguments.len() > 5 {
            data.registers.r9 = arguments[5] as u64;
        }
        if arguments.len() >= 6 {
            pr_err!("Too many arguments.");
        }
        //data.registers.cr3 = cr3 as u64;
        return data;
    }
    /// Create ContextData for system from 'original_context'.
    ///
    /// ContextData's rflags is set as 0x202(allow interrupt).
    pub fn fork_context_data(original_context: &Self, entry_address: usize, stack: usize) -> Self {
        let mut forked_data = Self::new();
        forked_data.registers.rip = entry_address as u64;
        forked_data.registers.cr3 = original_context.registers.cr3;
        forked_data.registers.cs = original_context.registers.cs;
        forked_data.registers.ss = original_context.registers.ss;
        forked_data.registers.rflags = 0x202;
        forked_data.registers.rsp = stack as u64;
        return forked_data;
    }

    pub fn get_system_call_arguments(&self, index: usize) -> Option<u64> {
        if index > 6 {
            None
        } else {
            Some(if index == 0 {
                self.registers.rax
            } else if index == 1 {
                self.registers.rdi
            } else if index == 2 {
                self.registers.rsi
            } else if index == 3 {
                self.registers.rdx
            } else if index == 4 {
                self.registers.r10
            } else if index == 5 {
                self.registers.r8
            } else {
                /* index == 6*/
                self.registers.r9
            })
        }
    }
}
