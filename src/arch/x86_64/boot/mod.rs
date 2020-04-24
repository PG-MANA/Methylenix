/*
 * asm files for boot
 */

global_asm!(include_str!("common.s"));
global_asm!(include_str!("multiboot_header.s"));
global_asm!(include_str!("boot_entry.s"));
global_asm!(include_str!("boot_from_multiboot.s"));
global_asm!(include_str!("init_long_mode.s"));
global_asm!(include_str!("init_x86_64.s"));
