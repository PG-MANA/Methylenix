/*
 * Multiboot2 information header
 */

.equ MULTIBOOT_HEADER_MAGIC,                0xe85250d6
.equ MULTIBOOT_HEADER_ARCH,                 0
.equ MULTIBOOT_HEADER_LEN,                  multiboot_end - multiboot
.equ MULTIBOOT_HEADER_CHECKSUM,             -(MULTIBOOT_HEADER_MAGIC + MULTIBOOT_HEADER_ARCH + MULTIBOOT_HEADER_LEN)
.equ MULTIBOOT_HEADER_FLAG_MANDATORY,       0
.equ MULTIBOOT_HEADER_FLAG_OPTIONAL,        1
.equ MULTIBOOT_HEADER_TAG_TYPE_END,         0
.equ MULTIBOOT_HEADER_TAG_TYPE_ENTRY,       3
.equ MULTIBOOT_HEADER_TAG_TYPE_CONSOLE,     4
.equ MULTIBOOT_HEADER_TAG_TYPE_FB,          5
.equ MULTIBOOT_HEADER_TAG_TYPE_ALIGN,       6

.extern   boot_multiboot
.section  .header.multiboot, "a" /* Alloc flag */

.align    8
.type     multiboot, %object
.size     multiboot, MULTIBOOT_HEADER_LEN
multiboot:
  .long   MULTIBOOT_HEADER_MAGIC
  .long   MULTIBOOT_HEADER_ARCH
  .long   MULTIBOOT_HEADER_LEN
  .long   MULTIBOOT_HEADER_CHECKSUM

multiboot_tags_start:
  .word   MULTIBOOT_HEADER_TAG_TYPE_ENTRY
  .word   MULTIBOOT_HEADER_FLAG_MANDATORY
  .long   12                              /* Tag size */
  .long   boot_multiboot
  .align  8
  .word   MULTIBOOT_HEADER_TAG_TYPE_CONSOLE
  .word   MULTIBOOT_HEADER_FLAG_OPTIONAL
  .long   12                              /* Tag size */
  .long   (1 << 1)                        /* (1 << 1) EGA TEXT Supported */
  .align  8
  .word   MULTIBOOT_HEADER_TAG_TYPE_FB
  .word   MULTIBOOT_HEADER_FLAG_OPTIONAL
  .long   20                              /* Tag size */
  .long   0                               /* width (no preference) */
  .long   0                               /* height (no preference) */
  .long   0                               /* depth (no preference) */
  .align  8
  .word   MULTIBOOT_HEADER_TAG_TYPE_ALIGN
  .word   MULTIBOOT_HEADER_FLAG_MANDATORY
  .long   8
  .align  8
  .word   MULTIBOOT_HEADER_TAG_TYPE_END
  .word   MULTIBOOT_HEADER_FLAG_MANDATORY
  .long   8                              /* Tag size */
multiboot_end:
