//!
//! Cluster of Managers for kernel
//!
//! This cluster stores necessary structs for kernel.
//! All members of manager must be Mutex.

use crate::arch::target_arch::device::serial_port::SerialPortManager;
use crate::arch::target_arch::interrupt::InterruptManager;
use crate::arch::target_arch::{ArchDependedCpuManagerCluster, ArchDependedKernelManagerCluster};

use crate::kernel::block_device::BlockDeviceManager;
use crate::kernel::collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode};
use crate::kernel::drivers::acpi::device::AcpiDeviceManager;
use crate::kernel::drivers::acpi::event::AcpiEventManager;
use crate::kernel::drivers::acpi::AcpiManager;
use crate::kernel::drivers::pci::PciManager;
use crate::kernel::file_manager::FileManager;
use crate::kernel::graphic_manager::GraphicManager;
use crate::kernel::memory_manager::memory_allocator::MemoryAllocator;
use crate::kernel::memory_manager::{system_memory_manager::SystemMemoryManager, MemoryManager};
use crate::kernel::network_manager::ethernet_device::EthernetDeviceManager;
use crate::kernel::sync::spin_lock::Mutex;
use crate::kernel::task_manager::run_queue::RunQueue;
use crate::kernel::task_manager::work_queue::WorkQueue;
use crate::kernel::task_manager::TaskManager;
use crate::kernel::timer_manager::{GlobalTimerManager, LocalTimerManager};
use crate::kernel::tty::TtyManager;

use core::mem::MaybeUninit;

pub static mut STATIC_KERNEL_MANAGER_CLUSTER: MaybeUninit<KernelManagerCluster> =
    MaybeUninit::uninit();

pub struct KernelManagerCluster {
    pub graphic_manager: GraphicManager,
    pub kernel_memory_manager: MemoryManager,
    pub system_memory_manager: SystemMemoryManager,
    pub serial_port_manager: SerialPortManager,
    pub task_manager: TaskManager,
    pub kernel_tty_manager: TtyManager, /*SerialPortManager has mutex process inner*/
    pub block_device_manager: BlockDeviceManager,
    pub ethernet_device_manager: EthernetDeviceManager,
    pub file_manager: FileManager,
    pub acpi_manager: Mutex<AcpiManager>,
    pub acpi_event_manager: AcpiEventManager,
    pub acpi_device_manager: AcpiDeviceManager,
    pub pci_manager: PciManager,
    pub global_timer_manager: GlobalTimerManager,
    pub boot_strap_cpu_manager: CpuManagerCluster,
    pub cpu_list: PtrLinkedList<CpuManagerCluster>, /* may be changed */
    pub arch_depend_data: ArchDependedKernelManagerCluster,
}

#[inline(always)]
pub fn get_kernel_manager_cluster() -> &'static mut KernelManagerCluster {
    /* You must assign new struct before use the structs!! */
    unsafe { STATIC_KERNEL_MANAGER_CLUSTER.assume_init_mut() }
}

pub struct CpuManagerCluster {
    pub cpu_id: usize,
    pub list: PtrLinkedListNode<Self>,
    pub interrupt_manager: InterruptManager,
    pub work_queue: WorkQueue,
    pub memory_allocator: MemoryAllocator,
    pub run_queue: RunQueue,
    pub local_timer_manager: LocalTimerManager,
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
