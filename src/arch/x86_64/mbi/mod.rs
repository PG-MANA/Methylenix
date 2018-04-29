/*
 * Copyright 2017 PG_MANA
 *
 * This software is Licensed under the Apache License Version 2.0
 * See LICENSE.md
 *
 * MultiBoot2実装
 */

//参考:http://git.savannah.gnu.org/cgit/grub.git/tree/doc/kernel.c?h=multiboot2

//マルチブートインフォメーションに関する宣言
//enum => u32が難しそう
//enum TagType{
//#defineに当たる

#![allow(dead_code)]
//!をつけているのでカッコがないこの場合はグローバル適用
const MULTIBOOT_TAG_ALIGN: u32 = 8;
const MULTIBOOT_TAG_TYPE_END: u32 = 0;
const MULTIBOOT_TAG_TYPE_CMDLINE: u32 = 1;
const MULTIBOOT_TAG_TYPE_BOOT_LOADER_NAME: u32 = 2;
const MULTIBOOT_TAG_TYPE_MODULE: u32 = 3;
const MULTIBOOT_TAG_TYPE_BASIC_MEMINFO: u32 = 4;
const MULTIBOOT_TAG_TYPE_BOOTDEV: u32 = 5;
const MULTIBOOT_TAG_TYPE_MMAP: u32 = 6;
const MULTIBOOT_TAG_TYPE_VBE: u32 = 7;
const MULTIBOOT_TAG_TYPE_FRAMEBUFFER: u32 = 8;
const MULTIBOOT_TAG_TYPE_ELF_SECTIONS: u32 = 9;
const MULTIBOOT_TAG_TYPE_APM: u32 = 10;
const MULTIBOOT_TAG_TYPE_EFI32: u32 = 11;
const MULTIBOOT_TAG_TYPE_EFI64: u32 = 12;
const MULTIBOOT_TAG_TYPE_SMBIOS: u32 = 13;
const MULTIBOOT_TAG_TYPE_ACPI_OLD: u32 = 14;
const MULTIBOOT_TAG_TYPE_ACPI_NEW: u32 = 15;
const MULTIBOOT_TAG_TYPE_NETWORK: u32 = 16;
const MULTIBOOT_TAG_TYPE_EFI_MMAP: u32 = 17;
const MULTIBOOT_TAG_TYPE_EFI_BS: u32 = 18;
const MULTIBOOT_TAG_TYPE_EFI32_IH: u32 = 19;
const MULTIBOOT_TAG_TYPE_EFI64_IH: u32 = 20;
const MULTIBOOT_TAG_TYPE_LOAD_BASE_ADDR: u32 = 21;

//Module
mod elf;
mod memory;

//use
pub use self::elf::{ElfInfo, ElfSection};
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
}

impl MultiBootInformation {
    pub fn new(addr: usize) -> MultiBootInformation {
        //core::mem::uninitializedは慎重に使うべき(zeroedも避けるべきだがデフォルトの数値がすべて0だから...)
        //let mut mbi = MultiBootInformation{..Default::default()};
        let mut mbi: MultiBootInformation = unsafe { mem::zeroed() };

        let total_size = total_size(addr);
        let mut tag = addr + 8;

        loop {
            let typ: u32 = unsafe { (&*(tag as *const MultibootTag)).s_type };
            match typ {
                MULTIBOOT_TAG_TYPE_END => {
                    puts!("END\n");
                    break;
                }
                MULTIBOOT_TAG_TYPE_MMAP => {
                    puts!("Memory Map=>");
                    mbi.memmapinfo = MemoryMapInfo::new(unsafe { &*(tag as *const _) });
                }
                MULTIBOOT_TAG_TYPE_ACPI_NEW => {
                    puts!("ACPI(NEW)=>");
                }
                MULTIBOOT_TAG_TYPE_BASIC_MEMINFO => {
                    puts!("Basic MemoryInfo=>");
                    mbi.meminfo = MemoryInfo::new(unsafe { &*(tag as *const _) }); //完全に信用すべきではない(CPUIDなどで問い合わせる)
                }
                MULTIBOOT_TAG_TYPE_VBE => {
                    puts!("VBE Struct=>");
                }
                MULTIBOOT_TAG_TYPE_CMDLINE => {
                    puts!("Cmdline=>");
                }
                MULTIBOOT_TAG_TYPE_BOOT_LOADER_NAME => {
                    puts!("Boot Loader Name=>");
                }
                MULTIBOOT_TAG_TYPE_SMBIOS => {
                    puts!("SMBIOS=>");
                }
                MULTIBOOT_TAG_TYPE_FRAMEBUFFER => {
                    puts!("FRAMEBUFFER=>");
                }
                MULTIBOOT_TAG_TYPE_EFI32 => {
                    puts!("EFI32=>");
                }
                MULTIBOOT_TAG_TYPE_ELF_SECTIONS => {
                    mbi.elfinfo = ElfInfo::new(unsafe { &*(tag as *const _) });
                    puts!("ELF SECTIONS=>");
                }
                MULTIBOOT_TAG_TYPE_APM => {
                    puts!("APM=>");
                }
                MULTIBOOT_TAG_TYPE_BOOTDEV => {
                    puts!("BOOTDEV=>");
                }
                _ => {
                    if tag - addr - 8 >= (total_size as usize) {
                        puts!("List is too long.");
                        break;
                    }
                    puts!("Unknown...=>")
                }
            }
            tag += ((unsafe { (&*(tag as *const MultibootTag)).size } as usize) + 7) & !7;
        }

        //返却
        mbi
    }
}

/*
impl Default for MultiBootInformation{
    fn default() -> MultiBootInformation {//デフォルト値
        MultiBootInformation{
            meminfo : MemoryInfo{..Default::default()},
            elfinfo : ElfInfo{..Default::default()},
            memmapinfo : MemoryMapInfo{..Default::default()},
        }
    }
}*/

pub fn test(addr: usize) -> bool {
    if addr & 7 != 0
    /*こう書かないと怒られる*/
    {
        false
    } else {
        true
    }
}

pub fn total_size(addr: usize) -> u32 {
    return unsafe { *(addr as *mut u32) };
}