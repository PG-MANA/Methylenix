//!
//! Timer Manager
//!
//! This manager is the frontend of timer system.
//! Arch-specific timer call this manager to process timer queue.
//! When task-switch, this will return to arch-specific timer function once
//! for processes like sending end of interrupt.
//! After that, the timer should recall this manager.
//! The member of this manager may be changed.

use crate::arch::target_arch::device::cpu::is_interrupt_enabled;
use crate::arch::target_arch::interrupt::InterruptManager;

use crate::kernel::collections::ptr_linked_list::{
    offset_of_list_node, PtrLinkedList, PtrLinkedListNode,
};
use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::slab_allocator::LocalSlabAllocator;
use crate::kernel::task_manager::work_queue::WorkList;

#[cfg(not(target_has_atomic = "64"))]
use crate::kernel::sync::spin_lock::SequenceSpinLock;
#[cfg(target_has_atomic = "64")]
use core::sync::atomic::AtomicU64;
use core::sync::atomic::{AtomicU8, Ordering};

pub struct GlobalTimerManager {
    #[cfg(target_has_atomic = "64")]
    tick: AtomicU64,
    #[cfg(not(target_has_atomic = "64"))]
    tick: u64,
    #[cfg(not(target_has_atomic = "64"))]
    tick_lock: SequenceSpinLock,
    //lock: IrqSaveSpinLockFlag,
}

pub struct TimerList {
    pub(super) t_list: PtrLinkedListNode<Self>,
    timeout: u64,
    function: fn(usize),
    data: usize,
    flags: AtomicU8,
}

pub struct LocalTimerManager {
    timer_list: PtrLinkedList<TimerList>,
    timer_list_pool: LocalSlabAllocator<TimerList>,
    last_processed_timeout: u64,
    source_timer: Option<&'static dyn Timer>,
}

const TIMER_LIST_FLAGS_WAITING: u8 = 0;
const TIMER_LIST_FLAGS_EXPIRED: u8 = 1 << 0;
const TIMER_LIST_FLAGS_RUNNING: u8 = 1 << 1;
const TIMER_LIST_FLAGS_FINISHED: u8 = 1 << 2;
const TIMER_LIST_FLAGS_CANCELED: u8 = 1 << 3;

impl GlobalTimerManager {
    pub const TIMER_INTERVAL_MS: u64 = 10;
    const TICK_INITIAL_VALUE: u64 = 0;

    pub fn new() -> Self {
        Self {
            #[cfg(target_has_atomic = "64")]
            tick: AtomicU64::new(Self::TICK_INITIAL_VALUE),
            #[cfg(not(target_has_atomic = "64"))]
            tick: Self::TICK_INITIAL_VALUE,
            #[cfg(not(target_has_atomic = "64"))]
            tick_lock: SequenceSpinLock::new(),
            //lock: IrqSaveSpinLockFlag::new(),
        }
    }

    const fn get_end_tick_ms(current: u64, ms: u64) -> (u64, bool) {
        current.overflowing_add(ms / Self::TIMER_INTERVAL_MS)
    }

    const fn calculate_timeout(current: u64, ms: u64) -> (u64, bool /* is_overflowed */) {
        Self::get_end_tick_ms(current, ms)
    }

    #[cfg(target_has_atomic = "64")]
    pub fn get_current_tick(&self) -> u64 {
        self.tick.load(Ordering::Relaxed)
    }

    #[cfg(not(target_has_atomic = "64"))]
    pub fn get_current_tick(&self) -> u64 {
        loop {
            let seq = self.tick_lock.read_start();
            let tick: u64 = self.tick;
            if self.tick_lock.should_read_retry(seq) {
                continue;
            }
            return tick;
        }
    }

    #[cfg(target_has_atomic = "64")]
    fn count_up_tick(&mut self) {
        self.tick.fetch_add(1, Ordering::SeqCst);
    }

    #[cfg(not(target_has_atomic = "64"))]
    fn count_up_tick(&mut self) {
        assert!(self.lock.is_locked());
        let lock = self.tick_lock.write_lock();
        self.tick += 1;
        self.tick_lock.write_unlock(lock);
    }

    pub fn get_difference_ms(&self, tick: u64) -> u64 {
        let current_tick = self.get_current_tick();
        let difference = if current_tick < tick {
            u64::MAX - tick + current_tick
        } else {
            current_tick - tick
        };
        difference * Self::TIMER_INTERVAL_MS
    }

