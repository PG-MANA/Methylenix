//!
//! Local APIC Manager
//!
//! To read/write local apic register
//!

use crate::arch::target_arch::device::cpu;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, PAddress, VAddress};
use crate::kernel::memory_manager::MemoryPermissionFlags;

pub struct LocalApicManager {
    apic_id: u32,
    is_x2apic_enabled: bool,
    base_address: VAddress,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum LocalApicRegisters {
    ApicId = 0x02,
    EOI = 0x0b,
    SIR = 0x0f,
    LvtTimer = 0x32,
    TimerInitialCount = 0x38,
    TimerCurrentCount = 0x39,
    TimerDivide = 0x3e,
}

impl LocalApicManager {
    const MSR_INDEX: u32 = 0x1b;
    const BASE_ADDR_MASK: u64 = 0xffffffffff000;
    const XAPIC_ENABLED_MASK: u64 = 0x800;
    const X2APIC_ENABLED_MASK: u64 = 0x400;
    const CPUID_X2APIC_MASK: u32 = 1 << 21;
    const X2APIC_MSR_INDEX: u32 = 0x800;

    /// Create LocalApicManager with invalid address.
    ///
    /// Before use, **you must call [`init`]**.
    ///
    /// [`init`]: #method.init
    pub const fn new() -> Self {
        Self {
            apic_id: 0,
            is_x2apic_enabled: false,
            base_address: VAddress::new(0),
        }
    }

    /// Init LocalApicManager.
    ///
    /// This function checks if x2APIC is available by cpuid.
    /// If x2APIC is available, enable it, otherwise mmap_dev(local apic's base address).
    /// If mapping is failed, this function will return false.   
    /// After that, enable EOI-Broadcast Suppression.
    pub fn init(&mut self) -> bool {
        let local_apic_msr = unsafe { cpu::rdmsr(Self::MSR_INDEX) };
        let base_address = PAddress::from((local_apic_msr & Self::BASE_ADDR_MASK) as usize);
        let is_x2apic_supported = unsafe {
            let mut eax = 1u32;
            let mut ebx = 0u32;
            let mut ecx = 0u32;
            let mut edx = 0u32;
            cpu::cpuid(&mut eax, &mut ebx, &mut ecx, &mut edx);
            ecx & Self::CPUID_X2APIC_MASK != 0
        };
        if is_x2apic_supported {
            unsafe {
                cpu::wrmsr(
                    Self::MSR_INDEX,
                    local_apic_msr | Self::X2APIC_ENABLED_MASK | Self::XAPIC_ENABLED_MASK,
                );
            }
            self.is_x2apic_enabled = true;
        } else {
            match get_kernel_manager_cluster()
                .memory_manager
                .lock()
                .unwrap()
                .mmap_dev(base_address, 0x1000.into(), MemoryPermissionFlags::data())
            {
                Ok(address) => {
                    self.base_address = address;
                }
                Err(e) => {
                    pr_err!("Cannot reserve memory of Local APIC Err:{:?}", e);
                    return false;
                }
            };
            unsafe {
                cpu::wrmsr(Self::MSR_INDEX, local_apic_msr | Self::XAPIC_ENABLED_MASK);
            }
        }
        self.apic_id = self.read_apic_register(LocalApicRegisters::ApicId);
        self.write_apic_register(
            LocalApicRegisters::SIR,
            self.read_apic_register(LocalApicRegisters::SIR) | 0x100,
        );
        pr_info!(
            "APIC ID:{}(x2APIC:{})",
            self.apic_id,
            self.is_x2apic_enabled
        );
        return true;
    }

    pub fn get_apic_id(&self) -> u32 {
        self.apic_id
    }

    /// Send end of interruption to Local APIC.
    pub fn send_eoi(&self) {
        self.write_apic_register(LocalApicRegisters::EOI, 0);
    }

    /// Read Local APIC registers.
    ///
    /// If x2APIC is enabled, this function will read MSR, otherwise it will read mapped memory area.
    pub fn read_apic_register(&self, index: LocalApicRegisters) -> u32 {
        if self.is_x2apic_enabled {
            unsafe { cpu::rdmsr(LocalApicManager::X2APIC_MSR_INDEX + (index as u32)) as u32 }
        } else {
            unsafe {
                core::ptr::read_volatile(
                    (self.base_address.to_usize() + (index as usize) * 0x10) as *const u32,
                )
            }
        }
    }

    /// Write Local APIC registers.
    ///
    /// If x2APIC is enabled, this function will write into MSR, otherwise it will write into mapped memory area.
    pub fn write_apic_register(&self, index: LocalApicRegisters, data: u32) {
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
                    (self.base_address.to_usize() + (index as usize) * 0x10) as *mut u32,
                    data,
                );
            }
        }
    }
}
