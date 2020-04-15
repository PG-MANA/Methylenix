/*
 *Local APIC
 */

use arch::target_arch::device::cpu;

use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::memory_manager::MemoryPermissionFlags;

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

    pub fn init() -> LocalApicManager {
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
        if !get_kernel_manager_cluster()
            .memory_manager
            .lock()
            .unwrap()
            .reserve_memory(
                base_address,
                base_address,
                0x1000,
                MemoryPermissionFlags::data(),
                true,
                true,
            )
        {
            panic!("Cannot reserve memory of Local APIC.");
        }
        let mut local_apic_manager = LocalApicManager {
            apic_id: 0,
            is_x2apic_enabled: is_x2apic_supported,
            base_address,
        };
        local_apic_manager.apic_id =
            local_apic_manager.read_apic_register(LocalApicRegisters::ApicId);
        local_apic_manager.write_apic_register(
            LocalApicRegisters::SIR,
            local_apic_manager.read_apic_register(LocalApicRegisters::SIR) | 0x100,
        );
        pr_info!(
            "APIC ID:{}(x2APIC:{})",
            local_apic_manager.apic_id,
            is_x2apic_supported
        );
        local_apic_manager
    }

    pub fn get_running_cpu_local_apic_manager() -> LocalApicManager {
        let local_apic_msr = unsafe { cpu::rdmsr(LocalApicManager::MSR_INDEX) };
        let base_address = (local_apic_msr & LocalApicManager::BASE_ADDR_MASK) as usize;
        let is_x2apic_enabled = local_apic_msr & LocalApicManager::X2APIC_ENABLED_MASK != 0;
        let mut local_apic_manager = LocalApicManager {
            apic_id: 0,
            is_x2apic_enabled,
            base_address,
        };

        local_apic_manager.apic_id =
            local_apic_manager.read_apic_register(LocalApicRegisters::ApicId);
        local_apic_manager
    }

    pub fn get_apic_id(&self) -> u32 {
        self.apic_id
    }

    pub fn send_eoi() {
        let local_apic_manager = LocalApicManager::get_running_cpu_local_apic_manager();
        local_apic_manager.write_apic_register(LocalApicRegisters::EOI, 0);
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
