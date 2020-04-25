/*
 * HVM directboot header
 */

.section .note, "a"

.extern boot_from_xen

/* cannot use ELFNOTE macro because this source is included by global_asm! */
/* https://xenbits.xen.org/docs/4.10-testing/misc/pvh.html */
.equ XEN_ELFNOTE_XEN_VERSION, 5
.equ XEN_ELFNOTE_GUEST_OS, 6
.equ XEN_ELFNOTE_LOADER, 8
.equ XEN_ELFNOTE_PAE_MODE, 9
.equ XEN_ELFNOTE_PHYS32_ENTRY, 18

.equ XEN_NAME_SIZE, 4 /* Xen\0 */

.align 4

/* XEN VERSION */
.long XEN_NAME_SIZE
.long xen_version_end - xen_version_start /* size of desc */
.long XEN_ELFNOTE_XEN_VERSION
.asciz "Xen"
/* .align 4 */
xen_version_start:
.asciz "xen-3.0"
xen_version_end:

.align 4

/* GUEST OS*/
.long XEN_NAME_SIZE
.long xen_guest_os_end - xen_guest_os_start /* size of desc */
.long XEN_ELFNOTE_GUEST_OS
.asciz "Xen"
/* .align 4 */
xen_guest_os_start:
.asciz "Methylenix"
xen_guest_os_end:

.align 4

/* LOADEER */
.long XEN_NAME_SIZE
.long xen_loader_name_end - xen_loader_name_start /* size of desc */
.long XEN_ELFNOTE_LOADER
.asciz "Xen"
/* .align 4 */
xen_loader_name_start:
.asciz "generic"
xen_loader_name_end:

.align 4

/* PAE MODE */
.long XEN_NAME_SIZE
.long xen_pae_end - xen_pae_start /* size of desc */
.long XEN_ELFNOTE_PAE_MODE
.asciz "Xen"
/* .align 4 */
xen_pae_start:
.asciz "generic"
xen_pae_end:

.align 4

/* ENTRY ADDRESS */
.long XEN_NAME_SIZE
.long 4 /* size of desc(.long = 4) */
.long XEN_ELFNOTE_PHYS32_ENTRY
.asciz "Xen"
/* .align 4 */
.long boot_from_xen

.align 4