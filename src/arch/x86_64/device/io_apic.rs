/*
IO APIC
TODO: impl
*/

use arch::target_arch::device::cpu;

pub fn init_io_apic(apic_id: u32) {
    //temp(serial port)
    unsafe {
        core::ptr::write_volatile(0xfec00000 as *mut u32, 0x10 + 4 * 2);
        let mut table = core::ptr::read_volatile(0xfec00010 as *mut u32) as u64;
        core::ptr::write_volatile(0xfec00000 as *mut u32, 0x10 + 4 * 2 + 1);
        table |= (core::ptr::read_volatile(0xfec00010 as *mut u32) as u64) << 32;
        table &= 0x00fffffffffe0000u64;
        table |= ((apic_id as u64) << 56) | 0x24;
        core::ptr::write_volatile(0xfec00000 as *mut u32, 0x10 + 4 * 2);
        core::ptr::write_volatile(0xfec00010 as *mut u32, table as u32);
        core::ptr::write_volatile(0xfec00000 as *mut u32, 0x10 + 4 * 2 + 1);
        core::ptr::write_volatile(0xfec00010 as *mut u32, (table >> 32) as u32);
    }
}
