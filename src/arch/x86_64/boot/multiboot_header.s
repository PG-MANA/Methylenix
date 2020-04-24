/*
 * Multiboot2 information header
 */

/* http://git.savannah.gnu.org/cgit/grub.git/tree/doc/multiboot2.h?h=multiboot2 */
.equ MULTIBOOT_HEADER_MAGIC,                0xe85250d6
.equ MULTIBOOT_HEADER_ARCH,                 0           /* 4=> mips */
.equ MULTIBOOT_HEADER_LEN,                  multiboot_end - multiboot_start
.equ MULTIBOOT_HEADER_CHECKSUM,             -(MULTIBOOT_HEADER_MAGIC + MULTIBOOT_HEADER_ARCH + MULTIBOOT_HEADER_LEN)
.equ MULTIBOOT_HEADER_FLAG,                 1           /* タグで使うフラグ(1はオプショナルを表す...?) */
.equ MULTIBOOT_HEADER_TAG_TYPE_END,         0           /* マルチブート用ヘッダー タグ終了 */
.equ MULTIBOOT_HEADER_TAG_TYPE_CONSOLE,     4           /* EGAテキストモードサポート */
.equ MULTIBOOT_HEADER_TAG_TYPE_FB,          5           /* フレームバッファ要求タグ */
.equ MULTIBOOT_HEADER_TAG_TYPE_ALIGN,       6           /* アライメント要求 */
.equ MULTIBOOT_HEADER_TAG_TYPE_EFI,         7           /* EFI サービスを終了させないようにする */
.equ MULTIBOOT_HEADER_TAG_TYPE_ENTRY_EFI64, 9           /* EFI64で最初に実行するアドレス */

/* マルチブート用ヘッダー */
.section .multiboot_header, "a" /* alloc flag*/

multiboot_start:
  .long      MULTIBOOT_HEADER_MAGIC
  .long      MULTIBOOT_HEADER_ARCH
  .long      MULTIBOOT_HEADER_LEN
  .long      MULTIBOOT_HEADER_CHECKSUM

multiboot_tags_start:
  .word      MULTIBOOT_HEADER_TAG_TYPE_CONSOLE
  .word      MULTIBOOT_HEADER_FLAG
  .long      12                         /* size */
  .long      (1 << 1)                   /* (1 << 1) EGA TEXT Supported */
  .align   8                            /* タグは8バイト間隔で並ぶ必要がある */
  /* 自力でフォント描写ができないため現在は無効 */
  //.word      MULTIBOOT_HEADER_TAG_TYPE_FB
  //.word      MULTIBOOT_HEADER_FLAG      /* flags  */
  //.long      20                         /* size(このタグのサイズ)(multiboot_tag_framebuffer_end - multiboot_tag_framebuffer)  */
  //.long      1024                       /* width(1行の文字数) */
  //.long      768                        /* height(行数) */
  //.long      32                         /* depth(色深度) */
  //.align   8
  .word      MULTIBOOT_HEADER_TAG_TYPE_ALIGN
  .word      MULTIBOOT_HEADER_FLAG
  .long      8
  .align   8
  //.word      MULTIBOOT_HEADER_TAG_TYPE_EFI
  //.word      MULTIBOOT_HEADER_FLAG
  //.long      8                          /* size */
  //.align   8
  //.word      MULTIBOOT_HEADER_TAG_TYPE_ENTRY_EFI64
  //.word      MULTIBOOT_HEADER_FLAG
  //.long      12                         /* size */
  //.long      init_efi64                 /* entry point for EFI(x86_64) */
  //.align   8
  .word      MULTIBOOT_HEADER_TAG_TYPE_END
  .word      MULTIBOOT_HEADER_FLAG
  .long      8                              /* size */
multiboot_tags_end:
multiboot_end:
