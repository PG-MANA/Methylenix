//use
//pub use self::pte::PTE;
//pub use self::pt::PageTable;

//mod pte;
//mod pt;

pub const PAGE_SIZE: usize = 4 * 1024;
pub const PAGE_MASK: usize = 0xFFFFFFFF_FFFFF000;