//!
//! Context Entry
//!
//! This entry contains arch-depending data.
//!

#[repr(C, align(64))]
pub struct ContextData {
    fx_save: [u8; 512],
    registers: Registers,
}

#[repr(C, packed)]
#[derive(Default)]
struct Registers {
    rax: u64,
    rdx: u64,
    rcx: u64,
    rbx: u64,
    rbp: u64,
    rsi: u64,
    rdi: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    ds: u64,
    fs: u64,
    gs: u64,
    es: u64,
    ss: u64,
    rsp: u64,
    rflags: u64,
    cs: u64,
    rip: u64,
    cr3: u64,
    padding: u64,
}

impl ContextData {
    /// This const val is used to const assert.
    /// Size is zero and read in Self::new()
    const STATIC_ASSERT_OF_REGISTERS: () = Self::check_struct_size();

    /// Operate const assert(static_assert)
    ///
    /// This function will eval while compiling.
    /// Check if the size of Registers was changed.
    /// if you changed, you must review assembly code like context_switch and fix this function.
    const fn check_struct_size() {
        if core::mem::size_of::<Registers>() != 26 * core::mem::size_of::<u64>() {
            panic!("GeneralRegisters was changed.\nYou must check task_switch function and interrupt handler.");
        } else if (core::mem::size_of::<Registers>() / core::mem::size_of::<u64>()) & 1 != 0 {
            panic!("GeneralRegisters is not 16byte aligned.");
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
    ///
    /// ContextData's rflags is set as 0x202(allow interrupt).
    pub fn create_context_data_for_system(
        entry_address: usize,
        stack: usize,
        cs: u64,
        ss: u64,
        cr3: usize,
    ) -> Self {
        let mut data = Self::new();
        data.registers.rip = entry_address as u64;
        data.registers.cs = cs;
        data.registers.ss = ss;
        data.registers.rflags = 0x202;
        data.registers.rsp = stack as u64;
        data.registers.cr3 = cr3 as u64;
        data
    }

    /// Get Paging table address(address of PML4)
    ///
    /// This function returns pml4's address.
    pub fn get_paging_table_address(&self) -> usize {
        self.registers.cr3 as usize
    }
}
