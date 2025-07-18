//!
//! Arm Generic Timer
//!

use crate::arch::target_arch::device::cpu;
use crate::arch::target_arch::interrupt::InterruptGroup;

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress, VAddress,
};
use crate::kernel::memory_manager::io_remap;
use crate::kernel::timer_manager::{GlobalTimerManager, Timer};

const SYSTEM_COUNTER_MEMORY_SIZE: MSize = MSize::new(0x1000);

pub struct GenericTimer {
    is_non_secure_timer: bool,
    is_level_trigger: bool,
    interrupt_id: u32,
    frequency: u32,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum SystemCounterBaseAddressType {
    Invalid,
    CntCtlBase,
}

pub struct SystemCounter {
    base_address: VAddress,
    current_frequency: u32,
    current_frequency_model: u16,
    base_address_type: SystemCounterBaseAddressType,
}

impl Default for SystemCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemCounter {
    const CNTCR: usize = 0x00;
    const CNTCR_EN: u64 = 0x01;

    const CNTFID0: usize = 0x20;

    pub fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
            current_frequency: 0,
            current_frequency_model: 0,
            base_address_type: SystemCounterBaseAddressType::Invalid,
        }
    }

    pub fn init_cnt_ctl_base(&mut self, base_address: PAddress) -> Result<(), ()> {
        self.base_address = match io_remap!(
            base_address,
            SYSTEM_COUNTER_MEMORY_SIZE,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(v) => v,
            Err(e) => {
                pr_err!("Failed to map System Counter: {:?}", e);
                return Err(());
            }
        };
        self.base_address_type = SystemCounterBaseAddressType::CntCtlBase;
        self.current_frequency_model = 0;
        unsafe { *((self.base_address.to_usize() + Self::CNTCR) as *mut u64) = Self::CNTCR_EN };

        self.current_frequency = unsafe {
            *((self.base_address.to_usize()
                + Self::CNTFID0
                + (self.current_frequency_model as usize) * size_of::<u32>())
                as *const u32)
        };
        if self.current_frequency == u32::MAX {
            self.current_frequency = 0;
        }
        Ok(())
    }

    pub fn get_current_frequency(&self) -> usize {
        if self.base_address_type == SystemCounterBaseAddressType::Invalid
            || self.current_frequency == 0
        {
            cpu::get_cntfrq() as usize
        } else {
            self.current_frequency as usize
        }
    }
}

impl GenericTimer {
    const CNTP_CTL_EL0_ENABLE: u64 = 0x01;

    const TIMER_PRIORITY: u8 = 0x00;

    pub const fn new() -> Self {
        Self {
            is_non_secure_timer: false,
            is_level_trigger: false,
            interrupt_id: 0,
            frequency: 0,
        }
    }

    /// Init manager and setup interrupt
    ///
    /// This function does not enable interrupt, only setup to be ready.
    pub fn init(
        &mut self,
        is_non_secure_timer: bool,
        is_level_trigger: bool,
        interrupt_id: u32,
        frequency: Option<u32>,
    ) {
        self.is_non_secure_timer = is_non_secure_timer;
        self.is_level_trigger = is_level_trigger;
        self.interrupt_id = interrupt_id;
        self.frequency = frequency.unwrap_or(0);
        pr_debug!("Generic Timer Interrupt ID: {interrupt_id}");
        get_cpu_manager_cluster()
            .interrupt_manager
            .set_device_interrupt_function(
                Self::interrupt_handler,
                interrupt_id,
                Self::TIMER_PRIORITY,
                if self.is_non_secure_timer {
                    Some(InterruptGroup::NonSecureEl1)
                } else {
                    unimplemented!()
                },
                is_level_trigger,
            )
            .expect("Failed to setup interrupt");
    }

    pub fn init_ap(&mut self, original: &Self) {
        self.init(
            original.is_non_secure_timer,
            original.is_level_trigger,
            original.interrupt_id,
            Some(original.frequency),
        );
    }

    pub fn start_interrupt(&self) {
        if self.is_non_secure_timer {
            self.reload_timeout_value();
            unsafe { cpu::set_cntp_ctl(Self::CNTP_CTL_EL0_ENABLE) };
        } else {
            unimplemented!()
        }
    }

    pub fn reload_timeout_value(&self) {
        let reset_value =
            (GlobalTimerManager::TIMER_INTERVAL_MS * self.get_frequency_hz() as u64) / 1000;
        assert!(reset_value <= i32::MAX as u64);
        if self.is_non_secure_timer {
            unsafe { cpu::set_cntp_tval(reset_value) };
        }
    }

    fn interrupt_handler(_interrupt_id: usize) -> bool {
        let generic_timer = &mut get_cpu_manager_cluster().arch_depend_data.generic_timer;
        get_cpu_manager_cluster()
            .local_timer_manager
            .local_timer_handler();
        if get_kernel_manager_cluster().boot_strap_cpu_manager.cpu_id
            == get_cpu_manager_cluster().cpu_id
        {
            get_kernel_manager_cluster()
                .global_timer_manager
                .global_timer_handler();
        }
        generic_timer.reload_timeout_value();
        true
    }
}

impl Timer for GenericTimer {
    fn get_count(&self) -> usize {
        cpu::get_cntpct() as usize
    }

    fn get_frequency_hz(&self) -> usize {
        if self.frequency != 0 {
            self.frequency as usize
        } else {
            get_kernel_manager_cluster()
                .arch_depend_data
                .system_counter
                .get_current_frequency()
        }
    }

    fn is_count_up_timer(&self) -> bool {
        true
    }

    fn get_difference(&self, earlier: usize, later: usize) -> usize {
        if earlier <= later {
            earlier + (self.get_max_counter_value() - later)
        } else {
            earlier - later
        }
    }

    fn get_ending_count_value(&self, _start: usize, _difference: usize) -> usize {
        unimplemented!()
    }

    fn get_max_counter_value(&self) -> usize {
        u64::MAX as usize
    }
}
