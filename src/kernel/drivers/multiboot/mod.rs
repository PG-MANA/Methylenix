//参考:https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html

mod elf;
mod frame_buffer;
mod memory;

pub use self::elf::{ElfInfo, ElfSection};
pub use self::frame_buffer::FrameBufferInfo;
pub use self::memory::{MemoryInfo, MemoryMapEntry, MemoryMapInfo};
use core::mem;

//構造体
#[repr(C)] //Rustではstructが記述通りに並んでない
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

pub struct MultiBootInformation {
    pub multiboot_information_address: usize,
    pub multiboot_information_size: usize,
    pub memory_info: MemoryInfo,
    pub elf_info: ElfInfo,
    pub memory_map_info: MemoryMapInfo,
    pub framebuffer_info: FrameBufferInfo,
    pub efi_table_pointer: usize,
}

impl MultiBootInformation {
    #![allow(dead_code)] //使っていない定数でエラーが出る
    const TAG_ALIGN: u32 = 8;
    const TAG_TYPE_END: u32 = 0;
    const TAG_TYPE_CMDLINE: u32 = 1;
    const TAG_TYPE_BOOT_LOADER_NAME: u32 = 2;
    const TAG_TYPE_MODULE: u32 = 3;
    const TAG_TYPE_BASIC_MEMINFO: u32 = 4;
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

    pub fn new(address: usize) -> MultiBootInformation {
        let mut mbi: MultiBootInformation = unsafe { mem::zeroed() };
        if !MultiBootInformation::test(address) {
            panic!("Unaligned Multi Boot Information")
        }
        let total_size = MultiBootInformation::total_size(address);
        if total_size == 0 {
            panic!("Invalid Multi Boot Information")
        }
        let mut tag = address + 8;

        loop {
            let tag_type: u32 = unsafe { (&*(tag as *const MultibootTag)).s_type };
            match tag_type {
                MultiBootInformation::TAG_TYPE_END => {
                    break;
                }
                MultiBootInformation::TAG_TYPE_MMAP => {
                    mbi.memory_map_info = MemoryMapInfo::new(unsafe { &*(tag as *const _) });
                }
                MultiBootInformation::TAG_TYPE_ACPI_NEW => {}
                MultiBootInformation::TAG_TYPE_BASIC_MEMINFO => {
                    mbi.memory_info = MemoryInfo::new(unsafe { &*(tag as *const _) }); //完全に信用すべきではない(CPUIDなどで問い合わせる)
                }
                MultiBootInformation::TAG_TYPE_CMDLINE => {}
                MultiBootInformation::TAG_TYPE_BOOT_LOADER_NAME => {}
                MultiBootInformation::TAG_TYPE_FRAMEBUFFER => {
                    mbi.framebuffer_info = FrameBufferInfo::new(unsafe { &*(tag as *const _) });
                }
                MultiBootInformation::TAG_TYPE_EFI64 => {
                    mbi.efi_table_pointer =
                        unsafe { (*(tag as *const EfiSystemTableInformation)).address };
                }
                MultiBootInformation::TAG_TYPE_ELF_SECTIONS => {
                    mbi.elf_info = ElfInfo::new(unsafe { &*(tag as *const _) });
                }
                MultiBootInformation::TAG_TYPE_BOOTDEV => {}
                _ => {
                    if tag - address - 8 >= (total_size as usize) {
                        break;
                    }
                }
            }
            tag += ((unsafe { (&*(tag as *const MultibootTag)).size } as usize) + 7) & !7;
        }

        //返却
        mbi
    }

    fn test(address: usize) -> bool {
        if address & 7 != 0 {
            false
        } else {
            true
        }
    }

    fn total_size(address: usize) -> u32 {
        unsafe { *(address as *mut u32) }
    }
}
