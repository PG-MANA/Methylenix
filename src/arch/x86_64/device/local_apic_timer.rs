/*
 * Local APIC Timer
 */

use arch::target_arch::device::cpu::{cpuid, wrmsr};
use arch::target_arch::device::local_apic::{LocalApicManager, LocalApicRegisters};

use arch::x86_64::device::cpu::rdmsr;
use kernel::manager_cluster::get_kernel_manager_cluster;

pub struct LocalApicTimer {
    is_deadline_mode_supported: bool,
    tsc_frequency: usize,
    is_interrupt_enabled: bool,
}

impl LocalApicTimer {
    pub const fn new() -> Self {
        Self {
            is_deadline_mode_supported: false,
            tsc_frequency: 0,
            is_interrupt_enabled: false,
        }
    }

    pub fn init(&mut self, local_apic_manager: &mut LocalApicManager) {
        let mut eax = 1u32;
        let mut ebx = 0u32;
        let mut ecx = 0u32;
        let mut edx = 0u32;
        unsafe { cpuid(&mut eax, &mut ebx, &mut ecx, &mut edx) };
        self.is_deadline_mode_supported = ecx & (1 << 24) != 0;
        if self.enable_deadline_mode(local_apic_manager) {
            pr_info!("TimeStampCounter's frequency: {:#X}", self.tsc_frequency);
        } else {
            pr_info!("APIC deadline timer is not enabled.");
        }
    }

    pub fn inthandler30_main() {
        if let Ok(im) = get_kernel_manager_cluster().interrupt_manager.try_lock() {
            im.send_eoi();
        }
        pr_info!("Deadline mode test!");
    }

    pub fn enable_deadline_mode(&mut self, local_apic_manager: &mut LocalApicManager) -> bool {
        if self.is_deadline_mode_supported == false {
            return false;
        }
        self.tsc_frequency = ((unsafe { rdmsr(0xce) as usize } >> 8) & 0xff) * 1000000;
        /* 2.12 MSRS IN THE 3RD GENERATION INTEL(R) CORE(TM) PROCESSOR FAMILY
         * (BASED ON INTEL® MICROARCHITECTURE CODE NAME IVY BRIDGE) Intel SDM Vol.4 2-189 */
        if self.tsc_frequency == 0 {
            return false;
        }
        let is_invariant_tsc = unsafe {
            let mut eax = 0x80000007u32;
            let mut ebx = 0;
            let mut edx = 0;
            let mut ecx = 0;
            cpuid(&mut eax, &mut ebx, &mut ecx, &mut edx);
            (edx & (1 << 8)) != 0
        };
        if !is_invariant_tsc {
            self.is_deadline_mode_supported = false;
            pr_warn!("TSC is not invariant, TSC deadline timer was disabled.");
            return false;
        }

        let lvt_timer_status = (0b100 << 16) | (0x30/*Vector*/);
        /* [18:17:16] <- 0b100 */
        local_apic_manager.write_apic_register(LocalApicRegisters::Timer, lvt_timer_status);
        unsafe { wrmsr(0x6e0, 0xA0991F43F) };
        self.is_interrupt_enabled = true;
        true
    }

    pub const fn is_interrupt_enabled(&self) -> bool {
        self.is_interrupt_enabled
    }

    pub fn set_deadline(&self, ms: usize) -> bool {
        if self.is_deadline_mode_supported && self.tsc_frequency != 0 {
            use core::sync::atomic;
            atomic::fence(atomic::Ordering::Acquire);
            unsafe {
                let deadline = rdmsr(0x10) as usize + (self.tsc_frequency / 1000) * ms;
                wrmsr(0x6e0, deadline as u64);
            }
            atomic::fence(atomic::Ordering::Release);
            true
        } else {
            false
        }
    }

    pub unsafe fn set_deadline_without_checking(deadline: u64) {
        wrmsr(0x6e0, deadline)
    }
}
