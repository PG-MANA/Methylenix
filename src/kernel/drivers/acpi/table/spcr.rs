//!
//! Serial Port Console Redirection
//!
//! This manager contains the information of SPCR

use super::{AcpiTable, OptionalAcpiTable};
use crate::kernel::drivers::acpi::GenericAddress;

use crate::kernel::memory_manager::data_type::{Address, VAddress};

#[repr(C, packed)]
struct SPCR {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: [u8; 4],
    creator_revision: u32,
    interface_type: u8,
    reserved: [u8; 3],
    base_address: [u8; 12],
    interrupt_type: u8,
    global_system_interrupt: u32,
    baud_rate: u8,
    parity: u8,
    stop_bits: u8,
    flow_control: u8,
    terminal_type: u8,
    language: u8,
    pci_device_id: u16,
    pci_vendor_id: u16,
    pci_bus_number: u8,
    pci_device_number: u8,
    pci_function_number: u8,
    pci_flags: u32,
    pci_segment: u8,
    clock_frequency: u32,
}

pub struct SpcrManager {
    base_address: VAddress,
}

impl AcpiTable for SpcrManager {
    const SIGNATURE: [u8; 4] = *b"SPCR";

    fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    fn init(&mut self, vm_address: VAddress) -> Result<(), ()> {
        /* vm_address must be accessible */
        let spcr = unsafe { &*(vm_address.to_usize() as *const SPCR) };
        if spcr.revision < 2 {
            pr_err!("Not supported SPCR revision:{}", spcr.revision);
        }
        self.base_address = remap_table!(vm_address, spcr.length);

        return Ok(());
    }
}

impl OptionalAcpiTable for SpcrManager {}

impl SpcrManager {
    pub const INTERFACE_TYPE_ARM_PL011: u8 = 0x03;
    pub const INTERFACE_TYPE_ARM_SBSA_GENERIC: u8 = 0x0E;

    pub fn get_memory_mapped_io_base_address(&self) -> Option<usize> {
        if self.base_address.is_zero() {
            return None;
        }
        let spcr = unsafe { &*(self.base_address.to_usize() as *const SPCR) };
        let base_address = GenericAddress::new(&spcr.base_address);
        if base_address.space_id != GenericAddress::ADDRESS_SPACE_ID_SYSTEM_MEMORY {
            None
        } else {
            Some(base_address.address as usize)
        }
    }

    pub fn get_interface_type(&self) -> u8 {
        unsafe { &*(self.base_address.to_usize() as *const SPCR) }.interface_type
    }
}
