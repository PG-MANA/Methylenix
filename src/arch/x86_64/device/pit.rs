/*
 * Programmable Interval Timer 8254
 */

use arch::target_arch::device::cpu;

use kernel::manager_cluster::get_kernel_manager_cluster;
use kernel::sync::spin_lock::SpinLockFlag;
use kernel::timer_manager::Timer;

pub struct PitManager {
    lock: SpinLockFlag,
    reload_value: u16,
}

impl PitManager {
    pub const fn new() -> Self {
        Self {
            lock: SpinLockFlag::new(),
            reload_value: 0,
        }
    }

    pub fn init(&mut self) {
        unsafe {
            cpu::out_byte(0x43, 0);
            cpu::out_byte(0x40, 0xff);
            cpu::out_byte(0x40, 0xff);
        }
        self.reload_value = 0xffff;
    }

    pub fn set_up_interrupt(&mut self) {
        unsafe {
            self.reload_value = 11932u16;
            cpu::out_byte(0x43, 0x34);
            cpu::out_byte(0x40, (self.reload_value & 0xff) as u8);
            cpu::out_byte(0x40, (self.reload_value >> 8) as u8);
        }
        /*make_interrupt_hundler!(inthandler20, PitManager::inthandler20_main);
        let mut interrupt_manager = get_kernel_manager_cluster()
            .interrupt_manager
            .lock()
            .unwrap();
        interrupt_manager.set_device_interrupt_function(
            inthandler20,
            Some(0),
            0x20,
            0,
        );*/
    }

    pub fn inthandler20_main() {
        if let Ok(im) = get_kernel_manager_cluster().interrupt_manager.try_lock() {
            im.send_eoi();
        }
    }
}

impl Timer for PitManager {
    #[inline(always)]
    fn get_count(&self) -> usize {
        /*let _lock = self.lock.lock();*/
        unsafe { cpu::out_byte(0x43, 0) };
        let (r1, r2) = unsafe { cpu::in_byte_twice(0x40) };
        ((r2 as usize) << 8) | r1 as usize
    }

    fn get_frequency_hz(&self) -> usize {
        1193182
    }

    fn is_count_up_timer(&self) -> bool {
        false
    }

    fn get_difference(
        &self,
        earlier: usize, /*earlier*/
        later: usize,   /*later*/
    ) -> usize {
        /*assume that counter is not rotated more than once.*/
        if earlier <= later {
            earlier + (self.reload_value as usize - later)
        } else {
            earlier - later
        }
    }

    fn get_ending_count_value(&self, start: usize, difference: usize) -> usize {
        if start > difference {
            start - difference
        } else {
            self.reload_value as usize - (difference - start)
        }
    }

    fn get_max_counter_value(&self) -> usize {
        self.reload_value as usize
    }
}
