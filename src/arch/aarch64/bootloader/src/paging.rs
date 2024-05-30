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

    #[allow(dead_code)]
    pub const fn rodata() -> Self {
        Self::new(true, false, false, false)
    }

    pub const fn data() -> Self {
        Self::new(true, true, false, false)
    }

    #[allow(dead_code)]
    pub const fn user_data() -> Self {
        Self::new(true, true, false, true)
    }

    #[allow(dead_code)]
    pub fn is_readable(&self) -> bool {
        self.0 & (1 << 0) != 0
    }

    pub fn is_writable(&self) -> bool {
        self.0 & (1 << 1) != 0
    }

    pub fn is_executable(&self) -> bool {
        self.0 & (1 << 2) != 0
    }

    #[allow(dead_code)]
    pub fn is_user_accessible(&self) -> bool {
        self.0 & (1 << 3) != 0
    }
}

pub fn get_direct_map_start_address() -> usize {
    u64::MAX as usize - ((1 << (64 - unsafe { MINIMUM_TXSZ })) - 1)
}

pub fn init_paging(top_level_page_table: usize) {
    let pa_range = unsafe { get_id_aa64mmfr0_el1() & 0b1111 };
    let pa_range = if pa_range > 4 {
        44 + 4 * (pa_range as u8 - 4)
    } else {
        (32 + 4 * pa_range as u8).min(42)
    };
    let minimum_txsz = (32 - (pa_range - 32)).max(unsafe { MINIMUM_TXSZ });

    let mut tcr_el1 = 0u64;
    if unsafe { get_current_el() >> 2 } == 2 && unsafe { get_hcr_el2() & (1 << 34) } == 0 {
        let original_tcr_el2 = unsafe { get_tcr_el2() };
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
        let original_tcr_el1 = unsafe { get_tcr_el1() };
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

    let mut sctlr_el1 = unsafe { get_sctlr_el1() };
    /* Enable I, SA, C, M*/
    sctlr_el1 |= (1 << 12) | (1 << 3) | (1 << 2) | 1;
    sctlr_el1 &= !((1 << 3) | (1 << 1));

    let mut mair_el1 = unsafe { get_mair_el1() };
    for i in 0..8 {
        if (mair_el1 & 0xff) == 0xff {
            unsafe { MAIR_INDEX = i };
            break;
        }
        mair_el1 >>= 8;
    }

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

pub fn apply_paging_settings() {
    unsafe {
        set_ttbr1_el1(TTBR1_EL1);
        set_sctlr_el1(SCTLR_EL1);
        set_tcr_el1(TCR_EL1);
        tlbi_asid_el1(0);
    }
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
        let attr = ((!permission.is_executable() as u64) << 54)
            | (1 << 10/* AF */)
            | (0b11 << 8/* Inner Shareable */)
            | (((!permission.is_writable() as u64) << 1) << 6)
            | unsafe { MAIR_INDEX as u64 } << 2
            | 0b11;

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
        if table[index] & 0b11 != 0b11 {
            let next_table_address = alloc_pages(1).ok_or(())?;
            unsafe {
                core::ptr::write_bytes(
                    next_table_address as *mut u64,
                    0,
                    NUM_OF_ENTRIES_IN_PAGE_TABLE,
                )
            };
            flush_data_cache();
            table[index] = (next_table_address as u64) | 0b11;
        }
        let next_table_address = table[index] & (((1 << 48) - 1) ^ ((1 << 12) - 1));

        _associate_address(
            next_table_address as usize,
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
    if virtual_address < TTBR1_EL1_START_ADDRESS {
        unimplemented!();
    }
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

fn _associate_direct_map_address(
    table_address: usize,
    shift_level: u8,
    physical_address: &mut usize,
    virtual_address: &mut usize,
    size: &mut usize,
    permission: MemoryPermissionFlags,
    alloc_pages: fn(usize) -> Option<usize>,
) -> Result<(), ()> {
    let table = unsafe { &mut *(table_address as *mut [u64; NUM_OF_ENTRIES_IN_PAGE_TABLE]) };
    let mut index = (*virtual_address >> shift_level) & (NUM_OF_ENTRIES_IN_PAGE_TABLE - 1);
    if shift_level <= (PAGE_SHIFT as u8) + (3 - 1) * 9 {
        /* Create Block */
        if (table[index] & 0b11) != 0b00 {
            return Err(());
        }

        let attr = ((!permission.is_executable() as u64) << 54)
            | (1 << 10/* AF */)
            | (0b11 << 8/* Inner Shareable */)
            | (((!permission.is_writable() as u64) << 1) << 6)
            | unsafe { MAIR_INDEX as u64 } << 2
            | 0b01;

        while *size > 0 && index < NUM_OF_ENTRIES_IN_PAGE_TABLE {
            if *size < (1 << shift_level) {
                return Err(());
            }
            table[index] = *physical_address as u64 | attr;
            *physical_address += 1 << shift_level;
            *virtual_address += 1 << shift_level;
            index += 1;
            *size -= 1 << shift_level;
        }
        flush_data_cache();
        return Ok(());
    }
    while *size > 0 && index < NUM_OF_ENTRIES_IN_PAGE_TABLE {
        if table[index] & 0b11 != 0b11 {
            let next_table_address = alloc_pages(1).ok_or(())?;
            unsafe {
                core::ptr::write_bytes(
                    next_table_address as *mut u64,
                    0,
                    NUM_OF_ENTRIES_IN_PAGE_TABLE,
                )
            };
            flush_data_cache();
            table[index] = (next_table_address as u64) | 0b11;
        }
        let next_table_address = table[index] & (((1 << 48) - 1) ^ ((1 << 12) - 1));

        _associate_direct_map_address(
            next_table_address as usize,
            shift_level - 9,
            physical_address,
            virtual_address,
            size,
            permission,
            alloc_pages,
        )?;

        index += 1;
    }
    Ok(())
}

pub fn associate_direct_map_address(
    mut physical_address: usize,
    mut virtual_address: usize,
    permission: MemoryPermissionFlags,
    mut size: usize,
    alloc_pages: fn(usize) -> Option<usize>,
) -> Result<(), ()> {
    if virtual_address < TTBR1_EL1_START_ADDRESS {
        unimplemented!();
    }
    virtual_address -= u64::MAX as usize - ((1 << (64 - unsafe { MINIMUM_TXSZ })) - 1);
    _associate_direct_map_address(
        unsafe { TTBR1_EL1 } as usize,
        unsafe { TTBR1_INITIAL_SHIFT },
        &mut physical_address,
        &mut virtual_address,
        &mut size,
        permission,
        alloc_pages,
    )
}
