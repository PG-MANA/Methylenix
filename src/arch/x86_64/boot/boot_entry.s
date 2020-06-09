/*
 * boot entry code for legacy boot or multiboot
 */

.code32
.att_syntax

.global boot_entry
.extern boot_from_multiboot     /* at boot_from_multiboot.asm */

.section .text

.code32

boot_entry:
  jmp boot_from_multiboot
