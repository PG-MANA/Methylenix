/*
 * Programmable Interval Timer 8254
 */

use arch::target_arch::device::cpu;
use arch::target_arch::interrupt::idt;
use kernel::manager_cluster::get_kernel_manager_cluster;

pub struct PitManager {}

impl PitManager {
    pub fn init() {
        make_interrupt_hundler!(inthandler20, PitManager::inthandler20_main);
        let mut interrupt_manager = get_kernel_manager_cluster()
            .interrupt_manager
            .lock()
            .unwrap();
        interrupt_manager.set_device_interrupt_function(
            inthandler20, /*上のマクロで指定した名前*/
            Some(0),
            0x20,
            0,
        );

        unsafe {
            cpu::out_byte(0x43, 0x34);
            cpu::out_byte(0x40, 0);
            cpu::out_byte(0x40, 0);
        }
    }

    pub fn inthandler20_main() {
        if let Ok(im) = get_kernel_manager_cluster().interrupt_manager.try_lock() {
            im.send_eoi();
        }
    }
}
