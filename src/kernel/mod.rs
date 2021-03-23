//!
//! Kernel Modules
//!
//! These modules not depending on Arch

#[macro_use]
pub mod tty;
#[macro_use]
pub mod collections {
    pub mod fifo;
    #[macro_use]
    pub mod ptr_linked_list;
}
pub mod drivers;
pub mod graphic_manager;
pub mod manager_cluster;
pub mod memory_manager;
pub mod panic;
pub mod sync;
pub mod task_manager;
pub mod timer_manager;
