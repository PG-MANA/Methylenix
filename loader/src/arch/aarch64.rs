//!
//! AArch64 Arch specific functions
//!

pub mod context {
    pub mod context_data {
        /// Only for the compatibility
        pub struct ContextData {}
    }

    pub mod memory_layout {
        use crate::kernel::memory_manager::data_type::*;

        pub static mut DIRECT_MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_0000_0000_0000);
        pub static mut HIGH_MEMORY_START_ADDRESS: VAddress = VAddress::new(0xffff_0000_0000_0000);

        pub fn get_direct_map_base_address() -> PAddress {
            PAddress::new(0)
        }

        /// Only for the compatibility, assumes direct mapping
        /// If you convert to direct mapped virtual address in the loader,
        /// use [`direct_map_to_physical_address_loader`].
        pub fn direct_map_to_physical_address(v: VAddress) -> PAddress {
            unsafe { v.to_direct_mapped_p_address() }
        }

        pub fn direct_map_to_physical_address_loader(v: VAddress) -> PAddress {
            PAddress::new(
                v.to_usize() - get_direct_map_start_address().to_usize()
                    + get_direct_map_base_address().to_usize(),
            )
        }

        /// Only for the compatibility, assumes direct mapping
        /// If you convert to direct mapped virtual address in the loader,
        /// use [`physical_address_to_direct_map_loader`].
        pub fn physical_address_to_direct_map(p: PAddress) -> VAddress {
            unsafe { p.to_direct_mapped_v_address() }
        }

        pub fn physical_address_to_direct_map_loader(p: PAddress) -> VAddress {
            VAddress::new(
                p.to_usize() - get_direct_map_base_address().to_usize()
                    + get_direct_map_start_address().to_usize(),
            )
        }

        pub fn get_high_memory_base_address() -> VAddress {
            unsafe { DIRECT_MAP_START_ADDRESS }
        }

        pub fn get_direct_map_start_address() -> VAddress {
            unsafe { HIGH_MEMORY_START_ADDRESS }
        }

        pub fn get_direct_map_end_address() -> VAddress {
            VAddress::new(0xffff_ff1f_ffff_ffff)
        }

        pub fn get_direct_map_size() -> MSize {
            get_direct_map_end_address() - get_direct_map_start_address() + MSize::new(1)
        }
    }
}

mod interrupt {
    pub struct InterruptManager {}

    impl InterruptManager {
        pub extern "C" fn get_interrupt_stack() -> usize {
            unimplemented!()
        }
    }
}

#[allow(dead_code)]
pub mod device {
    pub mod cpu {
        include!("../../../src/arch/aarch64/device/cpu.rs");

        #[inline(always)]
        pub fn get_tcr_el2() -> u64 {
            let result: u64;
            unsafe { asm!("mrs {}, tcr_el2", out(reg) result) };
            result
        }

        #[inline(always)]
        pub fn get_mair_el2() -> u64 {
            let result: u64;
            unsafe { asm!("mrs {}, mair_el2", out(reg) result) };
            result
        }

        #[inline(always)]
        pub fn get_sctlr_el2() -> u64 {
            let result: u64;
            unsafe { asm!("mrs {}, sctlr_el2", out(reg) result) };
            result
        }

        #[inline(always)]
        pub unsafe fn set_sctlr_el2(sctlr_el2: u64) {
            unsafe { asm!("msr sctlr_el2, {}", in(reg) sctlr_el2) };
        }
    }
}

#[allow(dead_code)]
pub mod paging {
    include!("../../../src/arch/aarch64/paging/mod.rs");
}

pub const ELF_MACHINE_NATIVE: u16 = crate::kernel::file_manager::elf::ELF_MACHINE_AA64;

use self::device::cpu;

use core::arch::asm;

pub fn setup_environment() {}

pub fn dump_system() {
    let current_el = cpu::get_current_el();
    println!("CurrentEL: {current_el}");
    println!("SCTLR_EL1: {:#X}", cpu::get_sctlr());
    println!("ID_AA64MMFR0_EL1: {:#X}", cpu::get_id_aa64mmfr0());
    println!("ID_AA64MMFR1_EL1: {:#X}", cpu::get_id_aa64mmfr1());
    println!(
        "Supported PA Size: {} bits",
        cpu::convert_pa_range_to_pa_bits(cpu::get_id_aa64mmfr0())
    );
    if current_el == 2 {
        println!("TCR_EL2: {:#X}", cpu::get_tcr_el2());
        println!("MAIR_EL2: {:#X}", cpu::get_mair_el2());
    } else {
        println!("TCR_EL1: {:#X}", cpu::get_tcr());
        println!("MAIR_EL1: {:#X}", cpu::get_mair());
    }
}

pub unsafe fn jump_to_el1() {
    unsafe {
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
        mov {tmp}, (1 << 47) /* FIEN */ | (1 << 41) /* API */ | (1 << 40) /* APK */
        orr {tmp}, {tmp}, (1 << 31) /* RW */
        msr hcr_el2, {tmp}
        isb
        eret
        1:", tmp = out(reg) _)
    };
}

#[cfg(target_os = "none")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".start")]
/// Setup relocation, Clear the .bss area, and jump to the main.
extern "C" fn _start() {
    unimplemented!()
}

/// Jump to the kernel
///
/// # Kernel arguments
/// - x0: pointer of [`crate::kernel::drivers::boot_information::BootInformation`]
#[inline(always)]
pub unsafe fn jump_to_kernel(
    entry_point: usize,
    boot_info: usize,
    stack: usize,
    mut page_manager: paging::PageManager,
) -> ! {
    match cpu::get_current_el() {
        1 => { /* Do nothing, just jump to the kernel */ }
        2 => {
            if (cpu::get_id_aa64mmfr1() & (0b1111 << 8)) != 0 {
                /* FEAT_VHE is supported */
                unsafe {
                    /* Enable FP accesses */
                    cpu::set_cptr(
                        (0b11 << 24) /* SMEN */ | (0b11 << 20) /* FPEN */ | (0b11 << 16), /* ZEN */
                    );
                    /* Disable the paging to enable E2H */
                    cpu::set_sctlr_el2(cpu::get_sctlr_el2() & !1 /* M */);
                    /* Enable E2H */
                    cpu::set_hcr(
                        (1 << 34) /* E2H */ | (1 << 31) /* RW */ | (1 << 27), /* TGE */
                    );
                }
            } else {
                /* Enable FP accesses */
                unsafe {
                    cpu::set_cptr(
                        (0b11 << 24)/* SMEN */|(0b11 << 20)/* FPEN */ | (0b11 << 16), /* ZEN */
                    )
                };
                unsafe { jump_to_el1() };
            }
        }
        _ => unreachable!(),
    }

    /* Jump to the kernel */
    page_manager.flush_page_table();
    unsafe {
        asm!("
        mov sp, x1
        mrs x3, sctlr_el1
        orr x3, x3, 1 /* M */
        msr sctlr_el1, x3
        br  x2",
        in("x0") boot_info,
        in("x1") stack,
        in("x2") entry_point,
        options(noreturn))
    }
}
