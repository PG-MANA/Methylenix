//!
//! Memory Layout for Virtual Memory
//!
//! This module defines memory layout of kernel.

use crate::kernel::memory_manager::data_type::{Address, MSize, PAddress, VAddress};

use core::ops::RangeInclusive;

/// DIRECT_MAP_START_ADDRESS is also defined in arch/target_arch/boot/common.s.
pub const DIRECT_MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_a000_0000_0000);
pub const DIRECT_MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_bfff_ffff_ffff);
pub const DIRECT_MAP_BASE_ADDRESS: PAddress = PAddress::new(0);
pub const DIRECT_MAP_MAX_SIZE: MSize = MSize::new(1024 * 1024 * 1024 * 1024); /* 1TB */
pub const MALLOC_START_ADDRESS: VAddress = VAddress::new(0xffff_d100_0000_0000);
pub const MALLOC_END_ADDRESS: VAddress = VAddress::new(0xffff_dfff_ffff_ffff);
pub const MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_e000_0000_0000);
pub const MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_efff_ffff_ffff);
/// KERNEL_MAP_START_ADDRESS is also defined in arch/target_arch/boot/common.s and linker script.
pub const KERNEL_MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_ff80_0000_0000);
//pub const KERNEL_MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_ffef_ffff_ffff);
pub const USER_STACK_START_ADDRESS: VAddress = VAddress::new(0x0000_7000_0000_0000);
pub const USER_STACK_END_ADDRESS: VAddress = VAddress::new(0x0000_7fff_ffff_ffff);
pub const USER_END_ADDRESS: VAddress = VAddress::new(0x0000_7fff_ffff_ffff);
const CANONICAL_AREA_LOW: RangeInclusive<VAddress> =
    VAddress::new(0)..=VAddress::new(0x0000_7fff_ffff_ffff);
pub const CANONICAL_AREA_HIGH: RangeInclusive<VAddress> =
    VAddress::new(0xffff_8000_0000_0000)..=VAddress::new(0xffff_ffff_ffff_ffff);

const _CHECK_MEMORY_LAYOUT: () = const {
    // TODO: const trait
    if (KERNEL_MAP_START_ADDRESS.to_usize() & ((1usize << 39) - 1)) != 0 {
        panic!("KERNEL_MAP_START_ADDRESS is not pml4 aligned.");
    }
    // TODO: const trait
    if (DIRECT_MAP_START_ADDRESS.to_usize() & ((1usize << 39) - 1)) != 0 {
        panic!("KERNEL_MAP_START_ADDRESS is not pml4 aligned.");
    }
};

pub fn is_address_canonical(start_address: VAddress, end_address: VAddress) -> bool {
    if CANONICAL_AREA_LOW.contains(&start_address) {
        CANONICAL_AREA_LOW.contains(&end_address)
    } else {
        CANONICAL_AREA_HIGH.contains(&start_address) && CANONICAL_AREA_HIGH.contains(&end_address)
    }
}

pub fn get_direct_map_start_address() -> VAddress {
    DIRECT_MAP_START_ADDRESS
}

pub fn get_direct_map_base_address() -> PAddress {
    DIRECT_MAP_BASE_ADDRESS
}

pub fn get_direct_map_size() -> MSize {
    DIRECT_MAP_MAX_SIZE
}

pub fn kernel_area_to_physical_address(kernel_virtual_address: VAddress) -> PAddress {
    PAddress::new((kernel_virtual_address - KERNEL_MAP_START_ADDRESS).to_usize())
}

pub fn direct_map_to_physical_address(direct_map_virtual_address: VAddress) -> PAddress {
    PAddress::new((direct_map_virtual_address - DIRECT_MAP_START_ADDRESS).to_usize())
}

pub fn is_direct_mapped(physical_address: PAddress) -> bool {
    (physical_address - DIRECT_MAP_BASE_ADDRESS)
        <= (DIRECT_MAP_END_ADDRESS - DIRECT_MAP_START_ADDRESS)
}

pub fn physical_address_to_direct_map(physical_address: PAddress) -> VAddress {
    VAddress::new(
        (physical_address - DIRECT_MAP_BASE_ADDRESS + DIRECT_MAP_START_ADDRESS).to_usize(),
    )
}

pub fn is_user_memory_area(address: VAddress) -> bool {
    address <= USER_END_ADDRESS
}
