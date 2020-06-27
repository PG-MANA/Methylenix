/*
 * Context Entry
 * This entry contains arch-depending data
 */

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
}

impl ContextData {
    pub fn new() -> Self {
        use core::mem;
        if mem::size_of::<Registers>() != 22 * mem::size_of::<usize>() {
            panic!("GeneralRegisters was changed.\nYou must check task_switch function.");
        }
        Self {
            registers: Registers::default(),
            fx_save: [0; 512],
        }
    }

    pub fn create_context_data_for_system(
        entry_address: usize,
        stack: usize,
        cs: usize,
        ss: usize,
    ) -> Self {
        let mut data = Self::new();
        data.registers.rip = entry_address;
        data.registers.cs = cs;
        data.registers.ss = ss;
        data.registers.rflags = 0x202;
        data.registers.rsp = stack;
        data
    }
}
