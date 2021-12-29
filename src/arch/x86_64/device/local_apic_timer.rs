//!
//! Local APIC Timer Manager
//!
//! Local APIC Timer is 32bit programmable timer and it is available when Local APIC is enabled.
//!
//! It has four registers: divide configuration, initial count, current count, LVT timer.
//!
//! Recent Local APIC Timer has TSC-Deadline mode that is the mode interrupt once
//! when current count register is zero. In the mode,Current count will decrease based on TSC
//! which is invariant.
//! Except TSC-Deadline mode, we must check frequency of it by PIT or ACPI PM Timer.    

use crate::arch::target_arch::device::cpu::{cpuid, rdmsr, rdtsc, wrmsr};
use crate::arch::target_arch::device::local_apic::{LocalApicManager, LocalApicRegisters};
use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::timer_manager::{GlobalTimerManager, Timer};

/// LocalApicTimer
///
/// LocalApicTimer has SpinLockFlag inner.
/// This implements Timer traits, but because of TSC-Deadline mode, current count may be zero.
/// Therefore, it is impossible to sync other timers with this timer.
pub struct LocalApicTimer {
    is_deadline_mode_enabled: bool,
    frequency: usize,
    reload_value: u64,
    is_interrupt_enabled: bool,
}

impl LocalApicTimer {
    const TSC_DEADLINE_MSR: u32 = 0x6E0;

    /// Create IoApicManager with invalid address.
    ///
    /// Before use, **you must call [`Self::init`]**.
    pub const fn new() -> Self {
        Self {
            is_deadline_mode_enabled: false,
            frequency: 0,
            reload_value: 0,
            is_interrupt_enabled: false,
        }
    }

    /// Init this manager.
    ///
    /// At this time, it does nothing.
    pub fn init(&mut self) {}

    /// Check if the machine supports TSC-Deadline mode.
    ///
    /// This function calls cpuid, avoid calling this many times.
    /// If TSC-Deadline mode is supported, it will not be enabled unless TSC is constant.
    pub fn is_deadline_mode_supported(&self) -> bool {
        let mut eax = 1u32;
        let mut ebx = 0u32;
        let mut ecx = 0u32;
        let mut edx = 0u32;
        unsafe { cpuid(&mut eax, &mut ebx, &mut ecx, &mut edx) };
        ecx & (1 << 24) != 0
    }

    /// Operate local apic timer interruption process.
    ///
    /// This function is called when the interrupt occurred.
    /// Currently, this function sends end of interrupt and switches to next thread.
    #[inline(never)]
    pub extern "C" fn local_apic_timer_handler() {
        loop {
            if get_cpu_manager_cluster().cpu_id
                == get_kernel_manager_cluster().boot_strap_cpu_manager.cpu_id
            {
                /* Temporary */
                get_kernel_manager_cluster()
                    .global_timer_manager
                    .global_timer_handler();
            }

            get_cpu_manager_cluster()
                .local_timer_manager
                .local_timer_handler();

            if !get_cpu_manager_cluster()
                .arch_depend_data
                .local_apic_timer
                .update_deadline_and_compare_with_current_tsc(GlobalTimerManager::TIMER_INTERVAL_MS)
            {
                break;
            }
        }
        get_cpu_manager_cluster()
            .arch_depend_data
            .local_apic_timer
            .write_deadline();
        get_cpu_manager_cluster().interrupt_manager.send_eoi();
    }

    fn calculate_next_reload_value(&self, ms: u64) -> (u64, bool) {
        self.reload_value
            .overflowing_add((self.frequency as u64 / 1000) * ms as u64)
    }

    /// Reset timer deadline for next interrupt
    ///
    /// This function is called from interrupt handler.
    /// Set [`Self::reload_value`] += TIMER_INTERVAL
    /// This function will return false
    /// when [`Self::is_deadline_mode_enabled`] is true and
    /// reset reload_value is bigger than current tsc.
    fn update_deadline_and_compare_with_current_tsc(&mut self, ms: u64) -> bool {
        if self.is_deadline_mode_enabled {
            let irq = InterruptManager::save_and_disable_local_irq();
            let (reload_value, overflowed) = self.calculate_next_reload_value(ms);
            let old_value = self.reload_value;
            self.reload_value = reload_value;
            InterruptManager::restore_local_irq(irq);
            let current = unsafe { rdtsc() };
            if overflowed {
                (old_value > current) && (self.reload_value <= current)
            } else {
                self.reload_value <= current
            }
        } else {
            false
        }
    }

    /// Set [`Self::reload_value`] to TSC_DEADLINE_MSR.
    ///
    /// Check if TSC-Deadline mode is enabled, and set new deadline(millisecond).
    /// If the mode is not enabled, this will return false.
    fn write_deadline(&self) -> bool {
        if !self.is_deadline_mode_enabled || self.frequency == 0 {
            return false;
        }
        unsafe { wrmsr(Self::TSC_DEADLINE_MSR, self.reload_value) };
        return true;
    }

