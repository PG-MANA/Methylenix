//!
//! AArch64 Specific Instruction
//!
//! This module is the collection of inline assembly functions.
//! All functions are unsafe, please be careful.  
//!

use crate::arch::target_arch::context::context_data::ContextData;

use core::arch::asm;

const DAIF_IRQ: u64 = 1 << 7;
const DAIF_FIQ: u64 = 1 << 6;

pub const SPSR_M_EL1H: u64 = 0b0101;
pub const SPSR_M_EL0T: u64 = 0b0000;
pub const SPSR_M: u64 = 0b1111;
pub const SPSR_I: u64 = 1 << 7;
pub const SPSR_F: u64 = 1 << 6;

const TCR_EL1_T0SZ_OFFSET: u64 = 0;
const TCR_EL1_T0SZ: u64 = 0b111111 << TCR_EL1_T0SZ_OFFSET;
const TCR_EL1_T1SZ_OFFSET: u64 = 16;
const TCR_EL1_T1SZ: u64 = 0b111111 << TCR_EL1_T1SZ_OFFSET;

#[no_mangle]
#[naked]
pub unsafe extern "C" fn _boot_main() {
    asm!("
        adr x8, {}
        add x8, x8, {}
        sub x8, x8, 8
        mov sp, x8
        b   boot_main
    ",
    sym crate::arch::target_arch::KERNEL_INITIAL_STACK,
    const crate::arch::target_arch::KERNEL_INITIAL_STACK_SIZE,
    options(noreturn));
}

