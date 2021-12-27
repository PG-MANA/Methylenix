//!
//! PCIe Memory Mapped Configuration
//!
//! This manager contains the information of MCFG

use super::{AcpiTable, OptionalAcpiTable};

use crate::kernel::memory_manager::data_type::{Address, VAddress};

#[repr(C, packed)]
struct MCFG {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: [u8; 4],
    creator_revision: u32,
    reserved: u64,
    /* interrupt_controller_structure: [struct; n] */
}

#[repr(C, packed)]
struct BaseAddressAllocationStructure {
    base_address: u64,
    pci_segment_group: u16,
    start_bus: u8,
    end_bus: u8,
    reserved: u32,
}

#[derive(Clone)]
pub struct PciBaseAddressInfo {
    pub base_address: u64,
    pub segment_group: u16,
    pub start_bus: u8,
    pub end_bus: u8,
}

pub struct McfgManager {
    base_address: VAddress,
}

impl AcpiTable for McfgManager {
    const SIGNATURE: [u8; 4] = *b"MCFG";

    fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    fn init(&mut self, vm_address: VAddress) -> Result<(), ()> {
        /* mcfg_vm_address must be accessible */
        let mcfg = unsafe { &*(vm_address.to_usize() as *const MCFG) };
        if mcfg.revision > 1 {
            pr_err!("Not supported MCFG revision:{}", mcfg.revision);
        }
        let mcfg_vm_address = remap_table!(vm_address, mcfg.length);
        self.base_address = mcfg_vm_address;

        return Ok(());
    }
}

impl OptionalAcpiTable for McfgManager {}

impl McfgManager {
    pub fn get_base_address_info(&self, index: usize) -> Option<PciBaseAddressInfo> {
        if self.base_address.is_zero() {
            return None;
        }
        const MCFG_SIZE: usize = core::mem::size_of::<MCFG>();
        const BASE_ADDRESS_ALLOCATION_STRUCT_SIZE: usize =
            core::mem::size_of::<BaseAddressAllocationStructure>();

        if ((unsafe { &*(self.base_address.to_usize() as *const MCFG) }.length) as usize
            - MCFG_SIZE)
            / BASE_ADDRESS_ALLOCATION_STRUCT_SIZE
            < index
        {
            return None;
        }
        let info = unsafe {
            &*((self.base_address.to_usize()
                + MCFG_SIZE
                + index * BASE_ADDRESS_ALLOCATION_STRUCT_SIZE)
                as *const BaseAddressAllocationStructure)
        };
        return Some(PciBaseAddressInfo {
            base_address: info.base_address,
            segment_group: info.pci_segment_group,
            start_bus: info.start_bus,
            end_bus: info.end_bus,
        });
    }
}
