//!
//! Task State Segment
//!
//! Control TSS.
//! This struct is not used, but in the future, it may be used to set up ist.

use crate::arch::target_arch::device::cpu;

use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

const IO_MAP_SIZE: usize = (0xFFFF / 8) + 1;

#[allow(dead_code)]
#[repr(C, packed)]
struct TSS {
    reserved_1: u32,
    rsp0_l: u32,
    rsp0_u: u32,
    rsp1_l: u32,
    rsp1_u: u32,
    rsp2_l: u32,
    rsp2_u: u32,
    reserved_2: u32,
    reserved_3: u32,
    ist_1_l: u32,
    ist_1_u: u32,
    ist_2_l: u32,
    ist_2_u: u32,
    ist_3_l: u32,
    ist_3_u: u32,
    ist_4_l: u32,
    ist_4_u: u32,
    ist_5_l: u32,
    ist_5_u: u32,
    ist_6_l: u32,
    ist_6_u: u32,
    ist_7_l: u32,
    ist_7_u: u32,
    reserved_4: u32,
    reserved_5: u32,
    reserved_6: u16,
    io_map_base: u16,
    /* I/O Permission flag (0:Allow, 1:Forbid) */
    io_permission_map: [u8; IO_MAP_SIZE],
}

pub struct TssManager {
    tss: usize,
}

impl TssManager {
    pub const SIZE_OF_TSS: MSize = MSize::new(size_of::<TSS>());

    pub const fn new() -> Self {
        Self { tss: 0 }
    }

    pub fn init_tss(tss_address: VAddress) {
        let tss_address = tss_address.to_usize();
        let tss = unsafe { &mut *(tss_address as *mut TSS) };

        unsafe {
            core::ptr::write_bytes(
                tss_address as *mut u8,
                0,
                Self::SIZE_OF_TSS.to_usize() - IO_MAP_SIZE,
            )
        };
        tss.io_map_base = ((&tss.io_permission_map as *const u8 as usize) - tss_address) as u16;
        tss.io_permission_map = [0xff; IO_MAP_SIZE];
    }

    pub fn load_current_tss(&mut self) {
        let mut gdt: u128 = 0;
        unsafe { cpu::sgdt(&mut gdt) };

        let gdt_address = ((gdt >> 16) & usize::MAX as u128) as usize;
        let gdt_limit = (gdt & u16::MAX as u128) as u16;
        let tr = unsafe { cpu::store_tr() };

        if tr >= gdt_limit {
            self.tss = 0;
            return;
        }
        let tss_descriptor_address = gdt_address + tr as usize;
        self.tss = unsafe {
            (*((tss_descriptor_address + 2) as *const u16)) as usize
                | ((*((tss_descriptor_address + 4) as *const u8) as usize) << 16)
                | ((*((tss_descriptor_address + 7) as *const u8) as usize) << 24)
                | ((*((tss_descriptor_address + 8) as *const u32) as usize) << 32)
        };
    }

    pub fn set_ist(&self, ist: u8, stack_address: usize) -> bool {
        if self.tss == 0 || 0 == ist || ist >= 8 {
            return false;
        }
        let target_ist_address = self.tss + 28 + (ist * 8) as usize;
        unsafe {
            *(target_ist_address as *mut u32) = (stack_address & 0xffffffff) as u32;
            *((target_ist_address + 4) as *mut u32) = (stack_address >> 32) as u32;
        }
        true
    }

    pub fn set_rsp(&self, rsp: u8, stack_address: usize) -> bool {
        if self.tss == 0 || rsp >= 3 {
            return false;
        }
        let target_rsp_address = self.tss + 4 + (rsp * 8) as usize;
        unsafe {
            *(target_rsp_address as *mut u32) = (stack_address & 0xffffffff) as u32;
            *((target_rsp_address + 4) as *mut u32) = (stack_address >> 32) as u32;
        }
        true
    }
}
