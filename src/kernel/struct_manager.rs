/*
structをグローバルでやり取りするためのマネージャ(プロセス通信ができない、割り込み向け)
*/

//use(Arch実装依存)
use arch::target_arch::interrupt::InterruptManager;

//use(Arch非依存)
use kernel::drivers::efi::EfiManager;
use kernel::graphic::GraphicManager;
use kernel::memory_manager::MemoryManager;
use kernel::spin_lock::Mutex;

//Boot時に格納するデータ群
pub static mut STATIC_BOOT_INFORMATION_MANAGER: BootInformationManager =
    init_bootinformation_manager();

pub struct BootInformationManager {
    pub graphic_manager: Mutex<GraphicManager>,
    pub memory_manager: Mutex<MemoryManager>,
    pub interrupt_manager: Mutex<InterruptManager>,
    pub efi_manager: Mutex<EfiManager>,
    //input_manager:
}

const fn init_bootinformation_manager() -> BootInformationManager {
    BootInformationManager {
        graphic_manager: Mutex::new(GraphicManager::new_static()),
        memory_manager: Mutex::new(MemoryManager::new_static()),
        interrupt_manager: Mutex::new(InterruptManager::new_static()),
        efi_manager: Mutex::new(EfiManager::new_static()),
    }
}
