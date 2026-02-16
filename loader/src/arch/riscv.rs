//!
//! RISC-V Arch specific functions
//!

pub mod context {
    pub mod context_data {
        /// Only for the compatibility
        pub struct ContextData {}
    }

    pub mod memory_layout {
        use crate::kernel::memory_manager::data_type::*;

        pub fn get_direct_map_base_address() -> PAddress {
            PAddress::new(0)
        }

        /// Only for the compatibility, assumes direct mapping
        pub fn direct_map_to_physical_address(v: VAddress) -> PAddress {
            unsafe { v.to_direct_mapped_p_address() }
        }

        /// Only for the compatibility, assumes direct mapping
        pub fn physical_address_to_direct_map(p: PAddress) -> VAddress {
            unsafe { p.to_direct_mapped_v_address() }
        }

        pub fn get_high_memory_base_address() -> VAddress {
            VAddress::new(0xffff_fff8_0000_0000)
        }

        pub fn get_direct_map_start_address() -> VAddress {
            get_high_memory_base_address()
        }

        pub fn get_direct_map_end_address() -> VAddress {
            VAddress::new(0xffff_ffff_bfff_ffff)
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
        include!("../../../src/arch/riscv64/device/cpu.rs");
    }
}

#[allow(dead_code)]
pub mod paging {
    include!("../../../src/arch/riscv64/paging/mod.rs");
}

pub const ELF_MACHINE_NATIVE: u16 = crate::kernel::file_manager::elf::ELF_MACHINE_RISCV;

pub fn setup_environment() {}

#[cfg(target_os = "none")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".start")]
/// Setup relocation, Clear the .bss area, and jump to the main.
/// `a0` and `a1` must be reserved.
extern "C" fn _start() {
    core::arch::naked_asm!("
    .extern __REL_START, __REL_END
    .extern __BSS_START, __BSS_END
    .extern __LOADER_END
    // This must be the first instruction
    auipc   t0, 0
    lla     t1, __REL_START
    lla     t2, __REL_END
    li      t3, 0xAA55
    slli    t4, t3, 16
1:
    ld      t5, (t1)
    srli    t6, t5, 16
    bne     t6, t3, 2f
    // t5 == 0x0000_0000_AA55_xxxx
    xor     t5, t5, t4  // t5 ^= 0xAA55_0000
    or      t5, t5, t0  // t5 |= t0(base_address)
    sd      t5, (t1)
2:
    addi    t1, t1, 8
    bne     t1, t2, 1b

    // Clear .bss
    lla     t1, __BSS_START
    lla     t2, __BSS_END
3:
    sd      x0, (t1)
    addi    t1, t1, 8
    bne     t1, t2, 3b

    // Jump to main
    mv      a2, t0
    lla     a3, __LOADER_END
    //xor     a3, a3, t4
    //add     a3, a3, a2
    j {main}", main = sym crate::baremetal_main);
}

/// Jump to the kernel
///
/// # Kernel arguments
/// - a0: hartid (U-Boot will set mhartid to tp)
/// - a1: dtb address
/// - a2: pointer of [`crate::kernel::drivers::boot_information::BootInformation`]
#[inline(always)]
pub unsafe fn jump_to_kernel(
    entry_point: usize,
    dtb_address: usize,
    boot_info: usize,
    stack: usize,
    mut page_manager: paging::PageManager,
) -> ! {
    page_manager.flush_page_table();

    unsafe {
        core::arch::asm!("
            mv  sp, {stack}
            mv  a0, tp
            jr  {entry_point}",
            in("a0") 0,
            in("a1") dtb_address,
            in("a2") boot_info,
            stack = in(reg) stack,
            entry_point = in(reg) entry_point,
            options(noreturn)
        )
    }
}
