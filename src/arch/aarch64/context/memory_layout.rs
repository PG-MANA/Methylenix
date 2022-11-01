//!
//! Memory Layout for Virtual Memory
//!
//! This module defines memory layout of kernel.

use crate::kernel::memory_manager::data_type::{Address, MSize, PAddress, VAddress};

/// DIRECT_MAP_START_ADDRESS is also defined in boot_loader
//pub const DIRECT_MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_0000_0000_0000);
pub const DIRECT_MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_ff1f_ffff_ffff);
pub const DIRECT_MAP_BASE_ADDRESS: PAddress = PAddress::new(0);
//pub const DIRECT_MAP_MAX_SIZE: MSize =
//    DIRECT_MAP_END_ADDRESS - DIRECT_MAP_START_ADDRESS + MSize::new(1);
pub const MALLOC_START_ADDRESS: VAddress = VAddress::new(0xffff_ff20_0000_0000);
pub const MALLOC_END_ADDRESS: VAddress = VAddress::new(0xffff_ff4f_ffff_ffff);
pub const MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_ff50_0000_0000);
pub const MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_ff7f_ffff_ffff);
/// KERNEL_MAP_START_ADDRESS is also defined in linker script.
pub const KERNEL_MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_ff80_0000_0000);
//pub const KERNEL_MAP_END_ADDRESS: VAddress = VAddress::new(0xffff_ffef_ffff_ffff);
pub const USER_STACK_START_ADDRESS: VAddress = VAddress::new(0x0000_7000_0000_0000);
pub const USER_STACK_END_ADDRESS: VAddress = VAddress::new(0x0000_7fff_ffff_ffff);
pub const USER_END_ADDRESS: VAddress = VAddress::new(0x0000_7fff_ffff_ffff);

pub static mut DIRECT_MAP_START_ADDRESS: VAddress = VAddress::new(0xffff_0000_0000_0000);
pub static mut HIGH_MEMORY_START_ADDRESS: VAddress = VAddress::new(0xffff_0000_0000_0000);

pub fn get_direct_map_start_address() -> VAddress {
    unsafe { DIRECT_MAP_START_ADDRESS }
}

pub fn get_direct_map_max_size() -> MSize {
    DIRECT_MAP_END_ADDRESS - unsafe { DIRECT_MAP_START_ADDRESS } + MSize::new(1)
}

pub fn kernel_area_to_physical_address(kernel_virtual_address: VAddress) -> PAddress {
    PAddress::new((kernel_virtual_address - KERNEL_MAP_START_ADDRESS).to_usize())
}

pub fn direct_map_to_physical_address(direct_map_virtual_address: VAddress) -> PAddress {
    assert!(
        (direct_map_virtual_address >= unsafe { DIRECT_MAP_START_ADDRESS })
            && (direct_map_virtual_address <= DIRECT_MAP_END_ADDRESS)
    );
    PAddress::new(
        (direct_map_virtual_address - unsafe { DIRECT_MAP_START_ADDRESS }).to_usize()
            + DIRECT_MAP_BASE_ADDRESS.to_usize(),
    )
}

pub fn is_direct_mapped(physical_address: PAddress) -> bool {
    (physical_address - DIRECT_MAP_BASE_ADDRESS)
        <= (DIRECT_MAP_END_ADDRESS - unsafe { DIRECT_MAP_START_ADDRESS })
}

pub fn physical_address_to_direct_map(physical_address: PAddress) -> VAddress {
    assert!(
        (physical_address - DIRECT_MAP_BASE_ADDRESS)
            <= (DIRECT_MAP_END_ADDRESS - unsafe { DIRECT_MAP_START_ADDRESS })
    );
    VAddress::new(
        physical_address.to_usize() - DIRECT_MAP_BASE_ADDRESS.to_usize()
            + unsafe { DIRECT_MAP_START_ADDRESS }.to_usize(),
    )
}

pub fn is_user_memory_area(address: VAddress) -> bool {
    address <= USER_END_ADDRESS
}
