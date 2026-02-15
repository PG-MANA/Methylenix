//
// RISC-V Specific Instruction
//
// This module is the collection of inline assembly functions.
// All functions are unsafe, please be careful.
//
// This comment is not the doc comment because this file is included by the loader.
//

use crate::arch::target_arch::context::context_data::ContextData;

use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

use core::arch::{asm, naked_asm};

const MIE_SEIE: u64 = 1 << 9;
const MIE_STIE: u64 = 1 << 5;
const MIE_SSIE: u64 = 1 << 1;
const MIE_SMASK: u64 = MIE_SEIE | MIE_STIE | MIE_SSIE;

const SENVCFG_CBCFE: u64 = 1 << 6;

pub const SBI_EID_SEND_IPI: u64 = 0x735049;
pub const SBI_FID_SEND_IPI: u64 = 0;
pub const SBI_EID_HART_START: u64 = 0x48534D;
pub const SBI_FID_HART_START: u64 = 0;

pub const SCAUSE_INTERRUPT: u64 = 1 << 63;
pub const SCAUSE_SUPERVISOR_SOFTWARE_INTERRUPT: u64 = 1;
pub const SCAUSE_SUPERVISOR_TIMER_INTERRUPT: u64 = 5;
pub const SCAUSE_SUPERVISOR_EXTERNAL_INTERRUPT: u64 = 9;
pub const SCAUSE_ENVIRONMENT_CALL_U_MODE: u64 = 8;

pub const SSTATUS_SPP: u64 = 1 << 8;
pub const SSTATUS_SPIE: u64 = 1 << 5;
pub const SSTATUS_SIE: u64 = 1 << 1;

#[inline(always)]
pub fn get_instruction_pointer() -> usize {
    let result: u64;
    unsafe { asm!("auipc {}, 0", out(reg) result) };
    result as usize
}

#[inline(always)]
pub fn get_stack_pointer() -> usize {
    let result: u64;
    unsafe { asm!("mv {}, sp", out(reg) result) };
    result as usize
}

#[inline(always)]
pub unsafe fn enable_interrupt() {
    unsafe { asm!("csrrs x0, sie, {}", in(reg) MIE_SMASK) };
}

#[inline(always)]
pub unsafe fn disable_interrupt() {
    unsafe { asm!("csrrc x0, sie, {}", in(reg) MIE_SMASK) };
}

#[inline(always)]
pub unsafe fn save_sie_and_disable_interrupt() -> u64 {
    let sie: u64;
    unsafe { asm!("csrrc {r}, sie, {m}",  r = out(reg) sie, m = in(reg) MIE_SMASK) };
    sie
}

#[inline(always)]
pub fn get_sie() -> u64 {
    let result: u64;
    unsafe { asm!("csrr {}, sie", out(reg) result) };
    result
}

#[inline(always)]
pub unsafe fn restore_sie(sie: u64) {
    unsafe { asm!("csrw sie, {}", in(reg) sie) };
}

#[inline(always)]
pub unsafe fn halt() {
    unsafe { asm!("wfi") };
}

#[inline(always)]
pub unsafe fn idle() {
    unsafe { asm!("csrrs x0, sie, {}\nwfi", in(reg) MIE_SMASK) };
}

#[inline(always)]
pub fn is_interrupt_enabled() -> bool {
    (get_sie() & MIE_SMASK) == MIE_SMASK
}

#[inline(always)]
pub fn get_cpu_base_address() -> usize {
    let gp: usize;
    unsafe { asm!("mv {}, gp", out(reg) gp) };
    gp
}

#[inline(always)]
pub unsafe fn set_cpu_base_address(address: u64) {
    unsafe { asm!("mv gp, {}", in(reg) address) };
}

#[inline(always)]
pub fn get_satp() -> u64 {
    let result: u64;
    unsafe { asm!("csrr {}, satp", out(reg) result) };
    result
}

#[inline(always)]
pub unsafe fn set_satp(satp: u64) {
    unsafe { asm!("csrw satp, {}", in(reg) satp) };
}

#[inline(always)]
pub fn get_sstatus() -> u64 {
    let result: u64;
    unsafe { asm!("csrr {}, sstatus", out(reg) result) };
    result
}

#[inline(always)]
pub fn get_stvec() -> u64 {
    let result: u64;
    unsafe { asm!("csrr {}, stvec", out(reg) result) };
    result
}

#[inline(always)]
pub fn get_scause() -> u64 {
    let result: u64;
    unsafe { asm!("csrr {}, scause", out(reg) result) };
    result
}

#[inline(always)]
pub unsafe fn set_stvec(stvec: u64) {
    unsafe { asm!("csrw stvec, {}", in(reg) stvec) };
}

#[inline(always)]
pub fn data_barrier() {
    unsafe { asm!("fence iorw, iorw") };
}

#[inline(always)]
pub fn memory_barrier() {
    unsafe { asm!("fence rw, rw") };
}

#[inline(always)]
pub fn instruction_barrier() {
    unsafe { asm!("fence.i") };
}

#[inline(always)]
pub fn get_senvcfg() -> u64 {
    let result: u64;
    unsafe { asm!("csrr {}, senvcfg", out(reg) result) };
    result
}

