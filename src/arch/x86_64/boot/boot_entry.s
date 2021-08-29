/*
 * Boot entry for legacy boot or multiboot
 */

.code32

.global boot_entry
.extern boot_multiboot     /* at boot_multiboot.s */

.section .text

.code32

boot_entry:
  jmp boot_multiboot
