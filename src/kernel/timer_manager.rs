/*
 * Timer Manager
 * This manager is the frontend of timer system.
 * Arch-specific timer call this function to process timer queue
 * when task-switch, once return to arch-specific timer function
 *  for process of sending end of interrupt and recall this manager
 * The member of this manager may be changed
 */

pub struct TimerManager {}

pub trait Timer {
    fn get_count(&self) -> usize;
    fn get_frequency_hz(&self) -> usize;
    fn is_count_up_timer(&self) -> bool;
    fn get_difference(&self, earlier: usize, later: usize) -> usize;
    fn get_ending_count_value(&self, start: usize, difference: usize) -> usize;
    fn get_max_counter_value(&self) -> usize;
    /*fn get_interval_ms(&self) -> usize;*/
    #[inline(always)]
    fn busy_wait_ms(&self, ms: usize) {
        let start = self.get_count();
        let difference = self.get_frequency_hz() * ms / 1000;
        if difference > self.get_max_counter_value() {
            panic!("Cannot count more than max_counter_value");
        }

        let end = self.get_ending_count_value(start, difference);
        if self.is_count_up_timer() {
            while self.get_count() < end {}
        } else {
            while self.get_count() > end {}
        }
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
        if self.is_count_up_timer() {
            while self.get_count() < end {}
        } else {
            while self.get_count() > end {}
        }
    }
}
