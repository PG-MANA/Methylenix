//!
//! Memory Layout for Virtual Memory
//!
//! This module defines memory layout of kernel.
//! Supported: Sv39, Sv47, Sv56

use crate::kernel::memory_manager::data_type::{Address, MSize, PAddress, VAddress};

// The below constants are for Sv47
/*
const HIGH_MEMORY_START_ADDRESS: VAddress = VAddress::new(0xffff_ff80_0000_0000);
const DIRECT_MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_ff80_0000_0000);
pub const DIRECT_MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_ffbf_ffff_ffff);
pub const DIRECT_MAP_BASE_ADDRESS: PAddress = PAddress::new(0);
pub const MALLOC_START_ADDRESS: VAddress = VAddress::new(0xffff_ffc0_0000_0000);
pub const MALLOC_END_ADDRESS: VAddress = VAddress::new(0xffff_ffcf_ffff_ffff);
pub const MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_ffd0_0000_0000);
pub const MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_ffdf_ffff_ffff);
/// KERNEL_MAP_START_ADDRESS is also defined in linker script.
pub const KERNEL_MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_ffe0_0000_0000);
//pub const KERNEL_MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_ffef_ffff_ffff);

pub const USER_STACK_START_ADDRESS: VAddress = VAddress::new(0x0000_7000_0000_0000);
pub const USER_STACK_END_ADDRESS: VAddress = VAddress::new(0x0000_7fff_ffff_ffff);
pub const USER_END_ADDRESS: VAddress = VAddress::new(0x0000_7fff_ffff_ffff);
*/

// The below constants are for Sv39
const HIGH_MEMORY_START_ADDRESS: VAddress = VAddress::new(0xffff_fff8_0000_0000);
const DIRECT_MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_fff8_0000_0000);
pub const DIRECT_MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_ffff_bfff_ffff);
pub const DIRECT_MAP_BASE_ADDRESS: PAddress = PAddress::new(0);
pub const MALLOC_START_ADDRESS: VAddress = VAddress::new(0xffff_ffff_c000_0000);
pub const MALLOC_END_ADDRESS: VAddress = VAddress::new(0xffff_ffff_cfff_ffff);
pub const MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_ffff_d000_0000);
pub const MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_ffff_dfff_ffff);
/// KERNEL_MAP_START_ADDRESS is also defined in linker script.
pub const KERNEL_MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_ffff_e000_0000);
//pub const KERNEL_MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_ffff_efff_ffff);

pub const USER_STACK_START_ADDRESS: VAddress = VAddress::new(0x0000_0070_0000_0000);
pub const USER_STACK_END_ADDRESS: VAddress = VAddress::new(0x0000_007f_8fff_ffff);
pub const USER_END_ADDRESS: VAddress = VAddress::new(0x0000_007f_8fff_ffff);

pub fn get_high_memory_base_address() -> VAddress {
    HIGH_MEMORY_START_ADDRESS
}

pub fn get_direct_map_base_address() -> PAddress {
    DIRECT_MAP_BASE_ADDRESS
}

pub fn get_direct_map_start_address() -> VAddress {
    DIRECT_MAP_START_ADDRESS
}

pub fn get_direct_map_size() -> MSize {
    DIRECT_MAP_END_ADDRESS - DIRECT_MAP_START_ADDRESS + MSize::new(1)
}

pub fn kernel_area_to_physical_address(kernel_virtual_address: VAddress) -> PAddress {
    PAddress::new((kernel_virtual_address - KERNEL_MAP_START_ADDRESS).to_usize())
}

pub fn direct_map_to_physical_address(direct_map_virtual_address: VAddress) -> PAddress {
    assert!(
        (direct_map_virtual_address >= DIRECT_MAP_START_ADDRESS)
            && (direct_map_virtual_address <= DIRECT_MAP_END_ADDRESS)
    );
    PAddress::new(
        (direct_map_virtual_address - DIRECT_MAP_START_ADDRESS).to_usize()
            + DIRECT_MAP_BASE_ADDRESS.to_usize(),
    )
}

pub fn is_direct_mapped(physical_address: PAddress) -> bool {
    (physical_address - DIRECT_MAP_BASE_ADDRESS)
        <= (DIRECT_MAP_END_ADDRESS - DIRECT_MAP_START_ADDRESS)
}

pub fn physical_address_to_direct_map(physical_address: PAddress) -> VAddress {
    assert!(
        (physical_address - DIRECT_MAP_BASE_ADDRESS)
            <= (DIRECT_MAP_END_ADDRESS - DIRECT_MAP_START_ADDRESS)
    );
    VAddress::new(
        physical_address.to_usize() - DIRECT_MAP_BASE_ADDRESS.to_usize()
            + DIRECT_MAP_START_ADDRESS.to_usize(),
    )
}

pub fn is_user_memory_area(address: VAddress) -> bool {
    address <= USER_END_ADDRESS
}
