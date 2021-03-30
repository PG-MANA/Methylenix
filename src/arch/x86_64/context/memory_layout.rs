//!
//! Memory Layout for Virtual Memory
//!
//! This module defines memory layout of kernel.

use crate::kernel::memory_manager::data_type::{MSize, VAddress};

use core::ops::RangeInclusive;

pub const MALLOC_START_ADDRESS: VAddress = VAddress::new(0xffffc90000000000);
pub const MALLOC_END_ADDRESS: VAddress = VAddress::new(0xffffe8ffffffffff);
const CANONICAL_AREA_LOW: RangeInclusive<VAddress> =
    VAddress::new(0)..=VAddress::new(0x0000_7fff_ffff_ffff);
const CANONICAL_AREA_HIGH: RangeInclusive<VAddress> =
    VAddress::new(0xffff_8000_0000_0000)..=VAddress::new(0xffff_ffff_ffff_ffff);

pub fn is_address_canonical(start_address: VAddress, end_address: VAddress) -> bool {
    if CANONICAL_AREA_LOW.contains(&start_address) {
        if CANONICAL_AREA_LOW.contains(&end_address) {
            true
        } else {
            false
        }
    } else {
        if CANONICAL_AREA_HIGH.contains(&start_address)
            && CANONICAL_AREA_HIGH.contains(&end_address)
        {
            true
        } else {
            false
        }
    }
}

pub fn adjust_start_address_to_be_canonical(
    start_address: VAddress,
    end_address: VAddress,
) -> Option<VAddress> {
    if (&start_address > CANONICAL_AREA_LOW.end() && &start_address < CANONICAL_AREA_HIGH.start())
        || (&start_address <= CANONICAL_AREA_LOW.end()
            && &end_address < CANONICAL_AREA_HIGH.start())
    {
        let size = MSize::from_address(start_address, end_address);
        let new_start_address = CANONICAL_AREA_HIGH.start().clone();
        let new_end_address = size.to_end_address(new_start_address);
        if CANONICAL_AREA_HIGH.contains(&new_end_address) {
            Some(new_start_address)
        } else {
            None
        }
    } else {
        Some(start_address)
    }
}
