/*
 * Reverse Memory Map Manager
 * This manager maintains information to convert physical address to virtual address
 */

use core::mem;

pub struct ReverseMemoryMapManager {
    array_head: usize,
    enabled: bool,
}

struct ReverseMapEntry {
    ref_count: usize,
    vm_address: usize,
    /* temporary */
}

impl ReverseMemoryMapManager {
    const ENTRY_SIZE: usize = mem::sizeof::<ReverseMapEntry>();
    pub const fn new() -> Self {
        Self {
            array_head: 0,
            enabled: false,
        }
    }
}
