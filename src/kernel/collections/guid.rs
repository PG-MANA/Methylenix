//!
//! Globally Unique Identifier
//!

use core::fmt::Formatter;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Guid {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: [u8; 8],
}

impl Guid {
    pub const fn new(d1: u32, d2: u16, d3: u16, d4_high: u16, mut d4_low: u64) -> Self {
        let mut v = [0u8; 8];
        v[0] = (d4_high >> 8) as u8;
        v[1] = (d4_high & 0xff) as u8;
        let mut i = 7;
        while 1 < i {
            v[i] = (d4_low & 0xff) as u8;
            d4_low >>= 8;
            i -= 1;
        }
        Self { d1, d2, d3, d4: v }
    }

    pub const fn new_ne(d: [u8; 16]) -> Self {
        unsafe { core::mem::transmute::<[u8; 16], Self>(d) }
    }

    pub fn new_le(d: &[u8; 16]) -> Self {
        unsafe {
            Self {
                d1: u32::from_le(*core::mem::transmute::<&u8, &u32>(&d[0])),
                d2: u16::from_le(*core::mem::transmute::<&u8, &u16>(&d[4])),
                d3: u16::from_le(*core::mem::transmute::<&u8, &u16>(&d[6])),
                d4: *core::mem::transmute::<&u8, &[u8; 8]>(&d[8]),
            }
        }
    }
}

impl core::fmt::Display for Guid {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
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
