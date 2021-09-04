//!
//! Panic Handler
//!

use crate::arch::target_arch::device::cpu;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;

use core::panic;

#[panic_handler]
#[no_mangle]
pub fn panic(info: &panic::PanicInfo) -> ! {
    let location = info.location();
    let message = info.message();

    kprintln!("\n!!!! Kernel panic !!!!\n---- Debug information ----");
    if location.is_some() && message.is_some() {
        kprintln!(
            "Line {} in {}\nMessage: {}",
            location.unwrap().line(),
            location.unwrap().file(),
            message.unwrap()
        );
    }
    get_kernel_manager_cluster()
        .memory_manager
        .dump_memory_manager();

    kprintln!("---- End of Debug information ----\nSystem will be halt.");

    loop {
        unsafe {
            cpu::halt();
        }
    }
}
