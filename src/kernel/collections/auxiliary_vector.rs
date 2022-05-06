//!
//! Auxiliary Vector
//!

#[repr(C)]
#[derive(Clone)]
pub struct AuxiliaryVector {
    pub aux_type: usize,
    pub value: usize,
}

pub const AT_NULL: usize = 0;
pub const AT_IGNORE: usize = 1;
