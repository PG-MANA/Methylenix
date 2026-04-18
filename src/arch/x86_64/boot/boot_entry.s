/*
 * Boot entry for legacy boot or multiboot
 */

.code32

.global     boot_entry
.extern     boot_multiboot     /* at boot_multiboot.s */

.section    .text.boot

.type       boot_entry, %function
boot_entry:
  jmp       boot_multiboot
.size       boot_entry, . - boot_entry
