//!
//! Panic Handler
//!

use crate::kernel::manager_cluster::get_kernel_manager_cluster;

#[panic_handler]
pub fn panic(info: &core::panic::PanicInfo) -> ! {
    kprintln!("\n!!!! Kernel panic !!!!");
    if let Some(location) = info.location() {
        kprintln!(
            "{}:{}: {}",
            location.file(),
            location.line(),
            info.message()
        );
    } else {
        kprintln!("{}", info.message());
    }

    get_kernel_manager_cluster()
        .kernel_memory_manager
        .dump_memory_manager();

    kprintln!("---- End of Debug information ----");

    /* Write twice */
    if let Some(location) = info.location() {
        kprintln!(
            "{}:{}: {}",
            location.file(),
            location.line(),
            info.message()
        );
    } else {
        kprintln!("{}", info.message());
    }

    loop {
        unsafe {
            crate::arch::target_arch::device::cpu::halt();
        }
    }
}
