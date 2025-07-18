//!
//! AArch 64 Paging
//!

use crate::cpu::*;

pub const NUM_OF_ENTRIES_IN_PAGE_TABLE: usize = 512;
pub const TTBR1_EL1_START_ADDRESS: usize = 0xFFFF_0000_0000_0000;
pub const PAGE_SIZE: usize = 0x1000;
pub const PAGE_SHIFT: usize = 12;

/* Settings */
static mut TTBR1_EL1: u64 = 0;
static mut TCR_EL1: u64 = 0;
static mut SCTLR_EL1: u64 = 0;
static mut MAIR_INDEX: u8 = 0;
static mut MINIMUM_TXSZ: u8 = 16;
static mut TTBR1_INITIAL_SHIFT: u8 = 0;

/* From kernel/memory_manager/data_type.rs */
#[derive(Clone, Eq, PartialEq, Copy)]
pub struct MemoryPermissionFlags(u8);

/* MemoryPermissionFlags */
impl MemoryPermissionFlags {
    pub const fn new(read: bool, write: bool, execute: bool, user_access: bool) -> Self {
        Self(
            (read as u8)
                | ((write as u8) << 1)
                | ((execute as u8) << 2)
                | ((user_access as u8) << 3),
        )
    }

    pub fn is_writable(&self) -> bool {
        self.0 & (1 << 1) != 0
    }

    pub fn is_executable(&self) -> bool {
        self.0 & (1 << 2) != 0
    }
}

pub fn get_direct_map_start_address() -> usize {
    u64::MAX as usize - ((1 << (64 - unsafe { MINIMUM_TXSZ })) - 1)
}

pub fn init_paging(top_level_page_table: usize) {
    let current_el = get_current_el() >> 2;
    let pa_range = get_id_aa64mmfr0_el1() & 0b1111;
    let pa_range = if pa_range > 4 {
        44 + 4 * (pa_range as u8 - 4)
    } else {
        (32 + 4 * pa_range as u8).min(42)
    };
    let minimum_txsz = (32 - (pa_range - 32)).max(unsafe { MINIMUM_TXSZ });

    let mut tcr_el1 = 0u64;
    if current_el == 2 && (get_hcr_el2() & (1 << 34)) == 0 {
        let original_tcr_el2 = get_tcr_el2();
        /* PS */
        tcr_el1 |= ((original_tcr_el2 & (0b111 << 16)) >> 16) << 32;
        /* TG0 */
        tcr_el1 |= original_tcr_el2 & (0b11 << 14);
        /* SH0 */
        tcr_el1 |= original_tcr_el2 & (0b11 << 12);
        /* ORGN0 */
        tcr_el1 |= original_tcr_el2 & (0b11 << 10);
        /* IRGN0 */
        tcr_el1 |= original_tcr_el2 & (0b11 << 8);
        /* T0SZ */
        tcr_el1 |= original_tcr_el2 & 0b111111;
    } else {
        let original_tcr_el1 = get_tcr_el1();
        /* IPS */
        tcr_el1 |= original_tcr_el1 & (0b111 << 32);
        /* TG0 */
        tcr_el1 |= original_tcr_el1 & (0b11 << 14);
        /* SH0 */
        tcr_el1 |= original_tcr_el1 & (0b11 << 12);
        /* ORGN0 */
        tcr_el1 |= original_tcr_el1 & (0b11 << 10);
        /* IRGN0 */
        tcr_el1 |= original_tcr_el1 & (0b11 << 8);
        /* T0SZ */
        tcr_el1 |= original_tcr_el1 & 0b111111;
    }
    /* TG1 */
    tcr_el1 |= 0b10 << 30;
    /* SH1 */
    tcr_el1 |= 0b11 << 28;
    /* IRGN1 */
    tcr_el1 |= 0b01 << 24;
    /* T1SZ */
    tcr_el1 |= (minimum_txsz as u64) << 16;

    /* Clear the page table */
    unsafe { core::ptr::write_bytes(top_level_page_table as *mut u8, 0, PAGE_SIZE) };
    flush_data_cache();

    let mut sctlr_el1 = get_sctlr_el1();
    /* Enable I, SA, C, M*/
    sctlr_el1 |= (1 << 12) | (1 << 3) | (1 << 2) | 1;
    sctlr_el1 &= !((1 << 3) | (1 << 1));

    let mut mair = if current_el == 2 {
        get_mair_el2()
    } else {
        get_mair_el1()
    };
    for i in 0..8 {
        if (mair & 0xff) == 0xff {
            unsafe { MAIR_INDEX = i };
            break;
        }
        mair >>= 8;
    }
    assert_ne!(mair, 0);

    unsafe {
        MINIMUM_TXSZ = minimum_txsz;
        TTBR1_INITIAL_SHIFT = PAGE_SHIFT as u8
            + 9 * (3
                - (4 - (1
                    + ((64 - minimum_txsz - (PAGE_SHIFT as u8) - 1) / ((PAGE_SHIFT as u8) - 3)))
                    as i8) as u8);
        TTBR1_EL1 = top_level_page_table as u64;
        TCR_EL1 = tcr_el1;
        SCTLR_EL1 = sctlr_el1;
    }
}

