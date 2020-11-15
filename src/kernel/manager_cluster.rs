/*
 * Cluster of Managers for kernel
 *
 * This cluster stores necessary structs for kernel.
 * All members of manager must be Mutex.
 */

use crate::arch::target_arch::device::serial_port::SerialPortManager;
use crate::arch::target_arch::interrupt::InterruptManager;
use crate::arch::target_arch::ArchDependedCpuManagerCluster;

use crate::kernel::drivers::efi::EfiManager;
use crate::kernel::graphic_manager::GraphicManager;
use crate::kernel::memory_manager::object_allocator::ObjectAllocator;
use crate::kernel::memory_manager::{MemoryManager, SystemMemoryManager};
use crate::kernel::ptr_linked_list::PtrLinkedListNode;
use crate::kernel::task_manager::work_queue::WorkQueueManager;
use crate::kernel::task_manager::TaskManager;
use crate::kernel::tty::TtyManager;

use crate::kernel::sync::spin_lock::Mutex;

use core::mem::MaybeUninit;

pub static mut STATIC_KERNEL_MANAGER_CLUSTER: MaybeUninit<KernelManagerCluster> =
    MaybeUninit::uninit();

pub struct KernelManagerCluster {
    pub graphic_manager: GraphicManager,
    pub memory_manager: Mutex<MemoryManager>,
    pub system_memory_manager: SystemMemoryManager,
    pub object_allocator: Mutex<ObjectAllocator>,
    pub efi_manager: Mutex<EfiManager>,
    pub serial_port_manager: SerialPortManager,
    pub task_manager: TaskManager,
    pub work_queue_manager: WorkQueueManager,
    pub kernel_tty_manager: TtyManager, /*SerialPortManager has mutex process inner*/
    //input_manager:
    pub boot_strap_cpu_manager: CpuManagerCluster,
}

#[inline(always)]
pub fn get_kernel_manager_cluster() -> &'static mut KernelManagerCluster {
    /* You must assign new struct before use the structs!! */
    unsafe { STATIC_KERNEL_MANAGER_CLUSTER.assume_init_mut() }
}

pub struct CpuManagerCluster {
    pub cpu_id: usize,
    pub list: PtrLinkedListNode<Self>,
    pub interrupt_manager: Mutex<InterruptManager>,
    pub work_queue_manager: WorkQueueManager,
    pub object_allocator: Mutex<ObjectAllocator>,
    pub arch_depend_data: ArchDependedCpuManagerCluster,
}

#[inline(always)]
pub fn get_cpu_manager_cluster() -> &'static mut CpuManagerCluster {
    /* You must assign new struct before use the structs!! */
    unsafe {
        &mut *(crate::arch::target_arch::device::cpu::get_cpu_base_address()
            as *mut CpuManagerCluster)
    }
}
