/*
 *  Multiple APIC Description Table
 */

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
    creator_revision: [u8; 4],
    flags: u32,
    local_interrupt_controller_address: u32,
    /* interrupt_controller_structure: [struct; n] */
}

pub struct MadtManager {
    base_address: VAddress,
    enabled: bool,
}

impl MadtManager {
    pub const SIGNATURE: [u8; 4] = ['A' as u8, 'P' as u8, 'I' as u8, 'C' as u8];

    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
            enabled: false,
        }
    }

    pub fn init(&mut self, madt_vm_address: VAddress) -> bool {
        /* madt_vm_address must be accessible */
        let madt = unsafe { &*(madt_vm_address.to_usize() as *const MADT) };
        if madt.revision != 4 {
            pr_err!("Not supported MADT version: {}", madt.revision);
        }
        if let Ok(a) = get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .mremap_dev(
                madt_vm_address,
                INITIAL_MMAP_SIZE.into(),
                MSize::new(madt.length as usize),
            )
        {
            self.base_address = a;
            self.enabled = true;
            true
        } else {
            pr_err!("Cannot reserve memory area of MADT.");
            false
        }
    }

    /// Find the Local APIC ID list
    ///
    /// This function will search the Local APIC ID from the Interrupt Controller Structures.
    /// Each Local APIC ID will be stored in list_to_store and this function will return the number of cores.
    /// If the size of list_to_store is smaller than the number of cores,
    /// this function sets Local APIC ID the size of list_to_store, and return.
    /// Currently, this cannot find more than 256 cores.
    pub fn find_apic_id_list(&self, list_to_store: &mut [u8]) -> usize {
        if !self.enabled {
            return 0;
        }
        let madt = unsafe { &*(self.base_address.to_usize() as *const MADT) };
        let length = madt.length as usize - core::mem::size_of::<MADT>();
        let base_address = self.base_address.to_usize() + core::mem::size_of::<MADT>();
        let mut pointer = 0;
        let mut list_pointer = 0;
        while length > pointer {
            let record_type = unsafe { *((base_address + pointer) as *const u8) };
            let record_length = unsafe { *((base_address + pointer + 1) as *const u8) };
            match record_type {
                0 => {
                    if list_pointer >= list_to_store.len() {
                        return list_pointer;
                    }
                    list_to_store[list_pointer] =
                        unsafe { *((base_address + pointer + 3) as *const u8) };
                    list_pointer += 1;
                }
                _ => {}
            };
            pointer += record_length as usize;
        }
        return list_pointer;
    }
}
