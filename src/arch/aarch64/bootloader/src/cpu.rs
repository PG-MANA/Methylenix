//!
//! AArch64 Assembly Instructions
//!

use core::arch::asm;

pub unsafe fn get_current_el() -> u64 {
    let c: u64;
    asm!("mrs {:x}, CurrentEL", out(reg) c);
    c
}

pub unsafe fn get_id_aa64mmfr0_el1() -> u64 {
    let id: u64;
    asm!("mrs {:x}, id_aa64mmfr0_el1", out(reg) id);
    id
}

pub unsafe fn get_ttbr1_el1() -> u64 {
    let ttbr: u64;
    asm!("mrs {:x}, ttbr1_el1", out(reg) ttbr);
    ttbr
}

pub unsafe fn set_ttbr1_el1(address: u64) {
    asm!("msr ttbr1_el1, {:x}", in(reg) address);
}

pub unsafe fn get_tcr_el1() -> u64 {
    let tcr: u64;
    asm!("mrs {:x}, tcr_el1", out(reg) tcr);
    tcr
}

pub unsafe fn get_tcr_el2() -> u64 {
    let tcr: u64;
    asm!("mrs {:x}, tcr_el2", out(reg) tcr);
    tcr
}

pub unsafe fn set_tcr_el1(tcr_el1: u64) {
    asm!("msr tcr_el1, {:x}", in(reg) tcr_el1);
}

pub unsafe fn get_sctlr_el1() -> u64 {
    let sctlr: u64;
    asm!("mrs {:x}, sctlr_el1", out(reg) sctlr);
    sctlr
}

pub unsafe fn set_sctlr_el1(sctlr_el1: u64) {
    asm!("msr sctlr_el1, {:x}", in(reg) sctlr_el1);
}

pub unsafe fn set_mair_el1(mair_el1: u64) {
    asm!("msr mair_el1, {:x}", in(reg) mair_el1);
}

pub unsafe fn cli() {
    asm!("  mrs {t}, DAIF
            orr {t}, {t}, (1 << 6) | (1 << 7)
            msr DAIF, {t}
            ", t = out(reg) _);
}
