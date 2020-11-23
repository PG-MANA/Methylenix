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
    rax: u64,     /* +  0 */
    rdx: u64,     /* +  1 */
    rcx: u64,     /* +  2 */
    rbx: u64,     /* +  3 */
    rbp: u64,     /* +  4 */
    rsi: u64,     /* +  5 */
    rdi: u64,     /* +  6 */
    r8: u64,      /* +  7 */
    r9: u64,      /* +  8 */
    r10: u64,     /* +  9 */
    r11: u64,     /* + 10 */
    r12: u64,     /* + 11 */
    r13: u64,     /* + 12 */
    r14: u64,     /* + 13 */
    r15: u64,     /* + 14 */
    ds: u64,      /* + 15 */
    fs: u64,      /* + 16 */
    fs_base: u64, /* + 17 */
    gs: u64,      /* + 18 */
    gs_base: u64, /* + 19 */
    es: u64,      /* + 20 */
    ss: u64,      /* + 21 */
    rsp: u64,     /* + 22 */
    rflags: u64,  /* + 23 */
    cs: u64,      /* + 24 */
    rip: u64,     /* + 25 */
    cr3: u64,     /* + 26 */
    padding: u64,
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