    pub fn busy_wait_ms(&self, ms: u64) -> bool {
        let start_tick = self.get_current_tick(); /* get_quickly */
        if !is_interrupt_enabled() {
            pr_err!("Interrupt is disabled.");
            return false;
        }
        let (end_tick, overflowed) = Self::get_end_tick_ms(start_tick as u64, ms);
        if overflowed {
            while self.get_current_tick() >= start_tick {
                core::hint::spin_loop();
            }
        }

        while self.get_current_tick() <= end_tick {
            core::hint::spin_loop();
        }
        return true;
    }

    pub fn busy_wait_us(&self, us: u64) -> bool {
        let start_tick = self.get_current_tick(); /* get_quickly */
        if !is_interrupt_enabled() {
            pr_err!("Interrupt is disabled.");
            return false;
        }
        /* We cannot count higher than TIMER_INTERVAL_MS currently. */
        let ms = if (us / 1000) < Self::TIMER_INTERVAL_MS {
            Self::TIMER_INTERVAL_MS
        } else {
            us / 1000
        };
        let (end_tick, overflowed) = Self::get_end_tick_ms(start_tick, ms);
        if overflowed {
            while self.get_current_tick() >= start_tick {
                core::hint::spin_loop();
            }
        }

        while self.get_current_tick() <= end_tick {
            core::hint::spin_loop();
        }
        return true;
    }

    pub fn global_timer_handler(&mut self) {
        self.count_up_tick();
    }
}

impl LocalTimerManager {
    pub fn new() -> Self {
        Self {
            timer_list: PtrLinkedList::new(),
            timer_list_pool: LocalSlabAllocator::new(0),
            last_processed_timeout: GlobalTimerManager::TICK_INITIAL_VALUE,
            source_timer: None,
        }
    }

    pub fn add_timer(
        &mut self,
        wait_ms: u64,
        function: fn(usize),
        data: usize,
    ) -> Result<usize, ()> {
        let irq = InterruptManager::save_and_disable_local_irq();
        let current = get_kernel_manager_cluster()
            .global_timer_manager
            .get_current_tick();
        let list = match self.timer_list_pool.alloc() {
            Ok(a) => a,
            Err(e) => {
                InterruptManager::restore_local_irq(irq);
                pr_err!("Failed to allocate TimerList: {:?}", e);
                return Err(());
            }
        };
        let is_overflowed;
        list.data = data;
        list.function = function;
        (list.timeout, is_overflowed) = GlobalTimerManager::calculate_timeout(current, wait_ms);
        list.flags = AtomicU8::new(TIMER_LIST_FLAGS_WAITING);

        if let Some(e) = unsafe {
            self.timer_list
                .get_first_entry_mut(offset_of_list_node!(TimerList, t_list))
        } {
            let mut entry = e;
            loop {
                if !is_overflowed {
                    if entry.timeout >= current && entry.timeout > list.timeout {
                        self.timer_list
                            .insert_before(&mut entry.t_list, &mut list.t_list);
                        break;
                    }
                } else {
                    if entry.timeout <= current && entry.timeout > list.timeout {
                        self.timer_list
                            .insert_before(&mut entry.t_list, &mut list.t_list);
                        break;
                    }
                }
                if let Some(e) = unsafe {
                    entry
                        .t_list
                        .get_next_mut(offset_of_list_node!(TimerList, t_list))
                } {
                    entry = e;
                } else {
                    self.timer_list
                        .insert_after(&mut entry.t_list, &mut list.t_list);
                    break;
                }
            }
            InterruptManager::restore_local_irq(irq);
            Ok(0)
        } else {
            self.timer_list.insert_head(&mut list.t_list);
            InterruptManager::restore_local_irq(irq);
            Ok(0)
        }
    }

    pub fn local_timer_handler(&mut self) {
        let current_tick = get_kernel_manager_cluster()
            .global_timer_manager
            .get_current_tick();
        let is_overflowed = current_tick < self.last_processed_timeout;

        while let Some(t) = unsafe {
            self.timer_list
                .get_first_entry(offset_of_list_node!(TimerList, t_list))
        } {
            if (!is_overflowed
                && self.last_processed_timeout < t.timeout
                && t.timeout <= current_tick)
                || (is_overflowed
                    && (self.last_processed_timeout < t.timeout || t.timeout <= current_tick))
            {
                if let Err(e) = t.flags.compare_exchange(
                    TIMER_LIST_FLAGS_WAITING,
                    TIMER_LIST_FLAGS_EXPIRED,
                    Ordering::SeqCst,
                    Ordering::Relaxed,
                ) {
                    if e == TIMER_LIST_FLAGS_CANCELED {
                        self.timer_list_pool.free(unsafe {
                            self.timer_list
                                .take_first_entry(offset_of_list_node!(TimerList, t_list))
                                .unwrap()
                        });
                    } else {
                        pr_err!("Unexpected flag: {:#X}", e);
                        let _ = unsafe {
                            self.timer_list
                                .take_first_entry(offset_of_list_node!(TimerList, t_list))
                                .unwrap()
                        };
                    }
                    continue;
                }
                if let Err(e) = get_cpu_manager_cluster().work_queue.add_work(WorkList::new(
                    Self::expired_timer_list_worker,
                    t as *const _ as usize,
                )) {
                    pr_err!("Failed to add the work of timer_list: {:?}", e);
                    break;
                }
                unsafe {
                    self.timer_list
                        .take_first_entry(offset_of_list_node!(TimerList, t_list))
                        .unwrap()
                };
            } else {
                self.last_processed_timeout = current_tick;
                break;
            }
        }
        get_cpu_manager_cluster().run_queue.tick();
    }

