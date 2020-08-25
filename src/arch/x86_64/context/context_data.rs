//!
//! Context Entry
//! This entry contains arch-depending data
//!

#[repr(C, align(64))]
pub struct ContextData {
    fx_save: [u8; 512],
    registers: Registers,
}

#[repr(C, packed)]
#[derive(Default)]
struct Registers {
    rax: usize,
    rdx: usize,
    rcx: usize,
    rbx: usize,
    rbp: usize,
    rsi: usize,
    rdi: usize,
    r8: usize,
    r9: usize,
    r10: usize,
    r11: usize,
    r12: usize,
    r13: usize,
    r14: usize,
    r15: usize,
    fs: usize,
    gs: usize,
    ss: usize,
    rsp: usize,
    rflags: usize,
    cs: usize,
    rip: usize,
    cr3: usize,
}

impl ContextData {
    /// This const val is used to const assert.
    /// Size is zero and read in Self::new()
    const STATIC_ASSERT_OF_REGISTERS: () = Self::check_struct_size();

    /// const assert(static_assert)
    /// This function will eval while compiling.
    /// Check if the size of Registers was changed.
    /// if you changed, you must review assembly code like context_switch and fix this function.
    const fn check_struct_size() {
        use core::mem;
        if mem::size_of::<Registers>() != 23 * mem::size_of::<usize>() {
            panic!("GeneralRegisters was changed.\nYou must check task_switch function.");
        }
    }

    /// Create ContextData by setting all registers to zero.
    pub fn new() -> Self {
        let _assert_check = Self::STATIC_ASSERT_OF_REGISTERS; /* to evaluate const assert */
        Self {
            registers: Registers::default(),
            fx_save: [0; 512],
        }
    }

    /// Create ContextData for system.
    /// ContextData's rflags is set as 0x202(allow interrupt).
    pub fn create_context_data_for_system(
        entry_address: usize,
        stack: usize,
        cs: usize,
        ss: usize,
        cr3: usize,
    ) -> Self {
        let mut data = Self::new();
        data.registers.rip = entry_address;
        data.registers.cs = cs;
        data.registers.ss = ss;
        data.registers.rflags = 0x202;
        data.registers.rsp = stack;
        data.registers.cr3 = cr3;
        data
    }
}
