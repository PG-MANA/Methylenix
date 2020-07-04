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
use kernel::memory_manager::MemoryManager;
use kernel::task_manager::TaskManager;

use kernel::sync::spin_lock::Mutex;

pub static mut STATIC_KERNEL_MANAGER_CLUSTER: KernelManagerCluster = init_manager_cluster();

pub struct KernelManagerCluster {
    pub graphic_manager: Mutex<GraphicManager>,
    pub memory_manager: Mutex<MemoryManager>,
    pub kernel_memory_alloc_manager: Mutex<KernelMemoryAllocManager>,
    pub interrupt_manager: Mutex<InterruptManager>,
    pub efi_manager: Mutex<EfiManager>,
    pub serial_port_manager: SerialPortManager,
    pub task_manager: TaskManager,
    /*SerialPortManager has mutex process inner*/
    //input_manager:
}

const fn init_manager_cluster() -> KernelManagerCluster {
    KernelManagerCluster {
        graphic_manager: Mutex::new(GraphicManager::new_static()),
        memory_manager: Mutex::new(MemoryManager::new_static()),
        kernel_memory_alloc_manager: Mutex::new(KernelMemoryAllocManager::new()),
        interrupt_manager: Mutex::new(InterruptManager::new()),
        efi_manager: Mutex::new(EfiManager::new_static()),
        serial_port_manager: SerialPortManager::new(0x3F8),
        task_manager: TaskManager::new(),
    }
}

#[inline(always)]
pub fn get_kernel_manager_cluster() -> &'static mut KernelManagerCluster {
    unsafe { &mut STATIC_KERNEL_MANAGER_CLUSTER }
}
