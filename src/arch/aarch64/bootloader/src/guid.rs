//!
//! Globally Unique Identifier
//!
//! This file is from kernel/collections/guid.rs

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Guid {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: [u8; 8],
}
impl core::fmt::Display for Guid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.d1,
            self.d2,
            self.d3,
            self.d4[0],
            self.d4[1],
            self.d4[2],
            self.d4[3],
            self.d4[4],
            self.d4[5],
            self.d4[6],
            self.d4[7]
        ))
    }
}
