//!
//! Multiple APIC Description Table
//!
//! This manager contains the information of MADT.
//! It has the list of Local APIC IDs.

use super::{AcpiTable, OptionalAcpiTable};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

use core::ptr::read_unaligned;

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

pub struct GicCpuIter {
    base_address: VAddress,
    pointer: MSize,
    length: MSize,
}

pub struct GenericInterruptDistributorInfo {
    pub base_address: usize,
    pub version: u8,
}

pub struct GenericInterruptControllerCpuInfo {
    #[allow(dead_code)]
    pub acpi_processor_uid: u32,
    pub gicr_base_address: u64,
}

pub struct GenericInterruptRedistributorInfo {
    pub discovery_range_base_address: u64,
    pub discovery_range_length: u32,
}

impl AcpiTable for MadtManager {
    const SIGNATURE: [u8; 4] = *b"APIC";

    fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    fn init(&mut self, vm_address: VAddress) -> Result<(), ()> {
        /* vm_address must be accessible */
        let madt = unsafe { &*(vm_address.to_usize() as *const MADT) };
        if madt.revision > 5 {
            pr_err!("Not supported MADT version: {}", madt.revision);
        }
        self.base_address = remap_table!(vm_address, madt.length);
        Ok(())
    }
}

impl OptionalAcpiTable for MadtManager {}

impl MadtManager {
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

    pub fn get_generic_interrupt_controller_cpu_info_iter(&self) -> GicCpuIter {
        let madt = unsafe { &*(self.base_address.to_usize() as *const MADT) };
        let length = madt.length as usize - core::mem::size_of::<MADT>();
        let base_address = self.base_address + MSize::new(core::mem::size_of::<MADT>());

        GicCpuIter {
            base_address,
            pointer: MSize::new(0),
            length: MSize::new(length),
        }
    }

    pub fn find_generic_interrupt_controller_cpu_interface(
        &self,
        target_mpidr: u64,
    ) -> Option<GenericInterruptControllerCpuInfo> {
        if self.base_address.is_zero() {
            return None;
        }
        let madt = unsafe { &*(self.base_address.to_usize() as *const MADT) };
        let length = madt.length as usize - core::mem::size_of::<MADT>();
        let base_address = self.base_address + MSize::new(core::mem::size_of::<MADT>());
        let mut pointer = 0usize;
        while pointer < length {
            let record_base = base_address.to_usize() + pointer;
            let record_type = unsafe { read_unaligned(record_base as *const u8) };
            let record_length = unsafe { read_unaligned((record_base + 1) as *const u8) };

            if record_type == 0x0B
                && (unsafe { read_unaligned((record_base + 68) as *const u64) } == target_mpidr)
                && (unsafe { read_unaligned((record_base + 12) as *const u8) } & 1) != 0
            {
                return Some(GenericInterruptControllerCpuInfo {
                    acpi_processor_uid: unsafe { read_unaligned((record_base + 8) as *const u32) },
                    gicr_base_address: unsafe { read_unaligned((record_base + 60) as *const u64) },
                });
            }
            pointer += record_length as usize;
        }
        None
    }

    ///
    pub fn find_generic_interrupt_distributor(&self) -> Option<GenericInterruptDistributorInfo> {
        if self.base_address.is_zero() {
            return None;
        }
        let madt = unsafe { &*(self.base_address.to_usize() as *const MADT) };
        let length = madt.length as usize - core::mem::size_of::<MADT>();
        let base_address = self.base_address + MSize::new(core::mem::size_of::<MADT>());
        let mut pointer = 0usize;
        while pointer < length {
            let record_base = base_address.to_usize() + pointer;
            let record_type = unsafe { read_unaligned(record_base as *const u8) };
            let record_length = unsafe { read_unaligned((record_base + 1) as *const u8) };

            if record_type == 0x0C {
                return Some(GenericInterruptDistributorInfo {
                    base_address: unsafe {
                        read_unaligned((record_base + 8) as *const u64) as usize
                    },
                    version: unsafe { read_unaligned((record_base + 20) as *const u8) },
                });
            }
            pointer += record_length as usize;
        }
        None
    }

    pub fn find_generic_interrupt_redistributor_struct(
        &self,
    ) -> Option<GenericInterruptRedistributorInfo> {
        if self.base_address.is_zero() {
            return None;
        }
        let madt = unsafe { &*(self.base_address.to_usize() as *const MADT) };
        let length = madt.length as usize - core::mem::size_of::<MADT>();
        let base_address = self.base_address + MSize::new(core::mem::size_of::<MADT>());
        let mut pointer = 0usize;
        while pointer < length {
            let record_base = base_address.to_usize() + pointer;
            let record_type = unsafe { read_unaligned(record_base as *const u8) };
            let record_length = unsafe { read_unaligned((record_base + 1) as *const u8) };

            if record_type == 0x0E {
                return Some(GenericInterruptRedistributorInfo {
                    discovery_range_base_address: unsafe {
                        read_unaligned((record_base + 4) as *const u64)
                    },
                    discovery_range_length: unsafe {
                        read_unaligned((record_base + 12) as *const u32)
                    },
                });
            }
            pointer += record_length as usize;
        }
        None
    }

    /// Release memory map and drop my self
    ///
    /// When you finished your process, this function should be called to free memory mapping.
    pub fn release_memory_map(self) {
        if !self.base_address.is_zero() {
            if let Err(e) = get_kernel_manager_cluster()
                .kernel_memory_manager
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
        let record_type = unsafe { read_unaligned(record_base as *const u8) };
        let record_length = unsafe { read_unaligned((record_base + 1) as *const u8) };

        self.pointer += MSize::new(record_length as usize);
        match record_type {
            0 => {
                if (unsafe { read_unaligned((record_base + 4) as *const u32) } & 1) == 1 {
                    /* Enabled */
                    Some(unsafe { read_unaligned((record_base + 3) as *const u8) } as u32)
                } else {
                    self.next()
                }
            }
            9 => {
                if (unsafe { read_unaligned((record_base + 8) as *const u32) } & 1) == 1 {
                    /* Enabled */
                    Some(unsafe { read_unaligned((record_base + 4) as *const u32) })
                } else {
                    self.next()
                }
            }

            _ => self.next(),
        }
    }
}

impl Iterator for GicCpuIter {
    type Item = u64;
    fn next(&mut self) -> Option<Self::Item> {
        if self.pointer >= self.length {
            return None;
        }
        let record_base = (self.base_address + self.pointer).to_usize();
        let record_type = unsafe { read_unaligned(record_base as *const u8) };
        let record_length = unsafe { read_unaligned((record_base + 1) as *const u8) };
        self.pointer += MSize::new(record_length as usize);
        match record_type {
            0x0B => {
                if (unsafe { read_unaligned((record_base + 12) as *const u8) } & 1) != 0 {
                    /* Enabled */
                    Some(unsafe { read_unaligned((record_base + 68) as *const u64) })
                } else {
                    self.next()
                }
            }
            _ => self.next(),
        }
    }
}
