//!
//! x86_64 Arch specific functions
//!

pub mod context {
    pub mod context_data {
        /// Only for the compatibility
        pub struct ContextData {}
    }

    pub mod memory_layout {
        use crate::kernel::memory_manager::data_type::*;
        pub const CANONICAL_AREA_HIGH: core::ops::RangeInclusive<VAddress> =
            VAddress::new(0xffff_8000_0000_0000)..=VAddress::new(0xffff_ffff_ffff_ffff);

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
            VAddress::new(0xffff_a000_0000_0000)
        }

        pub fn get_direct_map_start_address() -> VAddress {
            get_high_memory_base_address()
        }

        pub fn get_direct_map_end_address() -> VAddress {
            VAddress::new(0xffff_bfff_ffff_ffff)
        }

        pub fn get_direct_map_size() -> MSize {
            get_direct_map_end_address() - get_direct_map_start_address() + MSize::new(1)
        }
    }
}

#[allow(dead_code)]
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
        include!("../../../src/arch/x86_64/device/cpu.rs");
    }
}

#[allow(dead_code)]
pub mod paging {
    include!("../../../src/arch/x86_64/paging/mod.rs");
}

pub const ELF_MACHINE_NATIVE: u16 = crate::kernel::file_manager::elf::ELF_MACHINE_AMD64;

pub fn setup_environment() {}

pub fn dump_system() {}

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
/// - rdi: pointer of [`crate::kernel::drivers::boot_information::BootInformation`]
#[inline(always)]
pub unsafe fn jump_to_kernel(
    entry_point: usize, // rdi
    boot_info: usize,   // rsi
    stack: usize,       // rdx
    mut page_manager: paging::PageManager,
) -> ! {
    /* Jump to the kernel */
    page_manager.flush_page_table();
    unsafe {
        core::arch::asm!("
        mov rsp, rsi
        jmp rdx",
        in("rdi") boot_info,
        in("rsi") stack,
        in("rdx") entry_point,
        options(noreturn))
    }
}
