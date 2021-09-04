//!
//! Multiple APIC Description Table
//!
//! This manager contains the information of MADT
//! MADT has the list of Local APIC IDs.

use super::super::INITIAL_MMAP_SIZE;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

#[repr(C, packed)]
struct MADT {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: [u8; 4],
    creator_revision: u32,
    flags: u32,
    local_interrupt_controller_address: u32,
    /* interrupt_controller_structure: [struct; n] */
}

pub struct MadtManager {
    base_address: VAddress,
}

pub struct LocalApicIdIter {
    base_address: VAddress,
    pointer: MSize,
    length: MSize,
}

impl MadtManager {
    pub const SIGNATURE: [u8; 4] = *b"APIC";

    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    pub fn init(&mut self, madt_vm_address: VAddress) -> bool {
        /* madt_vm_address must be accessible */
        let madt = unsafe { &*(madt_vm_address.to_usize() as *const MADT) };
        if madt.revision > 4 {
            pr_err!("Not supported MADT version: {}", madt.revision);
        }
        self.base_address = remap_table!(madt_vm_address, madt.length);
        return true;
    }

    /// Find the Local APIC ID list
    ///
    /// This function will search the Local APIC ID from the Interrupt Controller Structures.
    /// Each Local APIC ID will be returned by  LocalApicIdIter.
    pub fn find_apic_id_list(&self) -> LocalApicIdIter {
        let madt = unsafe { &*(self.base_address.to_usize() as *const MADT) };
        let length = madt.length as usize - core::mem::size_of::<MADT>();
        let base_address = self.base_address + MSize::new(core::mem::size_of::<MADT>());

        LocalApicIdIter {
            base_address,
            pointer: MSize::new(0),
            length: MSize::new(length),
        }
    }

    /// Release memory map and drop my self
    ///
    /// When you finished your process, this function should be called to free memory mapping.
    pub fn release_memory_map(self) {
        if !self.base_address.is_zero() {
            if let Err(e) = get_kernel_manager_cluster()
                .memory_manager
                .free(self.base_address)
            {
                pr_warn!("Failed to free MADT: {:?}", e);
            }
        }
        drop(self)
    }
}

impl Iterator for LocalApicIdIter {
    type Item = u32;
    fn next(&mut self) -> Option<Self::Item> {
        if self.pointer >= self.length {
            return None;
        }
        let record_base = (self.base_address + self.pointer).to_usize();
        let record_type = unsafe { *(record_base as *const u8) };
        let record_length = unsafe { *((record_base + 1) as *const u8) };

        self.pointer += MSize::new(record_length as usize);
        match record_type {
            0 => {
                if (unsafe { *((record_base + 4) as *const u32) } & 1) == 1 {
                    /* Enabled */
                    Some(unsafe { *((record_base + 3) as *const u8) } as u32)
                } else {
                    self.next()
                }
            }
            9 => {
                if (unsafe { *((record_base + 8) as *const u32) } & 1) == 1 {
                    /* Enabled */
                    Some(unsafe { *((record_base + 4) as *const u32) })
                } else {
                    self.next()
                }
            }

            _ => self.next(),
        }
    }
}