    /// Enable TSC-Deadline mode.
    ///
    /// To enable it, this will check three things: is TSC-Deadline mode supported?,
    /// is it able to get frequency of TSC, and if it is invariant.
    /// After that, this function set the register to enable it.
    /// If it is enabled, the current value will be zero permanently.
    pub fn enable_deadline_mode(
        &mut self,
        vector: u16,
        local_apic_manager: &LocalApicManager,
    ) -> bool {
        if !self.is_deadline_mode_supported() {
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
            pr_warn!("TSC is not invariant.");
            return false;
        }

        let irq = InterruptManager::save_and_disable_local_irq();

        self.frequency = ((unsafe { rdmsr(0xce) as usize } >> 8) & 0xff) * 100 * 1000000;
        /* Frequency = MSRS(0xCE)[15:8] * 100MHz
         * 2.12 MSRS IN THE 3RD GENERATION INTEL(R) CORE(TM) PROCESSOR FAMILY
         * (BASED ON INTEL® MICROARCHITECTURE CODE NAME IVY BRIDGE) Intel SDM Vol.4 2-198 */
        /* This may be different from real frequency. */
        if self.frequency == 0 {
            InterruptManager::restore_local_irq(irq);
            pr_warn!("Cannot get the frequency of TSC.");
            return false;
        }

        local_apic_manager.write_apic_register(
            LocalApicRegisters::LvtTimer,
            (0b101 << 16) | (vector as u32),
        );
        self.is_deadline_mode_enabled = true;
        InterruptManager::restore_local_irq(irq);
        return true;
    }

    /// Set up interruption of timer.
    ///
    /// This function calculates the frequency of timer by using other timer.
    /// If interruption is already set up , this will return false.
    /// **This takes over 50ms for calculation.**
    ///
    ///  * vector: the index of IDT vector table to set the timer
    ///  * local_apic: LocalApicManager to read/write Local APIC.
    ///  * timer: the struct satisfied Timer trait. It must supply busy_wait_ms.
    ///
    /// This does not set up Interrupt Manager, you must set manually.
    /// After that, to start the interruption, [`Self::start_interrupt`].
    pub fn set_up_interrupt<T: Timer>(
        &mut self,
        vector: u16,
        local_apic: &LocalApicManager,
        timer: &T,
    ) -> bool {
        if self.frequency != 0 {
            return false;
        }
        let irq = InterruptManager::save_and_disable_local_irq();

        local_apic.write_apic_register(LocalApicRegisters::TimerDivide, 0b1011);
        local_apic.write_apic_register(LocalApicRegisters::LvtTimer, (0b001 << 16) | vector as u32); /*Masked*/
        self.reload_value = u32::MAX as u64;
        local_apic.write_apic_register(LocalApicRegisters::TimerInitialCount, u32::MAX);
        timer.busy_wait_ms(50);
        let end = local_apic.read_apic_register(LocalApicRegisters::TimerCurrentCount);
        let difference = self.get_difference(u32::MAX as usize, end as usize);
        self.frequency = difference * 20;
        InterruptManager::restore_local_irq(irq);
        return true;
    }

    /// Set the register to start interruption.
    ///
    /// Before calling it, ensure the interruption is set up.
    /// Currently, this function set 1000ms as the interval, in the future, it will be variable.
    pub fn start_interrupt(&mut self, local_apic: &LocalApicManager) -> bool {
        let irq = InterruptManager::save_and_disable_local_irq();
        if self.is_interrupt_enabled || self.frequency == 0 {
            InterruptManager::restore_local_irq(irq);
            return false;
        }

        if self.is_deadline_mode_enabled {
            let mut lvt = local_apic.read_apic_register(LocalApicRegisters::LvtTimer);
            lvt &= !(0b1 << 16);
            local_apic.write_apic_register(LocalApicRegisters::LvtTimer, lvt);
            self.reload_value = unsafe { rdtsc() };
            self.reload_value = self
                .calculate_next_reload_value(GlobalTimerManager::TIMER_INTERVAL_MS)
                .0;
            self.write_deadline();
        } else {
            let mut lvt = local_apic.read_apic_register(LocalApicRegisters::LvtTimer);
            lvt &= !(0b111 << 16);
            lvt |= 0b01 << 17;
            local_apic.write_apic_register(LocalApicRegisters::LvtTimer, lvt);
            self.set_interval(GlobalTimerManager::TIMER_INTERVAL_MS, local_apic);
        }
        self.is_interrupt_enabled = true;
        InterruptManager::restore_local_irq(irq);
        return true;
    }

    /// Return interrupt status.
    pub const fn is_interrupt_enabled(&self) -> bool {
        self.is_interrupt_enabled
    }

    /// Set reload value for interval mode.
    ///
    /// Set [`Self::reload_value`] and set into Local APIC Register.
    /// If TSC-Deadline mode is enabled, this will do nothing.
    /// This function assumes that [`Self::lock`] is locked.
    fn set_interval(&mut self, interval_ms: u64, local_apic: &LocalApicManager) -> bool {
        if self.is_deadline_mode_enabled || self.frequency == 0 {
            return false;
        }
        self.reload_value = (self.frequency / 1000) as u64 * interval_ms;
        local_apic.write_apic_register(
            LocalApicRegisters::TimerInitialCount,
            self.reload_value as u32,
        );
        return true;
    }
}

impl Timer for LocalApicTimer {
    fn get_count(&self) -> usize {
        if self.is_deadline_mode_enabled {
            unsafe { rdtsc() as usize }
        } else {
            get_cpu_manager_cluster()
                .interrupt_manager
                .get_local_apic_manager()
                .read_apic_register(LocalApicRegisters::TimerCurrentCount) as usize
        }
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
        u32::MAX as usize
    }
}
