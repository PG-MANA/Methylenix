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

pub struct MultiBootInformation {
    pub meminfo: MemoryInfo,
    pub elfinfo: ElfInfo,
    pub memmapinfo: MemoryMapInfo,
    pub framebufferinfo: FrameBufferInfo,
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

    pub fn new(addr: usize) -> MultiBootInformation {
        //core::mem::uninitializedは慎重に使うべき(zeroedも避けるべきだがデフォルトの数値がすべて0だから...)
        //let mut mbi = MultiBootInformation{..Default::default()};
        let mut mbi: MultiBootInformation = unsafe { mem::zeroed() };

        let total_size = total_size(addr);
        let mut tag = addr + 8;
        loop {
            let tag_type: u32 = unsafe { (&*(tag as *const MultibootTag)).s_type };
            match tag_type {
                MultiBootInformation::TAG_TYPE_END => {
                    break;
                }
                MultiBootInformation::TAG_TYPE_MMAP => {
                    mbi.memmapinfo = MemoryMapInfo::new(unsafe { &*(tag as *const _) });
                }
                MultiBootInformation::TAG_TYPE_ACPI_NEW => {}
                MultiBootInformation::TAG_TYPE_BASIC_MEMINFO => {
                    mbi.meminfo = MemoryInfo::new(unsafe { &*(tag as *const _) }); //完全に信用すべきではない(CPUIDなどで問い合わせる)
                }
                MultiBootInformation::TAG_TYPE_CMDLINE => {}
                MultiBootInformation::TAG_TYPE_BOOT_LOADER_NAME => {}
                MultiBootInformation::TAG_TYPE_FRAMEBUFFER => {
                    mbi.framebufferinfo = FrameBufferInfo::new(unsafe { &*(tag as *const _) });
                }
                MultiBootInformation::TAG_TYPE_EFI32 => {}
                MultiBootInformation::TAG_TYPE_ELF_SECTIONS => {
                    mbi.elfinfo = ElfInfo::new(unsafe { &*(tag as *const _) });
                }
                MultiBootInformation::TAG_TYPE_BOOTDEV => {}
                _ => {
                    if tag - addr - 8 >= (total_size as usize) {
                        break;
                    }
                }
            }
            tag += ((unsafe { (&*(tag as *const MultibootTag)).size } as usize) + 7) & !7;
        }

        //返却
        mbi
    }
}

pub fn test(addr: usize) -> bool {
    if addr & 7 != 0 {
        false
    } else {
        true
    }
}

pub fn total_size(addr: usize) -> u32 {
    return unsafe { *(addr as *mut u32) };
}
