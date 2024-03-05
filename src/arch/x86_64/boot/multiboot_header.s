/*
 * Multiboot2 information header
 */

/* http://git.savannah.gnu.org/cgit/grub.git/tree/doc/multiboot2.h?h=multiboot2 */
.equ MULTIBOOT_HEADER_MAGIC,                0xe85250d6
.equ MULTIBOOT_HEADER_ARCH,                 0           /* 4=> MIPS */
.equ MULTIBOOT_HEADER_LEN,                  multiboot_end - multiboot
.equ MULTIBOOT_HEADER_CHECKSUM,             -(MULTIBOOT_HEADER_MAGIC + MULTIBOOT_HEADER_ARCH + MULTIBOOT_HEADER_LEN)
.equ MULTIBOOT_HEADER_FLAG,                 1           /* Optional flag */
.equ MULTIBOOT_HEADER_TAG_TYPE_END,         0           /* Tag end */
.equ MULTIBOOT_HEADER_TAG_TYPE_CONSOLE,     4           /* Console setting tag */
.equ MULTIBOOT_HEADER_TAG_TYPE_FB,          5           /* Framebuffer setting tag */
.equ MULTIBOOT_HEADER_TAG_TYPE_ALIGN,       6           /* Alignment setting tag */
.equ MULTIBOOT_HEADER_TAG_TYPE_EFI,         7           /* EFI Service setting tag */
.equ MULTIBOOT_HEADER_TAG_TYPE_ENTRY_EFI64, 9           /* EFI64 entry point setting tag */

.section .header.multiboot, "a" /* Alloc flag */

.align  8
.type   multiboot, %object
.size   multiboot, MULTIBOOT_HEADER_LEN
multiboot:
  .long      MULTIBOOT_HEADER_MAGIC
  .long      MULTIBOOT_HEADER_ARCH
  .long      MULTIBOOT_HEADER_LEN
  .long      MULTIBOOT_HEADER_CHECKSUM

multiboot_tags_start:
  .word      MULTIBOOT_HEADER_TAG_TYPE_CONSOLE
  .word      MULTIBOOT_HEADER_FLAG
  .long      12                         /* Tag size */
  .long      (1 << 1)                   /* (1 << 1) EGA TEXT Supported */
  .align   8                            /* Tags are aligned by 8bytes */
  //.word      MULTIBOOT_HEADER_TAG_TYPE_FB
  //.word      MULTIBOOT_HEADER_FLAG      /* flags  */
  //.long      20                         /* tag size */
  //.long      1024                       /* width */
  //.long      768                        /* height */
  //.long      32                         /* depth */
  //.align   8
  .word      MULTIBOOT_HEADER_TAG_TYPE_ALIGN
  .word      MULTIBOOT_HEADER_FLAG
  .long      8
  .align   8
  //.word      MULTIBOOT_HEADER_TAG_TYPE_EFI
  //.word      MULTIBOOT_HEADER_FLAG
  //.long      8                          /* tag size */
  //.align   8
  //.word      MULTIBOOT_HEADER_TAG_TYPE_ENTRY_EFI64
  //.word      MULTIBOOT_HEADER_FLAG
  //.long      12                         /* tag size */
  //.long      init_efi64                 /* entry point for EFI(x86_64) */
  //.align   8
  .word      MULTIBOOT_HEADER_TAG_TYPE_END
  .word      MULTIBOOT_HEADER_FLAG
  .long      8                              /* tag size */
multiboot_end:
