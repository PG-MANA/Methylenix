/*
 * Local APIC Timer
 */

use arch::target_arch::device::cpu::{cpuid, rdmsr, wrmsr};
use arch::target_arch::device::local_apic::{LocalApicManager, LocalApicRegisters};

use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::sync::spin_lock::SpinLockFlag;
use kernel::timer_manager::Timer;

use core::sync::atomic::{fence, Ordering};

pub struct LocalApicTimer {
    lock: SpinLockFlag,
    is_deadline_mode_enabled: bool,
    frequency: usize,
    reload_value: usize,
    is_interrupt_setup: bool,
}

impl LocalApicTimer {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            is_deadline_mode_enabled: false,
            frequency: 0,
            reload_value: 0,
            is_interrupt_setup: false,
        }
    }

    pub fn init(&mut self) {}

    pub fn is_deadline_mode_supported(&self) -> bool {
        let mut eax = 1u32;
        let mut ebx = 0u32;
        let mut ecx = 0u32;
        let mut edx = 0u32;
        unsafe { cpuid(&mut eax, &mut ebx, &mut ecx, &mut edx) };
        ecx & (1 << 24) != 0
    }

    pub fn local_apic_timer_handler() {
        if let Ok(im) = get_kernel_manager_cluster().interrupt_manager.try_lock() {
            im.send_eoi();
        }
        get_kernel_manager_cluster()
            .task_manager
            .switch_to_next_thread();
    }

    pub fn enable_deadline_mode(
        &mut self,
        vector: u16,
        local_apic_manager: &LocalApicManager,
    ) -> bool {
        if self.is_deadline_mode_supported() == false {
            return false;
        }
        let _lock = self.lock.lock();
        self.frequency = ((unsafe { rdmsr(0xce) as usize } >> 8) & 0xff) * 1000000;
        /* 2.12 MSRS IN THE 3RD GENERATION INTEL(R) CORE(TM) PROCESSOR FAMILY
         * (BASED ON INTEL® MICROARCHITECTURE CODE NAME IVY BRIDGE) Intel SDM Vol.4 2-189 */
        if self.frequency == 0 {
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
            pr_warn!("TSC is not invariant, TSC deadline timer was disabled.");
            return false;
        }

        let lvt_timer_status = (0b101 << 16) | (vector as u32); /* Masked */
        /* [18:17:16] <- 0b100 */
        local_apic_manager.write_apic_register(LocalApicRegisters::LvtTimer, lvt_timer_status);
        unsafe { wrmsr(0x6e0, 0xA0991F43F) };
        self.is_interrupt_setup = true;
        self.is_deadline_mode_enabled = true;
        true
    }

    pub fn set_up_interrupt<T: Timer>(
        &mut self,
        vector: u16,
        local_apic: &LocalApicManager,
        timer: &T, /**/
    ) -> bool {
        use core::u32;
        let _lock = self.lock.lock();
        if self.is_interrupt_setup {
            return false;
        }

        local_apic.write_apic_register(LocalApicRegisters::TimerDivide, 0b1011);
        local_apic.write_apic_register(LocalApicRegisters::LvtTimer, (0b001 << 16) | vector as u32); /*Masked*/
        self.reload_value = u32::MAX as usize;
        local_apic.write_apic_register(LocalApicRegisters::TimerInitialCount, u32::MAX);
        timer.busy_wait_ms(50);
        let end = local_apic.read_apic_register(LocalApicRegisters::TimerCurrentCount);
        let difference = self.get_difference(u32::MAX as usize, end as usize);
        self.frequency = difference * 20;
        self.is_interrupt_setup = true;
        return true;
    }

    pub fn start_interrupt(&mut self, local_apic: &LocalApicManager) -> bool {
        let _lock = self.lock.lock();
        if self.is_interrupt_setup == false {
            return false;
        }
        if self.is_deadline_mode_enabled {
            let mut lvt = local_apic.read_apic_register(LocalApicRegisters::LvtTimer);
            lvt &= !(0b1 << 16);
            local_apic.write_apic_register(LocalApicRegisters::LvtTimer, lvt);
            self.set_deadline(10);
        } else {
            let mut lvt = local_apic.read_apic_register(LocalApicRegisters::LvtTimer);
            lvt &= !(0b111 << 16);
            lvt |= 0b01 << 17;
            local_apic.write_apic_register(LocalApicRegisters::LvtTimer, lvt);
            local_apic.write_apic_register(
                LocalApicRegisters::TimerInitialCount,
                (self.frequency * 1) as u32, //Testing
            );
            self.reload_value = self.frequency * 1;
        }
        true
    }

    pub const fn is_interrupt_enabled(&self) -> bool {
        self.is_interrupt_setup
    }

    pub fn set_deadline(&self, ms: usize) -> bool {
        if self.is_deadline_mode_enabled && self.frequency != 0 {
            fence(Ordering::Acquire);
            unsafe {
                let deadline = rdmsr(0x10) as usize + (self.frequency / 1000) * ms;
                wrmsr(0x6e0, deadline as u64);
            }
            fence(Ordering::Release);
            true
        } else {
            false
        }
    }

    pub unsafe fn set_deadline_without_checking(deadline: u64) {
        wrmsr(0x6e0, deadline)
    }
}

impl Timer for LocalApicTimer {
    fn get_count(&self) -> usize {
        unimplemented!();
    }

    fn get_frequency_hz(&self) -> usize {
        self.frequency
    }

    fn is_count_up_timer(&self) -> bool {
        true
    }

    fn get_difference(&self, earlier: usize, later: usize) -> usize {
        assert_eq!(self.is_deadline_mode_enabled, false);
        if earlier <= later {
            earlier + (self.reload_value as usize - later)
        } else {
            earlier - later
        }
    }

    fn get_ending_count_value(&self, _start: usize, _difference: usize) -> usize {
        unimplemented!()
    }

    fn get_max_counter_value(&self) -> usize {
        use core::u32;
        u32::MAX as usize
    }
}
