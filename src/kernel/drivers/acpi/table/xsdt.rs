//!
//! Extended System Description Table
//!
//! This manager contains the information about Extended System Description Table(XSDT).
//! XSDT is the list of tables like MADT.

use super::dsdt::DsdtManager;
use super::fadt::FadtManager;
use super::ssdt::SsdtManager;
use super::INITIAL_MMAP_SIZE;
use super::{AcpiTable, OptionalAcpiTable};

use crate::arch::target_arch::context::memory_layout::{
    is_direct_mapped, physical_address_to_direct_map,
};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};
use crate::kernel::memory_manager::{free_pages, io_remap};

use core::mem::MaybeUninit;

pub struct XsdtManager {
    base_address: VAddress,
    /* Essential Managers */
    fadt_manager: MaybeUninit<FadtManager>,
    dsdt_manager: MaybeUninit<DsdtManager>,
}

impl XsdtManager {
    pub const fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
            fadt_manager: MaybeUninit::uninit(),
            dsdt_manager: MaybeUninit::uninit(),
        }
    }

    pub fn init(&mut self, xsdt_physical_address: PAddress) -> Result<(), ()> {
        let xsdt_vm_address = match io_remap!(
            xsdt_physical_address,
            MSize::new(INITIAL_MMAP_SIZE),
            MemoryPermissionFlags::rodata(),
            MemoryOptionFlags::PRE_RESERVED | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS
        ) {
            Ok(a) => a,
            Err(e) => {
                pr_err!("Failed to map XSDT: {:?}", e);
                return Err(());
            }
        };

        if unsafe { *(xsdt_vm_address.to_usize() as *const [u8; 4]) } != *b"XSDT" {
            pr_err!("Invalid XSDT Signature");
            return Err(());
        }
        if unsafe { *((xsdt_vm_address.to_usize() + 8) as *const u8) } != 1 {
            pr_err!("Not supported XSDT version");
            return Err(());
        }
        let xsdt_size = unsafe { *((xsdt_vm_address.to_usize() + 4) as *const u32) };
        let xsdt_vm_address = remap_table!(xsdt_vm_address, xsdt_size);
        self.base_address = xsdt_vm_address;

        let mut index = 0;
        let mut is_dsdt_initialized = false;
        let mut is_fadt_initialized = false;

        while let Some(entry_physical_address) = self.get_entry(index) {
            let vm_address = match io_remap!(
                entry_physical_address,
                MSize::new(INITIAL_MMAP_SIZE),
                MemoryPermissionFlags::rodata(),
                MemoryOptionFlags::PRE_RESERVED | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS
            ) {
                Ok(a) => a,
                Err(e) => {
                    pr_err!("Failed to map ACPI Table: {:?}", e);
                    return Err(());
                }
            };
            pr_info!(
                "{}",
                core::str::from_utf8(unsafe { &*(vm_address.to_usize() as *const [u8; 4]) })
                    .unwrap_or("----")
            );

            match unsafe { *(vm_address.to_usize() as *const [u8; 4]) } {
                FadtManager::SIGNATURE => {
                    let mut fadt_manager = FadtManager::new();
                    if let Err(e) = fadt_manager.init(vm_address) {
                        pr_err!("Failed to init FADT Manager: {:?}", e);
                        return Err(e);
                    }
                    self.fadt_manager.write(fadt_manager);
                    is_fadt_initialized = true;
                }
                DsdtManager::SIGNATURE => {
                    let mut dsdt_manager = DsdtManager::new();
                    if let Err(e) = dsdt_manager.init(vm_address) {
                        pr_err!("Failed to initialize DSDT Manager: {:?}", e);
                        return Err(e);
                    }
                    self.dsdt_manager.write(dsdt_manager);
                    is_dsdt_initialized = true;
                }
                _ => {
                    /* Skip */
                    if let Err(e) = get_kernel_manager_cluster()
                        .kernel_memory_manager
                        .free(vm_address)
                    {
                        pr_warn!("Failed to free a ACPI table: {:?}", e)
                    }
                }
            };

            index += 1;
        }

        if !is_fadt_initialized {
            pr_err!("Cannot find FADT.");
            return Err(());
        }
        if !is_dsdt_initialized {
            let vm_address = if let Ok(a) = io_remap!(
                unsafe { self.fadt_manager.assume_init_ref() }.get_dsdt_address(),
                MSize::new(INITIAL_MMAP_SIZE),
                MemoryPermissionFlags::rodata(),
                MemoryOptionFlags::PRE_RESERVED | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS
            ) {
                a
            } else {
                pr_err!("Failed to map memory of DSDT.");
                return Err(());
            };
            let mut dsdt_manager = DsdtManager::new();
            if let Err(e) = dsdt_manager.init(vm_address) {
                pr_err!("Failed to initialize DSDT Manager: {:?}", e);
                return Err(e);
            }
            self.dsdt_manager.write(dsdt_manager);
        }
        Ok(())
    }

    pub fn get_table_manager<T: AcpiTable + OptionalAcpiTable>(&self) -> Option<T> {
        if let Some(vm_address) = self.search_entry(&T::SIGNATURE) {
            let mut manager = T::new();
            if let Err(e) = manager.init(vm_address) {
                pr_err!("Failed to initialize the ACPI table manager: {:?}", e);
                if let Err(e) = get_kernel_manager_cluster()
                    .kernel_memory_manager
                    .free(vm_address)
                {
                    pr_warn!("Failed to free memory for ACPI table manager: {:?}", e);
                }
                return None;
            }
            return Some(manager);
        }
        None
    }

    pub fn get_fadt_manager(&self) -> &FadtManager {
        unsafe { self.fadt_manager.assume_init_ref() }
    }

    pub fn get_dsdt_manager(&self) -> &DsdtManager {
        unsafe { self.dsdt_manager.assume_init_ref() }
    }

    pub fn get_ssdt_manager<F>(&self, mut call_back: F) -> bool
    where
        F: FnMut(&SsdtManager) -> bool,
    {
        let mut index = 0;
        while let Some(entry_physical_address) = self.get_entry(index) {
            let result = io_remap!(
                entry_physical_address,
                MSize::new(INITIAL_MMAP_SIZE),
                MemoryPermissionFlags::rodata(),
                MemoryOptionFlags::PRE_RESERVED | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS
            ); /* To drop Mutex Lock */

            if let Ok(vm_address) = result {
                if unsafe { &*(vm_address.to_usize() as *const [u8; 4]) } == &SsdtManager::SIGNATURE
                {
                    let mut ssdt_manager = SsdtManager::new();
                    let result = ssdt_manager.init(vm_address);
                    if result.is_err() || !call_back(&ssdt_manager) {
                        if let Err(e) = result {
                            pr_err!("Failed to initialize SsdtManager: {:?}", e);
                        } else {
                            pr_err!("Failed to call the callback function for SsdtManager.");
                        }
                        if let Err(e) = get_kernel_manager_cluster()
                            .kernel_memory_manager
                            .free(vm_address)
                        {
                            pr_warn!("Failed to free memory mapping for SSDT: {:?}", e)
                        }
                        return false;
                    }
                } else if let Err(e) = get_kernel_manager_cluster()
                    .kernel_memory_manager
                    .free(vm_address)
                {
                    pr_warn!("Cannot free an ACPI table: {:?}", e)
                }
            } else {
                pr_err!("Cannot map ACPI Table: {:?}", result.unwrap_err());
                return false;
            };
            index += 1;
        }
        true
    }

    fn get_length(&self) -> usize {
        unsafe { *((self.base_address.to_usize() + 4) as *const u32) as usize }
    }

    fn get_entry(&self, index: usize) -> Option<PAddress> {
        if (self.get_length() - 0x24) >> 3 > index {
            Some(PAddress::new(unsafe {
                *((self.base_address.to_usize() + 0x24 + index * 8) as *const u64)
            } as usize))
        } else {
            None
        }
    }

    fn search_entry(&self, signature: &[u8; 4]) -> Option<VAddress> {
        let mut index = 0;
        macro_rules! map_table {
            ($address:expr) => {
                match io_remap!(
                    $address,
                    MSize::new(INITIAL_MMAP_SIZE),
                    MemoryPermissionFlags::rodata(),
                    MemoryOptionFlags::PRE_RESERVED
                        | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        pr_err!("Failed to map ACPI Table: {:?}", e);
                        return None;
                    }
                }
            };
        }
        while let Some(entry_physical_address) = self.get_entry(index) {
            if is_direct_mapped(entry_physical_address) {
                if unsafe {
                    &*(physical_address_to_direct_map(entry_physical_address).to_usize()
                        as *const [u8; 4])
                } == signature
                {
                    return Some(map_table!(entry_physical_address));
                }
            } else {
                let virtual_address = map_table!(entry_physical_address);
                if unsafe { &*(virtual_address.to_usize() as *const [u8; 4]) } == signature {
                    return Some(virtual_address);
                }
                if let Err(e) = free_pages!(virtual_address) {
                    pr_warn!("Failed to free the map of ACPI Table: {:?}", e)
                }
            }
            index += 1;
        }
        None
    }
}
