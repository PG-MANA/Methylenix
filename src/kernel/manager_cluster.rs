/*
 * Cluster of Managers for kernel
 *
 * This cluster stores necessary structs for kernel.
 * All members of manager must be Mutex.
 */

use arch::target_arch::device::serial_port::SerialPortManager;
use arch::target_arch::interrupt::InterruptManager;

use kernel::drivers::efi::EfiManager;
use kernel::graphic::GraphicManager;
use kernel::memory_manager::kernel_malloc_manager::KernelMemoryAllocManager;
use kernel::memory_manager::{MemoryManager, SystemMemoryManager};
use kernel::task_manager::TaskManager;

use kernel::sync::spin_lock::Mutex;

use core::mem::MaybeUninit;

pub static mut STATIC_KERNEL_MANAGER_CLUSTER: MaybeUninit<KernelManagerCluster> =
    MaybeUninit::uninit();

pub struct KernelManagerCluster {
    pub graphic_manager: Mutex<GraphicManager>,
    pub memory_manager: Mutex<MemoryManager>,
    pub system_memory_manager: SystemMemoryManager,
    pub kernel_memory_alloc_manager: Mutex<KernelMemoryAllocManager>,
    pub interrupt_manager: Mutex<InterruptManager>,
    pub efi_manager: Mutex<EfiManager>,
    pub serial_port_manager: SerialPortManager,
    pub task_manager: TaskManager,
    /*SerialPortManager has mutex process inner*/
    //input_manager:
}

#[inline(always)]
pub fn get_kernel_manager_cluster() -> &'static mut KernelManagerCluster {
    /* You must assign new struct before use the structs!! */
    unsafe { STATIC_KERNEL_MANAGER_CLUSTER.get_mut() }
}
