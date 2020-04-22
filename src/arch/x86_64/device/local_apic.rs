/*
 *Local APIC
 */

use arch::target_arch::device::cpu;

use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::{MemoryOptionFlags, MemoryPermissionFlags};

pub struct LocalApicManager {
    apic_id: u32,
    is_x2apic_enabled: bool,
    base_address: usize,
}

enum LocalApicRegisters {
    ApicId = 0x02,
    EOI = 0x0b,
    SIR = 0x0f,
}

impl LocalApicManager {
    const MSR_INDEX: u32 = 0x1b;
    const BASE_ADDR_MASK: u64 = 0xffffffffff000;
    const _XAPIC_ENABLED_MASK: u64 = 0x800;
    const X2APIC_ENABLED_MASK: u64 = 0x400;
    const CPUID_X2APIC_MASK: u32 = 1 << 21;
    const X2APIC_MSR_INDEX: u32 = 0x800;

    pub const fn new() -> Self {
        Self {
            apic_id: 0,
            is_x2apic_enabled: false,
            base_address: 0,
        }
    }

    pub fn init(&mut self) -> bool {
        let local_apic_msr = unsafe { cpu::rdmsr(LocalApicManager::MSR_INDEX) };
        let base_address = (local_apic_msr & LocalApicManager::BASE_ADDR_MASK) as usize;
        let is_x2apic_supported = unsafe {
            let mut eax = 1u32;
            let mut ebx = 0u32;
            let mut ecx = 0u32;
            let mut edx = 0u32;
            cpu::cpuid(&mut eax, &mut ebx, &mut ecx, &mut edx);
            ecx & LocalApicManager::CPUID_X2APIC_MASK != 0
        };
        if local_apic_msr & LocalApicManager::X2APIC_ENABLED_MASK == 0 && is_x2apic_supported {
            unsafe {
                cpu::wrmsr(
                    LocalApicManager::MSR_INDEX,
                    local_apic_msr | LocalApicManager::X2APIC_ENABLED_MASK,
                );
            }
        }
        self.apic_id = 0;
        self.is_x2apic_enabled = is_x2apic_supported;
        match get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .memory_remap(
                base_address,
                0x1000,
                MemoryPermissionFlags::data(),
                MemoryOptionFlags::new(MemoryOptionFlags::NORMAL),
            ) {
            Ok(address) => {
                self.base_address = address;
            }
            Err(err) => {
                pr_err!("Cannot reserve memory of Local APIC: {}", err);
                return false;
            }
        };
        drop(base_address); /* avoid page fault */

        self.apic_id = self.read_apic_register(LocalApicRegisters::ApicId);
        self.write_apic_register(
            LocalApicRegisters::SIR,
            self.read_apic_register(LocalApicRegisters::SIR) | 0x100,
        );
        pr_info!("APIC ID:{}(x2APIC:{})", self.apic_id, is_x2apic_supported);
        return true;
    }

    pub fn get_apic_id(&self) -> u32 {
        self.apic_id
    }

    pub fn send_eoi(&self) {
        self.write_apic_register(LocalApicRegisters::EOI, 0);
    }

    fn read_apic_register(&self, index: LocalApicRegisters) -> u32 {
        if self.is_x2apic_enabled {
            unsafe { cpu::rdmsr(LocalApicManager::X2APIC_MSR_INDEX + (index as u32)) as u32 }
        } else {
            unsafe {
                core::ptr::read_volatile(
                    (self.base_address + (index as usize) * 0x10) as *const u32,
                )
            }
        }
    }

    fn write_apic_register(&self, index: LocalApicRegisters, data: u32) {
        if self.is_x2apic_enabled {
            unsafe {
                cpu::wrmsr(
                    LocalApicManager::X2APIC_MSR_INDEX + (index as u32),
                    data as u64,
                );
            }
        } else {
            unsafe {
                core::ptr::write_volatile(
                    (self.base_address + (index as usize) * 0x10) as *mut u32,
                    data,
                );
            }
        }
    }
}
