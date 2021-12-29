//! Assembly files for boot
//!
//! This module is the boot code to jump to the main code by global_asm
//! Supported boot system:
//!  * Multiboot2
//!  * Xen HVM direct boot

use core::arch::global_asm;

global_asm!(include_str!("common.s"));
global_asm!(include_str!("multiboot_header.s"));
global_asm!(include_str!("xen_header.s"));
global_asm!(include_str!("boot_entry.s"), options(att_syntax));
global_asm!(include_str!("boot_ap.s"), options(att_syntax));
global_asm!(include_str!("boot_multiboot.s"), options(att_syntax));
global_asm!(include_str!("boot_xen.s"), options(att_syntax));
global_asm!(include_str!("setup_long_mode.s"), options(att_syntax));
global_asm!(include_str!("init_long_mode.s"), options(att_syntax));
