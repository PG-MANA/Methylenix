//!
//! Kernel Modules
//!
//! These modules not depending on Arch

#[macro_use]
pub mod tty;
pub mod application_loader;
pub mod block_device;
pub mod collections;
pub mod drivers;
pub mod file_manager;
pub mod graphic_manager;
pub mod initialization;
pub mod manager_cluster;
pub mod memory_manager;
pub mod network_manager;
pub mod panic;

pub mod sync {
    pub mod rwlock;
    pub mod spin_lock;
}

pub mod system_call;
pub mod task_manager;
pub mod timer_manager;
