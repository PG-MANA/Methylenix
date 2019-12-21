/*
    Struct Manager for kernel
*/

//use
use arch::target_arch::device::serial_port::SerialPortManager;
use arch::target_arch::interrupt::InterruptManager;

use kernel::drivers::efi::EfiManager;
use kernel::graphic::GraphicManager;
use kernel::memory_manager::MemoryManager;
use kernel::spin_lock::Mutex;
use kernel::kernel_malloc::KernelMemoryAllocManager;


//Boot時に格納するデータ群
pub static mut STATIC_BOOT_INFORMATION_MANAGER: BootInformationManager =
    init_bootinformation_manager();

pub struct BootInformationManager {
    pub graphic_manager: Mutex<GraphicManager>,
    pub memory_manager: Mutex<MemoryManager>,
    pub kernel_memory_alloc_manager: Mutex<KernelMemoryAllocManager>,
    pub interrupt_manager: Mutex<InterruptManager>,
    pub efi_manager: Mutex<EfiManager>,
    pub serial_port_manager: Mutex<SerialPortManager>,
    //input_manager:
}

const fn init_bootinformation_manager() -> BootInformationManager {
    BootInformationManager {
        graphic_manager: Mutex::new(GraphicManager::new_static()),
        memory_manager: Mutex::new(MemoryManager::new_static()),
        kernel_memory_alloc_manager: Mutex::new(KernelMemoryAllocManager::new()),
        interrupt_manager: Mutex::new(InterruptManager::new()),
        efi_manager: Mutex::new(EfiManager::new_static()),
        serial_port_manager: Mutex::new(SerialPortManager::new_static()),
    }
}
