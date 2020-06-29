/*
 * Timer Manager
 * This manager is the frontend of timer system.
 * Arch-specific timer call this function to process timer queue
 * when task-switch, once return to arch-specific timer function
 *  for process of sending end of interrupt and recall this manager
 */

pub struct TimerManager {
    resolution: usize,
    counter: usize,
    ms_counter: usize,
}

impl TimerManager {
    pub const fn new() -> Self {
        Self {
            resolution: 0,
            counter: 0,
            ms_counter: 0,
        }
    }

    pub fn init(&mut self, resolution: usize) {
        self.resolution = resolution;
    }
}
