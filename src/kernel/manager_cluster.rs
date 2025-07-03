//!
//! Cluster of Managers for kernel
//!
//! This cluster includes essential structs for kernel.
//! All members of manager must be Mutex.

use crate::arch::target_arch::{
    ArchDependedCpuManagerCluster, ArchDependedKernelManagerCluster,
    device::serial_port::SerialPortManager, interrupt::InterruptManager,
};

use crate::kernel::{
    block_device::BlockDeviceManager,
    collections::ptr_linked_list::{PtrLinkedList, PtrLinkedListNode},
    drivers::{
        acpi::{AcpiManager, device::AcpiDeviceManager, event::AcpiEventManager},
        pci::PciManager,
    },
    file_manager::FileManager,
    graphic_manager::GraphicManager,
    memory_manager::{
        MemoryManager, memory_allocator::MemoryAllocator,
        system_memory_manager::SystemMemoryManager,
    },
    network_manager::NetworkManager,
    sync::spin_lock::Mutex,
    task_manager::{TaskManager, run_queue::RunQueue, work_queue::WorkQueue},
    timer_manager::{GlobalTimerManager, LocalTimerManager},
    tty::TtyManager,
};

use core::mem::MaybeUninit;

static mut STATIC_KERNEL_MANAGER_CLUSTER: MaybeUninit<KernelManagerCluster> = MaybeUninit::uninit();

pub struct KernelManagerCluster<'a> {
    pub graphic_manager: GraphicManager,
    pub kernel_memory_manager: MemoryManager,
    pub system_memory_manager: SystemMemoryManager,
    pub serial_port_manager: SerialPortManager,
    pub task_manager: TaskManager,
    pub kernel_tty_manager: [TtyManager; TtyManager::NUMBER_OF_KERNEL_TTY],
    pub block_device_manager: BlockDeviceManager,
    pub network_manager: NetworkManager,
    pub file_manager: FileManager,
    pub acpi_manager: Mutex<AcpiManager>,
    pub acpi_event_manager: AcpiEventManager,
    pub acpi_device_manager: AcpiDeviceManager,
    pub pci_manager: PciManager,
    pub global_timer_manager: GlobalTimerManager,
    pub boot_strap_cpu_manager: CpuManagerCluster<'a>,
    pub cpu_list: PtrLinkedList<CpuManagerCluster<'a>>,
    pub arch_depend_data: ArchDependedKernelManagerCluster,
}

#[inline(always)]
pub fn get_kernel_manager_cluster() -> &'static mut KernelManagerCluster<'static> {
    /* You must assign new struct before use the structs!! */
    unsafe { STATIC_KERNEL_MANAGER_CLUSTER.assume_init_mut() }
}

pub struct CpuManagerCluster<'a> {
    pub cpu_id: usize,
    pub list: PtrLinkedListNode<Self>,
    pub interrupt_manager: InterruptManager,
    pub work_queue: WorkQueue,
    pub memory_allocator: MemoryAllocator,
    pub run_queue: RunQueue,
    pub local_timer_manager: LocalTimerManager<'a>,
    pub arch_depend_data: ArchDependedCpuManagerCluster,
}

#[inline(always)]
pub fn get_cpu_manager_cluster() -> &'static mut CpuManagerCluster<'static> {
    /* You must assign new struct before use the structs!! */
    unsafe {
        &mut *(crate::arch::target_arch::device::cpu::get_cpu_base_address()
            as *mut CpuManagerCluster)
    }
}