    pub fn set_source_timer(&mut self, timer: &'static dyn Timer) {
        self.source_timer = Some(timer);
    }

    pub fn get_monotonic_clock_ns(&self) -> u64 {
        let nano_second_freq = 1u64.pow(9);
        if let Some(t) = self.source_timer {
            if t.is_count_up_timer() {
                let freq = t.get_frequency_hz() as u64;
                if freq > nano_second_freq {
                    return t.get_count() as u64;
                } else if freq > GlobalTimerManager::TIMER_INTERVAL_MS * 1000 {
                    return t.get_count() as u64 * (nano_second_freq / freq);
                }
            }
        }
        return get_kernel_manager_cluster()
            .global_timer_manager
            .get_current_tick()
            * (nano_second_freq / (GlobalTimerManager::TIMER_INTERVAL_MS * 1000));
    }

    fn expired_timer_list_worker(data: usize) {
        let entry = unsafe { &mut *(data as *mut TimerList) };
        if let Err(e) = entry.flags.compare_exchange(
            TIMER_LIST_FLAGS_EXPIRED,
            TIMER_LIST_FLAGS_RUNNING,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            if e != TIMER_LIST_FLAGS_CANCELED {
                pr_err!("Unexpected flag: {:#X}", e);
            }
            let irq = InterruptManager::save_and_disable_local_irq();
            get_cpu_manager_cluster()
                .local_timer_manager
                .timer_list_pool
                .free(entry);
            InterruptManager::restore_local_irq(irq);
            return;
        }
        (entry.function)(entry.data);
        entry
            .flags
            .store(TIMER_LIST_FLAGS_FINISHED, Ordering::Release);
        let irq = InterruptManager::save_and_disable_local_irq();
        get_cpu_manager_cluster()
            .local_timer_manager
            .timer_list_pool
            .free(entry);
        InterruptManager::restore_local_irq(irq);
    }
}

pub trait Timer {
    fn get_count(&self) -> usize;
    fn get_frequency_hz(&self) -> usize;
    fn is_count_up_timer(&self) -> bool;
    fn get_difference(&self, earlier: usize, later: usize) -> usize;
    fn get_ending_count_value(&self, start: usize, difference: usize) -> usize;
    fn get_max_counter_value(&self) -> usize;

    #[inline(always)]
    fn busy_wait_ms(&self, ms: usize) {
        let start = self.get_count();
        let difference = self.get_frequency_hz() * ms / 1000;
        if difference > self.get_max_counter_value() {
            panic!("Cannot count more than max_counter_value");
        }
        let end = self.get_ending_count_value(start, difference);
        self.wait_until(start, end);
    }

    #[inline(always)]
    fn busy_wait_us(&self, us: usize) {
        let start = self.get_count();
        let difference = self.get_frequency_hz() * us / 1000000;
        if difference > self.get_max_counter_value() {
            panic!("Cannot count more than max_counter_value");
        } else if difference == 0 {
            panic!("Cannot count less than the resolution");
        }
        let end = self.get_ending_count_value(start, difference);
        self.wait_until(start, end);
    }

    #[inline(always)]
    fn wait_until(&self, start_counter_value: usize, end_counter_value: usize) {
        use core::hint::spin_loop;
        if self.is_count_up_timer() {
            if start_counter_value > end_counter_value {
                while self.get_count() >= start_counter_value {
                    spin_loop();
                }
            }
            while self.get_count() < end_counter_value {
                spin_loop();
            }
        } else {
            if start_counter_value < end_counter_value {
                while self.get_count() <= start_counter_value {
                    spin_loop();
                }
            }
            while self.get_count() > end_counter_value {
                spin_loop();
            }
        }
    }
}
