//!
//! Data Structs and Macros
//!

pub mod auxiliary_vector;
pub mod fifo;
pub mod guid;
pub mod ptr_linked_list;
pub mod ring_buffer;

macro_rules! init_struct {
    ($st:expr_2021, $v:expr) => {
        core::mem::forget(core::mem::replace(&mut $st, $v))
    };
}

pub(crate) use init_struct;
