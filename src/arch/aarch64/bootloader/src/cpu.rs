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

pub unsafe fn get_id_aa64mmfr1_el1() -> u64 {
    let id: u64;
    asm!("mrs {:x}, id_aa64mmfr1_el1", out(reg) id);
    id
}

pub unsafe fn set_ttbr1_el1(address: u64) {
    asm!("msr ttbr1_el1, {:x}", in(reg) address);
}

pub unsafe fn get_tcr_el1() -> u64 {
    let tcr: u64;
    asm!("mrs {:x}, tcr_el1", out(reg) tcr);
    tcr
}

pub unsafe fn set_tcr_el1(tcr_el1: u64) {
    asm!("msr tcr_el1, {:x}", in(reg) tcr_el1);
}

pub unsafe fn get_tcr_el2() -> u64 {
    let tcr: u64;
    asm!("mrs {:x}, tcr_el2", out(reg) tcr);
    tcr
}

pub unsafe fn get_sctlr_el1() -> u64 {
    let sctlr: u64;
    asm!("mrs {:x}, sctlr_el1", out(reg) sctlr);
    sctlr
}

pub unsafe fn set_sctlr_el1(sctlr_el1: u64) {
    asm!("msr sctlr_el1, {:x}", in(reg) sctlr_el1);
}

pub unsafe fn get_hcr_el2() -> u64 {
    let hcr: u64;
    asm!("mrs {:x}, hcr_el2", out(reg) hcr);
    hcr
}

pub unsafe fn set_hcr_el2(hcr_el2: u64) {
    asm!("msr hcr_el2, {:x}", in(reg) hcr_el2);
}

pub unsafe fn get_mair_el1() -> u64 {
    let mair: u64;
    asm!("mrs {:x}, mair_el1", out(reg) mair);
    mair
}

pub unsafe fn cli() {
    asm!("  mrs {t}, DAIF
            orr {t}, {t}, (1 << 6) | (1 << 7)
            msr DAIF, {t}
            ", t = out(reg) _);
}

pub unsafe fn tlbi_asid_el1(asid: u64) {
    asm!("tlbi aside1, {:x}", in(reg) asid);
}

#[inline(always)]
pub fn dsb() {
    unsafe { asm!("dsb sy") };
}

pub fn flush_instruction_cache() {
    unsafe { asm!("isb") };
    unsafe { asm!("ic ialluis") };
}

pub fn flush_data_cache() {
    let clidr: u64;
    dsb();
    unsafe { asm!("mrs {:x}, clidr_el1", out(reg) clidr) };
    /* Check All Cache Type */
    for cache_level in 0..7 {
        let ccsidr: u64;
        let cache_type = (clidr >> (3 * cache_level)) & 0b111;
        if cache_type == 0b000 {
            break; /* No Cache, Ignore the rest */
        }
        unsafe {
            asm!("msr csselr_el1, {:x}\nisb\nmrs {:x}, ccsidr_el1",
            in(reg) cache_level << 1,
            out(reg) ccsidr)
        };
        let num_sets = (ccsidr & ((1 << 27) - 1)) >> 13;
        let associativity = (ccsidr & ((1 << 13) - 1)) >> 3;
        let line_size = ccsidr & 0b111;
        let a = (associativity as u32).leading_zeros();
        let l = line_size + 4;
        for set in 0..=num_sets {
            for way in 0..=associativity {
                unsafe {
                    asm!("dc cisw, {:x}", in(reg) (way << a) | (set << l) | (cache_level << 1))
                };
            }
        }
    }
    dsb();
    unsafe { asm!("msr csselr_el1, {:x}", in(reg) 0) };
}

pub unsafe fn jump_to_el1() {
    asm!("
            mrs {tmp}, midr_el1
            msr vpidr_el2, {tmp}
            mrs {tmp}, mpidr_el1
            msr vmpidr_el2, {tmp}
            mov {tmp}, (1 << 11) | (1 << 10) | (1 << 9) | (1 << 8) | (1 << 1) | (1 << 0)
            msr cnthctl_el2, {tmp}
            mov {tmp}, sp
            msr sp_el1, {tmp}
            adr {tmp}, 1f
            msr elr_el2, {tmp}
            mrs {tmp}, tcr_el2
            msr tcr_el1, {tmp}
            mrs {tmp}, ttbr0_el2
            msr ttbr0_el1, {tmp}
            mrs {tmp}, vbar_el2
            msr vbar_el1, {tmp}
            mrs {tmp}, sctlr_el2
            msr sctlr_el1, {tmp}
            mrs {tmp}, mair_el2
            msr mair_el1, {tmp}
            mov {tmp}, 0xC5
            msr spsr_el2, {tmp}
            mov {tmp}, (1 << 47) | (1 << 41) | (1 << 40)
            orr {tmp}, {tmp}, (1 << 31)
            orr {tmp}, {tmp}, (1 << 19)
            msr hcr_el2, {tmp}
            isb
            eret
    1:", tmp = out(reg) _);
}