#[inline(always)]
pub unsafe fn enable_interrupt() {
    asm!("      dsb ish
                mrs {t}, DAIF
                and {t}, {t}, {c}
                msr DAIF, {t} 
                ", t = out(reg) _, c = const !(DAIF_FIQ | DAIF_IRQ));
}

#[inline(always)]
pub unsafe fn disable_interrupt() {
    asm!("      dsb ish
                mrs {t}, DAIF
                orr {t}, {t}, {fiq}
                orr {t}, {t}, {irq}
                msr DAIF, {t} 
                ", t = out(reg) _, fiq = const DAIF_FIQ, irq = const DAIF_IRQ);
}

#[inline(always)]
pub unsafe fn save_daif_and_disable_irq_fiq() -> u64 {
    let daif: u64;
    asm!("
            mrs {t}, DAIF
            mov {r}, {t}
            orr {t}, {t}, {fiq}
            orr {t}, {t}, {irq}
            dsb ish
            msr DAIF, {t}",
    t = out(reg) _,
    r = out(reg) daif,
    fiq = const DAIF_FIQ,
    irq = const DAIF_IRQ);
    daif
}

#[inline(always)]
pub unsafe fn get_daif() -> u64 {
    let result: u64;
    asm!("mrs {:x}, daif", out(reg) result);
    result
}

#[inline(always)]
pub unsafe fn restore_irq_fiq(daif: u64) {
    asm!("  dsb ish
            msr DAIF, {:x}", in(reg) daif);
}

#[inline(always)]
pub unsafe fn halt() {
    asm!("wfi");
}

#[inline(always)]
pub unsafe fn idle() {
    asm!("      dsb ish
                mrs {t}, DAIF
                and {t}, {t}, {fiq_m}
                and {t}, {t}, {irq_m}
                msr DAIF, {t}
                wfi
                ", t = out(reg) _, fiq_m = const !DAIF_FIQ, irq_m = const !DAIF_IRQ);
}

#[inline(always)]
pub fn is_interrupt_enabled() -> bool {
    let daif: u64;
    unsafe { asm!("mrs {:x}, DAIF", out(reg) daif) };
    (daif & (DAIF_FIQ | DAIF_IRQ)) != 0
}

#[inline(always)]
pub unsafe fn get_cpu_base_address() -> usize {
    let result: usize;
    asm!("mrs {:x}, tpidr_el1", out(reg) result);
    result
}

#[inline(always)]
pub unsafe fn set_cpu_base_address(address: u64) {
    asm!("msr tpidr_el1, {:x}", in(reg) address);
}

#[inline(always)]
pub unsafe fn get_ttbr1() -> u64 {
    let result: u64;
    asm!("mrs {:x}, ttbr1_el1", out(reg) result);
    result
}

#[inline(always)]
pub unsafe fn set_ttbr0(ttbr1: u64) {
    asm!("msr ttbr0_el1, {:x}", in(reg) ttbr1);
}

#[inline(always)]
pub unsafe fn get_tcr() -> u64 {
    let result: u64;
    asm!("mrs {:x}, tcr_el1", out(reg) result);
    result
}

#[inline(always)]
pub unsafe fn get_t0sz() -> u64 {
    (get_tcr() & TCR_EL1_T0SZ) >> TCR_EL1_T0SZ_OFFSET
}

#[inline(always)]
pub unsafe fn get_t1sz() -> u64 {
    (get_tcr() & TCR_EL1_T1SZ) >> TCR_EL1_T1SZ_OFFSET
}

#[inline(always)]
pub unsafe fn set_mair(mair: u64) {
    asm!("msr mair_el1, {:x}", in(reg) mair);
}

#[inline(always)]
pub unsafe fn tlbi_va(target: u64) {
    asm!("tlbi vaae1,{:x}", in(reg) target);
}

#[inline(always)]
pub unsafe fn set_vbar(address: u64) {
    asm!("msr vbar_el1, {:x}", in(reg) address);
}

#[inline(always)]
pub unsafe fn get_icc_sre() -> u64 {
    let r: u64;
    asm!("mrs {:x}, icc_sre_el1", out(reg) r);
    r
}

#[inline(always)]
pub unsafe fn get_icc_hppir1() -> u64 {
    let r: u64;
    asm!("mrs {:x}, icc_hppir1_el1", out(reg) r);
    r
}

#[inline(always)]
pub unsafe fn get_icc_iar1() -> u64 {
    let r: u64;
    asm!("mrs {:x}, icc_iar1_el1", out(reg) r);
    r
}

#[inline(always)]
pub unsafe fn set_icc_sre(icc_sre: u64) {
    asm!("msr icc_sre_el1, {:x}", in(reg) icc_sre);
}

#[inline(always)]
pub unsafe fn set_icc_igrpen1(icc_igrpen1: u64) {
    asm!("msr icc_igrpen1_el1, {:x}", in(reg) icc_igrpen1);
}

#[inline(always)]
pub unsafe fn set_icc_eoir1(icc_eoir1: u64) {
    asm!("msr icc_eoir1_el1, {:x}", in(reg) icc_eoir1);
}

#[inline(always)]
pub unsafe fn set_icc_pmr(icc_pmr: u64) {
    asm!("msr icc_pmr_el1, {:x}", in(reg) icc_pmr);
}

#[inline(always)]
pub unsafe fn get_cntcr() -> u64 {
    let r: u64;
    asm!("
            isb
            mrs {:x}, cntcr_el1", out(reg) r);
    r
}

#[inline(always)]
pub unsafe fn get_cntpct() -> u64 {
    let r: u64;
    asm!("
            isb
            mrs {:x}, cntpct_el0", out(reg) r);
    r
}

#[inline(always)]
pub unsafe fn get_cntfrq() -> u64 {
    let r: u64;
    asm!("mrs {:x}, cntfrq_el0", out(reg) r);
    r
}

#[inline(always)]
pub unsafe fn set_cntp_ctl(cntp_ctl: u64) {
    asm!("msr cntp_ctl_el0, {:x}", in(reg) cntp_ctl)
}

#[inline(always)]
pub unsafe fn set_cntp_tval(cntp_tval: u64) {
    asm!("msr cntp_tval_el0, {:x}", in(reg) cntp_tval)
}

#[inline(always)]
pub unsafe fn get_mpidr() -> u64 {
    let r: u64;
    asm!("mrs {:x}, mpidr_el1", out(reg) r);
    r
}

pub const fn mpidr_to_affinity(mpidr: u64) -> u64 {
    mpidr & !((1 << 31) | (1 << 30))
}

#[naked]
#[allow(unused_variables)]
pub unsafe extern "C" fn run_task(context_data_address: *const ContextData) {
    asm!(
        "
            ldp  x1, x2, [x0, #(8 * 34)]
            msr  elr_el1, x1
            and  x2, x2, {m}
            cmp  x2, {el0}
            b.ne 1f
            ldp  x1, x2, [x0, #(8 * 32)]
            msr  sp_el0, x1
            msr  tpidr_el0, x2
            b    2f
        1:
            ldr  x1, [x0, #(8 * 32)]
            mov  sp, x1
        2:
            ldp  x2,  x3, [x0, #(16 * 1)]
            ldp  x4,  x5, [x0, #(16 * 2)]
            ldp  x6,  x7, [x0, #(16 * 3)]
            ldp  x8,  x9, [x0, #(16 * 4)]
            ldp x10, x11, [x0, #(16 * 5)]
            ldp x12, x13, [x0, #(16 * 6)]
            ldp x14, x15, [x0, #(16 * 7)]
            ldp x16, x17, [x0, #(16 * 8)]
            ldp x18, x19, [x0, #(16 * 9)]
            ldp x20, x21, [x0, #(16 * 10)]
            ldp x22, x23, [x0, #(16 * 11)]
            ldp x24, x25, [x0, #(16 * 12)]
            ldp x26, x27, [x0, #(16 * 13)]
            ldp x28, x29, [x0, #(16 * 14)]
            ldr x30,      [x0, #(16 * 15)]
            ldp  x0,  x1, [x0, #(16 * 0)]
            eret
    ",
        m = const SPSR_M,
        el0 = const SPSR_M_EL0T,
        options(noreturn)
    )
}

/// Save current process into now_context_data and run next_context_data.
///
/// This function is called by ContextManager.
/// This function does not return until another process switches to now_context_data.
/// This function assume 1st argument is passed by "x0" and 2nd is passed by "x1".
/// now_context_data_address.registers.spsr_el1 must be set by caller.
#[inline(never)]
pub unsafe extern "C" fn task_switch(
    next_context_data_address: *const ContextData,
    now_context_data_address: *mut ContextData,
) {
    asm!(
    "
            /* x2 ~ x17 are usable in this function(caller saved registers) */
            mrs  x3, tpidr_el0
            mov  x2, sp
            stp  x2, x3, [x1, #(8 * 32)]
            adr  x4, 1f
            ldr  x4, [x0, #(8 * 34)]
            //stp  x0,  x1, [x1, #(16 * 0)]
            //stp  x2,  x3, [x1, #(16 * 1)]
            //stp  x4,  x5, [x1, #(16 * 2)]
            //stp  x6,  x7, [x1, #(16 * 3)]
            stp  x8,  x9, [x1, #(16 * 4)]
            stp x10, x11, [x1, #(16 * 5)]
            stp x12, x13, [x1, #(16 * 6)]
            stp x14, x15, [x1, #(16 * 7)]
            stp x16, x17, [x1, #(16 * 8)]
            stp x18, x19, [x1, #(16 * 9)]
            stp x20, x21, [x1, #(16 * 10)]
            stp x22, x23, [x1, #(16 * 11)]
            stp x24, x25, [x1, #(16 * 12)]
            stp x26, x27, [x1, #(16 * 13)]
            stp x28, x29, [x1, #(16 * 14)]
            str x30,      [x1, #(16 * 15)]
            b   {}
        1:
    ",
    sym run_task,
    in("x0") next_context_data_address,
    in("x1") now_context_data_address
    );
}