pub fn flush_data_cache(virtual_address: VAddress, size: MSize) {
    memory_barrier();
    if (get_senvcfg() & SENVCFG_CBCFE) != 0 {
        let block_size = 4; /* TODO: detect block size dynamically */

        let end = (((virtual_address + size).to_usize()) & !(block_size - 1)) + block_size;
        let mut a = virtual_address.to_usize() & !(block_size - 1);
        while a < end {
            unsafe { asm!(".attribute arch,\"rv64izicbom\"\ncbo.flush 0({})", in(reg) a) }
            a += block_size;
        }
        instruction_barrier();
    }
}

pub fn flush_data_cache_all() {
    memory_barrier();
    // Currently, there is no instruction to clean all data cache
    instruction_barrier();
}

#[inline(always)]
pub fn flush_all_cache() {
    instruction_barrier();
    // Currently, there is no instruction to clean instruction caches
    flush_data_cache_all();
}

#[inline(always)]
pub fn synchronize<T>(target_virtual_address: *const T) {
    flush_data_cache(
        VAddress::new(target_virtual_address as usize),
        MSize::new(size_of::<T>()),
    );
}

#[inline(always)]
pub fn get_hartid() -> u64 {
    let result: u64;
    unsafe { asm!("csrr {}, mhartid", out(reg) result) };
    result
}

/// Execute SMC #0 with Secure Monitor Call Conversation
#[inline(always)]
pub unsafe fn sbi_call(
    mut arg0: u64,
    mut arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    function_id: u64,
    extension_id: u64,
) -> (u64, u64) {
    unsafe {
        asm!(
            "ecall",
            inout("a0") arg0,
            inout("a1") arg1,
            in("a2") arg2,
            in("a3") arg3,
            in("a4") arg4,
            in("a5") arg5,
            in("a6") function_id,
            in("a7") extension_id,
            clobber_abi("C")
        )
    }
    (arg0, arg1)
}

#[unsafe(naked)]
pub unsafe extern "C" fn run_task(_context_data_address: *const ContextData) {
    naked_asm!(
        "
    // a0(x10) contains `_context_data_address`
    // Set interrupt status
    ld      t0, (8 * 31)(a0)
    ld      t1, (8 * 32)(a0)
    csrw    sstatus, t0
    csrw    sepc, t1
    // Set the a0 to sscratch
    ld      t0, (8 * 9)(a0)
    csrw    sscratch, t0
    // Set general registers
    ld      x1, (8 * 0)(a0)
    ld      x2, (8 * 1)(a0)
    ld      x3, (8 * 2)(a0)
    ld      x4, (8 * 3)(a0)
    ld      x5, (8 * 4)(a0)
    ld      x6, (8 * 5)(a0)
    ld      x7, (8 * 6)(a0)
    ld      x8, (8 * 7)(a0)
    ld      x9, (8 * 8)(a0)
    // x10 is a0 register, set later
    ld      x10, (8 * 9)(a0)
    ld      x11, (8 * 10)(a0)
    ld      x12, (8 * 11)(a0)
    ld      x13, (8 * 12)(a0)
    ld      x14, (8 * 13)(a0)
    ld      x15, (8 * 14)(a0)
    ld      x16, (8 * 15)(a0)
    ld      x17, (8 * 16)(a0)
    ld      x18, (8 * 17)(a0)
    ld      x19, (8 * 18)(a0)
    ld      x20, (8 * 19)(a0)
    ld      x21, (8 * 20)(a0)
    ld      x22, (8 * 21)(a0)
    ld      x23, (8 * 22)(a0)
    ld      x24, (8 * 23)(a0)
    ld      x25, (8 * 24)(a0)
    ld      x26, (8 * 25)(a0)
    ld      x27, (8 * 26)(a0)
    ld      x28, (8 * 27)(a0)
    ld      x29, (8 * 28)(a0)
    ld      x30, (8 * 29)(a0)
    ld      x31, (8 * 30)(a0)
    // Set sscratch to a0
    ld      a0, (8 * 33)(a0)
    // Swap a0 and sscratch, .
    csrrw   a0, sscratch, a0
    sret"
    )
}

/// Save the current process into now_context_data and run next_context_data.
///
/// This function is called by ContextManager.
/// This function does not return until another process switches to now_context_data.
/// This function assumes 1st argument is passed by "x0" and 2nd is passed by "x1".
/// now_context_data_address.registers.spsr_el1 must be set by caller.
#[inline(never)]
pub unsafe extern "C" fn task_switch(
    next_context_data_address: *const ContextData,
    now_context_data_address: *mut ContextData,
) {
    unsafe {
        asm!("
    csrr    t0, sstatus
    la      t1, 1f // sepc
    sd      t0, (8 * 31)(sp)
    sd      t1, (8 * 32)(sp)
    /* Save callee-saved register */
    sd      x2, (8 * 1)(a1)
    sd      x3, (8 * 2)(sp)
    sd      x4, (8 * 3)(sp)
    sd      x8, (8 * 7)(sp)
    sd      x9, (8 * 8)(sp)
    sd      x18, (8 * 17)(sp)
    sd      x19, (8 * 18)(sp)
    sd      x20, (8 * 19)(sp)
    sd      x21, (8 * 20)(sp)
    sd      x22, (8 * 21)(sp)
    sd      x23, (8 * 22)(sp)
    sd      x24, (8 * 23)(sp)
    sd      x25, (8 * 24)(sp)
    sd      x26, (8 * 25)(sp)
    sd      x27, (8 * 26)(sp)
    j       {}
    1:
    ",
        sym run_task,
        in("a0") next_context_data_address,
        in("a1") now_context_data_address
        )
    };
}