pub fn set_page_table() {
    unsafe {
        set_ttbr1_el1(TTBR1_EL1);
        set_sctlr_el1(SCTLR_EL1);
        set_tcr_el1(TCR_EL1);
        tlbi_asid_el1(0);
    }
}

fn create_attribute(is_executable: bool, is_writable: bool) -> u64 {
    ((!is_executable as u64) << 54)
        | (1 << 10/* AF */)
        | (0b11 << 8/* Inner Shareable */)
        | (((!is_writable as u64) << 1) << 6)
        | (unsafe { MAIR_INDEX as u64 } << 2)
}

fn _associate_address(
    table_address: usize,
    shift_level: u8,
    physical_address: &mut usize,
    virtual_address: &mut usize,
    pages: &mut usize,
    permission: MemoryPermissionFlags,
    alloc_pages: fn(usize) -> Option<usize>,
) -> Result<(), ()> {
    let table = unsafe { &mut *(table_address as *mut [u64; NUM_OF_ENTRIES_IN_PAGE_TABLE]) };
    let mut index = (*virtual_address >> shift_level) & (NUM_OF_ENTRIES_IN_PAGE_TABLE - 1);
    if shift_level == PAGE_SHIFT as u8 {
        let attr = create_attribute(permission.is_executable(), permission.is_writable()) | 0b11;

        while *pages > 0 && index < NUM_OF_ENTRIES_IN_PAGE_TABLE {
            table[index] = *physical_address as u64 | attr;
            *physical_address += PAGE_SIZE;
            *virtual_address += PAGE_SIZE;
            index += 1;
            *pages -= 1;
        }
        flush_data_cache();
        return Ok(());
    }
    while *pages > 0 && index < NUM_OF_ENTRIES_IN_PAGE_TABLE {
        let block_size = 1 << shift_level;
        let mask = block_size - 1;
        if *physical_address & mask == 0
            && *virtual_address & mask == 0
            && (*pages << PAGE_SHIFT) >= block_size
        {
            /* Block Entry */
            table[index] = *physical_address as u64
                | create_attribute(permission.is_executable(), permission.is_writable())
                | 0b01;
            *physical_address += block_size;
            *virtual_address += block_size;
            *pages -= block_size >> PAGE_SHIFT;
            index += 1;
            continue;
        }
        let next_table_address;
        if table[index] & 0b11 != 0b11 {
            next_table_address = alloc_pages(1).ok_or(())?;
            unsafe {
                core::ptr::write_bytes(
                    next_table_address as *mut u64,
                    0,
                    NUM_OF_ENTRIES_IN_PAGE_TABLE,
                )
            };
            flush_data_cache();
            table[index] = (next_table_address as u64) | 0b11;
        } else {
            next_table_address = (table[index] & (((1 << 48) - 1) ^ ((1 << 12) - 1))) as usize;
        }

        _associate_address(
            next_table_address,
            shift_level - 9,
            physical_address,
            virtual_address,
            pages,
            permission,
            alloc_pages,
        )?;

        index += 1;
    }
    Ok(())
}

pub fn associate_address(
    mut physical_address: usize,
    mut virtual_address: usize,
    permission: MemoryPermissionFlags,
    mut pages: usize,
    alloc_pages: fn(usize) -> Option<usize>,
) -> Result<(), ()> {
    assert!(virtual_address >= TTBR1_EL1_START_ADDRESS);
    virtual_address -= u64::MAX as usize - ((1 << (64 - unsafe { MINIMUM_TXSZ })) - 1);
    _associate_address(
        unsafe { TTBR1_EL1 } as usize,
        unsafe { TTBR1_INITIAL_SHIFT },
        &mut physical_address,
        &mut virtual_address,
        &mut pages,
        permission,
        alloc_pages,
    )
}
