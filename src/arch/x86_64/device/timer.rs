//use(Arch依存)
use arch::target_arch::device::cpu;
use arch::target_arch::device::pic;
use arch::target_arch::interrupt::idt;

//use(Arch非依存)
use kernel::struct_manager::STATIC_BOOT_INFORMATION_MANAGER;
use kernel::task::TaskManager;

pub struct PitManager {}

impl PitManager {
    pub fn init() {
        unsafe {
            let interrupt_manager = STATIC_BOOT_INFORMATION_MANAGER.interrupt_manager.lock().unwrap();
            interrupt_manager.set_gatedec(
                0x20,
                idt::GateDescriptor::new(
                    Self::inthandler20_main, /*上のマクロで指定した名前*/
                    interrupt_manager.get_gdt(),
                    0,
                    idt::GateDescriptor::AR_INTGATE32,
                ),
            );
        }
        unsafe {
            cpu::out_byte(0x43, 0x34);
            cpu::out_byte(0x40, 0);
            cpu::out_byte(0x40, 0);
            pic::pic0_accept(0x01);
        }
    }


    extern "x86-interrupt" fn inthandler20_main() {
        unsafe {
            pic::pic0_eoi(0x00);
            if let Ok(task_manager) = STATIC_BOOT_INFORMATION_MANAGER.task_manager.try_lock() {
                TaskManager::context_switch(task_manager);
            }
        }
    }
}