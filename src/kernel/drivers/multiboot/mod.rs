//!
//! Multiboot Information
//!
//! This manager contains the multiboot2 information.
//! https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html
//!

mod elf;
mod frame_buffer;
mod memory;

pub use self::elf::{ElfInfo, ElfSection};
pub use self::frame_buffer::FrameBufferInfo;
pub use self::memory::{MemoryMapEntry, MemoryMapInfo};

use core::{mem, slice, str};

#[repr(C)]
struct MultibootTag {
    s_type: u32,
    size: u32,
}

#[repr(C)]
struct EfiSystemTableInformation {
    s_type: u32,
    size: u32,
    address: usize,
}

#[repr(C)]
struct MultibootTagModule {
    s_type: u32,
    size: u32,
    mod_start: u32,
    mod_end: u32,
    string: u8,
}

pub struct ModuleInfo {
    pub start_address: usize,
    pub end_address: usize,
    pub name: &'static str,
}

pub struct MultiBootInformation {
    pub elf_info: ElfInfo,
    pub memory_map_info: MemoryMapInfo,
    pub framebuffer_info: FrameBufferInfo,
    pub efi_table_pointer: Option<usize>,
    pub address: usize,
    pub size: usize,
    pub boot_loader_name: &'static str,
    pub boot_cmd_line: &'static str,
    pub new_acpi_rsdp_ptr: Option<usize>,
    pub old_acpi_rsdp_ptr: Option<usize>,
    pub modules: [ModuleInfo; 4],
}

impl MultiBootInformation {
    #![allow(dead_code)]
    const TAG_ALIGN: u32 = 8;
    const TAG_TYPE_END: u32 = 0;
    const TAG_TYPE_CMDLINE: u32 = 1;
    const TAG_TYPE_BOOT_LOADER_NAME: u32 = 2;
    const TAG_TYPE_MODULE: u32 = 3;
    /*const TAG_TYPE_BASIC_MEMINFO: u32 = 4;*/
    const TAG_TYPE_BOOTDEV: u32 = 5;
    const TAG_TYPE_MMAP: u32 = 6;
    const TAG_TYPE_VBE: u32 = 7;
    const TAG_TYPE_FRAMEBUFFER: u32 = 8;
    const TAG_TYPE_ELF_SECTIONS: u32 = 9;
    const TAG_TYPE_APM: u32 = 10;
    const TAG_TYPE_EFI32: u32 = 11;
    const TAG_TYPE_EFI64: u32 = 12;
    const TAG_TYPE_SMBIOS: u32 = 13;
    const TAG_TYPE_ACPI_OLD: u32 = 14;
    const TAG_TYPE_ACPI_NEW: u32 = 15;
    const TAG_TYPE_NETWORK: u32 = 16;
    const TAG_TYPE_EFI_MMAP: u32 = 17;
    const TAG_TYPE_EFI_BS: u32 = 18;
    const TAG_TYPE_EFI32_IH: u32 = 19;
    const TAG_TYPE_EFI64_IH: u32 = 20;
    const TAG_TYPE_BASE_ADDR: u32 = 21;

    pub fn new(address: usize, should_test: bool) -> Self {
        #[allow(invalid_value)]
        let mut mbi: Self = unsafe { mem::MaybeUninit::zeroed().assume_init() };
        if should_test && !Self::test(address) {
            pr_err!("Unaligned Multi Boot Information");
            return mbi;
        }
        mbi.address = address;
        mbi.size = Self::get_total_size(address);
        if mbi.size == 0 {
            pr_err!("Invalid Multi Boot Information");
            return mbi;
        }

        let mut tag = address + 8;
        loop {
            let tag_type: u32 = unsafe { (*(tag as *const MultibootTag)).s_type };
            match tag_type {
                MultiBootInformation::TAG_TYPE_END => {
                    break;
                }
                MultiBootInformation::TAG_TYPE_MMAP => {
                    mbi.memory_map_info = MemoryMapInfo::new(unsafe { &*(tag as *const _) });
                }
                MultiBootInformation::TAG_TYPE_ACPI_OLD => {
                    mbi.old_acpi_rsdp_ptr = Some(tag + 8);
                }
                MultiBootInformation::TAG_TYPE_ACPI_NEW => {
                    mbi.new_acpi_rsdp_ptr = Some(tag + 8);
                }
                MultiBootInformation::TAG_TYPE_CMDLINE => {
                    mbi.boot_cmd_line = str::from_utf8(unsafe {
                        slice::from_raw_parts(
                            (tag + 8) as *const u8,
                            (*(tag as *const MultibootTag)).size as usize - 8 - 1, /*\0*/
                        )
                    })
                    .unwrap_or("");
                }
                MultiBootInformation::TAG_TYPE_BOOT_LOADER_NAME => {
                    mbi.boot_loader_name = str::from_utf8(unsafe {
                        slice::from_raw_parts(
                            (tag + 8) as *const u8,
                            (*(tag as *const MultibootTag)).size as usize - 8 - 1, /*\0*/
                        )
                    })
                    .unwrap_or("");
                }
                MultiBootInformation::TAG_TYPE_FRAMEBUFFER => {
                    mbi.framebuffer_info = FrameBufferInfo::new(unsafe { &*(tag as *const _) });
                }
                MultiBootInformation::TAG_TYPE_EFI64 => {
                    mbi.efi_table_pointer =
                        Some(unsafe { (*(tag as *const EfiSystemTableInformation)).address });
                }
                MultiBootInformation::TAG_TYPE_ELF_SECTIONS => {
                    mbi.elf_info = ElfInfo::new(unsafe { &*(tag as *const _) });
                }
                MultiBootInformation::TAG_TYPE_MODULE => {
                    let module_info = unsafe { &*(tag as *const MultibootTagModule) };
                    for e in mbi.modules.iter_mut() {
                        if e.start_address == 0 && e.end_address == 0 {
                            e.start_address = module_info.mod_start as usize;
                            e.end_address = module_info.mod_end as usize;
                            e.name = str::from_utf8(unsafe {
                                slice::from_raw_parts(
                                    &module_info.string,
                                    module_info.size as usize - 16 - 1, /*\0*/
                                )
                            })
                            .unwrap_or("");
                            break;
                        }
                    }
                }
                _ => {
                    if tag - address - 8 >= mbi.size {
                        break;
                    }
                }
            }
            tag += ((unsafe { (*(tag as *const MultibootTag)).size } as usize) + 7) & !7;
        }
        return mbi;
    }

    fn test(address: usize) -> bool {
        if address & 7 != 0 {
            false
        } else {
            true
        }
    }

    fn get_total_size(address: usize) -> usize {
        unsafe { *(address as *mut u32) as usize }
    }
}
