//!
//! Assembly files for boot
//!

use core::arch::global_asm;

global_asm!(include_str!("multiboot_header.s"));
global_asm!(include_str!("common.s"));
global_asm!(include_str!("boot_entry.s"), options(att_syntax));
global_asm!(include_str!("setup_long_mode.s"), options(att_syntax));
global_asm!(include_str!("init_long_mode.s"), options(att_syntax));
global_asm!(include_str!("boot_ap.s"), options(att_syntax));
