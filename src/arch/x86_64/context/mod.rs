/*
 * Context Manager
 * This manager is the backend of task management system.
 * This manager treats arch-specific processes.
 */

pub mod context_data;

pub struct ContextManager {}

impl ContextManager {
    pub const fn new() -> Self {
        Self {}
    }

    pub fn init(&mut self) {}
}
