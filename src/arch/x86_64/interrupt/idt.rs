//!
//! Interrupt Descriptor Table
//!
//! This module treats GateDescriptor.
//! This is usually used by InterruptManager.
use crate::arch::target_arch::device::cpu;

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{Address, MSize};

/// GateDescriptor
///
/// This structure is from Intel Software Developer's Manual.
/// If you want the detail, please read "Intel SDM 6.14 EXCEPTION AND INTERRUPT HANDLING IN 64-BIT MODE".
///
/// ## Structure
///
///  * offset: the virtual address to call when interruption was happened
///  * selector: th segment selector which is used when switch to the handler
///  * ist: interrupt stack table: if it is not zero,
///         CPU will change the stack from the specific stack pointer of TSS
///  * type_attr: GateDescriptor's type(task gate, interrupt gate, and call gate) and privilege level
#[repr(C)]
pub struct GateDescriptor {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_middle: u16,
    offset_high: u32,
    reserved: u32,
}

#[repr(C, packed)]
pub struct DescriptorTableRegister {
    pub limit: u16,
    pub offset: u64,
}

impl GateDescriptor {
    /// Create Gate Descriptor
    ///
    /// the detail is above.
    pub fn new(offset: unsafe fn(), selector: u16, ist: u8, type_attr: u8) -> GateDescriptor {
        let c = offset as *const unsafe fn() as usize;
        GateDescriptor {
            offset_low: (c & 0xffff) as u16,
            offset_middle: ((c & 0xffff0000) >> 16) as u16,
            offset_high: (c >> 32) as u32,
            selector,
            ist: ist & 0x07,
            type_attr,
            reserved: 0,
        }
    }

    pub fn fork_gdt_from_other_and_create_tss_and_set(original_gdt: usize, copy_size: u16) {
        let mut object_allocator = get_kernel_manager_cluster()
            .object_allocator
            .lock()
            .unwrap();
        let memory_manager = &get_kernel_manager_cluster().memory_manager;

        let new_gdt_address = object_allocator
            .alloc(
                MSize::new(copy_size as usize + 16 /*For TSS descriptor*/),
                memory_manager,
            )
            .expect("Cannot alloc the memory for GDT");

        let tss_address = object_allocator
            .alloc(MSize::new(104), memory_manager)
            .expect("Cannot alloc the memory for TSS");
        unsafe {
            core::ptr::copy_nonoverlapping(
                original_gdt as *const u8,
                new_gdt_address.to_usize() as *mut u8,
                copy_size as usize,
            );
            /*Set TSS Address */
            *((new_gdt_address.to_usize() + copy_size as usize) as *mut u128) = 110
                | ((tss_address.to_usize() & 0xff) << 16) as u128
                | ((tss_address.to_usize() as u128 & 0xf00) >> 16) << 32
                | 0b10001001 << 40
                | ((tss_address.to_usize() as u128 & 0xf000) >> 24) << 56
                | (tss_address.to_usize() as u128 >> 32) << 64;
            /* Zeroed TSS */
            core::ptr::write_bytes(tss_address.to_usize() as *mut u8, 0, 104);
        }
        let gdtr = ((new_gdt_address.to_usize() as u128) << 16) | (copy_size + 16) as u128;
        unsafe {
            cpu::lgdt(&gdtr);
            cpu::load_tr(copy_size);
        }
    }
}
