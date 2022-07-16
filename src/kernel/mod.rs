//!
//! Kernel Modules
//!
//! These modules not depending on Arch

#[macro_use]
pub mod tty;
#[macro_use]
pub mod collections {
    #[macro_export]
    macro_rules! init_struct {
        ($st:expr, $v:expr) => {
            core::mem::forget(core::mem::replace(&mut $st, $v))
        };
    }
    pub mod auxiliary_vector;
    pub mod fifo;
    pub mod guid;
    #[macro_use]
    pub mod ptr_linked_list;
    pub mod ring_buffer;
}
pub mod application_loader;
pub mod block_device;
pub mod drivers;
pub mod file_manager;
pub mod graphic_manager;
pub mod manager_cluster;
pub mod memory_manager;
pub mod network_manager;
pub mod panic;
pub mod sync;
pub mod system_call;
pub mod task_manager;
pub mod timer_manager;
