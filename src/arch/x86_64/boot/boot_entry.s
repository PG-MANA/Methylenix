/*
 * Boot entry points
 */

.global     boot_entry, boot_multiboot
.global     BOOT_FROM_MULTIBOOT_MARK, BOOT_FROM_LOADER_MARK
.extern     init_long_mode                          /* at init_long_mode.s */
.extern     setup_long_mode                         /* at setup_long_mode.s */
.extern     OS_STACK_SIZE, os_stack                 /* at common.s */

.equ        BOOT_FROM_MULTIBOOT_MARK,   1
.equ        BOOT_FROM_LOADER_MARK,      2

.equ        MULTIBOOT_CHECK_MAGIC, 0x36d76289       /* multiboot2 magic code */

.section    .text
.code64
.type       boot_entry, %function
boot_entry:
    pushq   $BOOT_FROM_LOADER_MARK
    pushq   %rdi
    jmp     init_long_mode
.size       boot_entry, . - boot_entry

.section    .text.boot
.code32
.type       boot_multiboot, %function
boot_multiboot:
    mov     $(os_stack + OS_STACK_SIZE - KERNEL_MAP_START_ADDRESS), %esp
    push    $0
    popfd                                           /* Clear eflags */
    push    $0                                      /* padding for 64bit pop */
    push    $BOOT_FROM_MULTIBOOT_MARK               /* Push the mark booted from multiboot */
    push    $0                                      /* padding for 64bit pop */
    push    %ebx                                    /* Save multiboot information */
    cmp     $MULTIBOOT_CHECK_MAGIC, %eax
    je      setup_long_mode
 1:
    jmp     1b
.size       boot_multiboot, . - boot_multiboot
