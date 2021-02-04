//!
//! Memory Manager Data Type
//!
//! This module has basic data types for Memory Manager.

use crate::arch::target_arch::paging::PAGE_SHIFT;

use core::convert::{From, Into};
use core::iter::Step;
use core::ops::{
    Add, AddAssign, BitAnd, BitOr, BitXor, Shl, ShlAssign, Shr, ShrAssign, Sub, SubAssign,
};

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VAddress(usize);

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PAddress(usize);

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct MSize(usize);
pub type MOffset = MSize;

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct MOrder(usize);

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct MPageOrder(usize);

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct MIndex(usize);

#[derive(Clone, Eq, PartialEq, Copy)]
pub struct MemoryPermissionFlags(u8);

#[derive(Clone, Eq, PartialEq, Copy)]
pub struct MemoryOptionFlags(u16);

pub trait Address:
    Copy
    + Clone
    + Ord
    + PartialOrd
    + Eq
    + PartialEq
    + Add<MSize>
    + AddAssign<MSize>
    + Sub<MSize>
    + SubAssign<MSize>
    + Into<usize>
    + From<usize>
    + BitAnd<usize>
    + BitOr<usize>
    + BitXor<usize>
{
    fn to_usize(&self) -> usize;
    fn is_zero(&self) -> bool;
}

macro_rules! address {
    ($t:ty) => {
        impl $t {
            pub const fn new(a: usize) -> Self {
                Self(a)
            }
        }
#[rustfmt::skip]
        impl const Address for $t {
            #[inline]
            fn to_usize(&self) -> usize {
                self.0
            }

            #[inline]
            fn is_zero(&self) -> bool {
                self.0 == 0
            }
        }
    };
}

macro_rules! address_bit_operation {
    ($t:ty) => {
#[rustfmt::skip]
        impl const BitAnd<usize> for $t {
            type Output = usize;
            fn bitand(self, rhs: usize) -> Self::Output {
                self.0 & rhs
            }
        }
#[rustfmt::skip]
        impl const BitOr<usize> for $t {
            type Output = usize;
            fn bitor(self, rhs: usize) -> Self::Output {
                self.0 | rhs
            }
        }
#[rustfmt::skip]
        impl const BitXor<usize> for $t {
            type Output = usize;
            fn bitxor(self, rhs: usize) -> Self::Output {
                self.0 ^ rhs
            }
        }
    };
}

macro_rules! to_usize {
    ($t:ty) => {
        impl $t {
            pub const fn to_usize(&self) -> usize {
                self.0
            }
        }
    };
}

macro_rules! add_and_sub_shift_with_m_size {
    ($t:ty) => {
#[rustfmt::skip]
        impl const Add<MSize> for $t {
            type Output = Self;
            #[inline]
            fn add(self, rhs: MSize) -> Self::Output {
                Self(self.0 + rhs.0)
            }
        }

#[rustfmt::skip]
        impl const AddAssign<MSize> for $t {
            #[inline]
            fn add_assign(&mut self, rhs: MSize) {
                self.0 += rhs.0;
            }
        }

#[rustfmt::skip]
        impl const Sub<MSize> for $t {
            type Output = Self;
            #[inline]
            fn sub(self, rhs: MSize) -> Self::Output {
                Self(self.0 - rhs.0)
            }
        }

#[rustfmt::skip]
        impl const SubAssign<MSize> for $t {
            #[inline]
            fn sub_assign(&mut self, rhs: MSize) {
                self.0 -= rhs.0;
            }
        }

#[rustfmt::skip]
        impl const Shr<MSize> for $t {
            type Output = Self;
            #[inline]
            fn shr(self, rhs: MSize) -> Self::Output {
                Self(self.0 >> rhs.0)
            }
        }

#[rustfmt::skip]
        impl const ShrAssign<MSize> for $t {
            #[inline]
            fn shr_assign(&mut self, rhs: MSize) {
                self.0 >>= rhs.0;
            }
        }

#[rustfmt::skip]
        impl const Shl<MSize> for $t {
            type Output = Self;
            #[inline]
            fn shl(self, rhs: MSize) -> Self::Output {
                Self(self.0 << rhs.0)
            }
        }

#[rustfmt::skip]
        impl const ShlAssign<MSize> for $t {
            #[inline]
            fn shl_assign(&mut self, rhs: MSize) {
                self.0 <<= rhs.0;
            }
        }
    };
}

macro_rules! into_and_from_usize {
    ($t:ty) => {
#[rustfmt::skip]
        impl const Into<usize> for $t {
            fn into(self) -> usize {
                self.0
            }
        }

#[rustfmt::skip]
        impl const From<usize> for $t {
            fn from(s: usize) -> Self {
                Self(s)
            }
        }
    };
}

/* VAddress */
address!(VAddress);
address_bit_operation!(VAddress);
add_and_sub_shift_with_m_size!(VAddress);
into_and_from_usize!(VAddress);

impl VAddress {
    /// Casting from VAddress to PAddress without mapping
    /// This is used to cast address when using direct map
    pub const fn to_direct_mapped_p_address(&self) -> PAddress {
        PAddress::new(self.0)
    }
}

#[rustfmt::skip]
impl<T: Sized> const Into<*mut T> for VAddress {
    fn into(self) -> *mut T {
        self.0 as usize as *mut T
    }
}

#[rustfmt::skip]
impl<T: Sized> const Into<*const T> for VAddress {
    fn into(self) -> *const T {
        self.0 as usize as *const T
    }
}

#[rustfmt::skip]
impl const Sub<Self> for VAddress {
    type Output = MSize;
    fn sub(self, rhs: Self) -> Self::Output {
        MSize(self.0 - rhs.0)
    }
}

/* PAddress */
address!(PAddress);
address_bit_operation!(PAddress);
add_and_sub_shift_with_m_size!(PAddress);
into_and_from_usize!(PAddress);

impl PAddress {
    /// Casting from VAddress to PAddress without mapping
    /// This is used to cast address when using direct map
    pub const fn to_direct_mapped_v_address(&self) -> VAddress {
        VAddress::new(self.0)
    }
}

#[rustfmt::skip]
impl const Sub<Self> for PAddress {
    type Output = MSize;
    fn sub(self, rhs: Self) -> Self::Output {
        MSize(self.0 - rhs.0)
    }
}

/* MSize */
to_usize!(MSize);
into_and_from_usize!(MSize);
address_bit_operation!(MSize);
add_and_sub_shift_with_m_size!(MSize);

impl MSize {
    pub const fn new(s: usize) -> Self {
        Self(s)
    }

    pub fn from_address<T: Address>(start_address: T, end_address: T) -> Self {
        assert!(start_address <= end_address);
        Self(end_address.to_usize() - start_address.to_usize() + 1)
    }

    pub fn to_end_address<T: Address>(self, mut start_address: T) -> T {
        start_address += self - MSize::new(1);
        start_address
    }

    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub const fn to_index(&self) -> MIndex {
        MIndex::from_offset(*self)
    }

    pub fn to_order(&self, max: Option<MOrder>) -> MOrder {
        MOrder::from_offset(*self, max.unwrap_or(MOrder::new(core::usize::MAX)))
    }
}

#[rustfmt::skip]
impl const Add<VAddress> for MSize {
    type Output = VAddress;
    fn add(self, rhs: VAddress) -> Self::Output {
        VAddress(self.0 + rhs.0)
    }
}

#[rustfmt::skip]
impl const Add<PAddress> for MSize {
    type Output = PAddress;
    fn add(self, rhs: PAddress) -> Self::Output {
        PAddress(self.0 + rhs.0)
    }
}

/* MOrder */
into_and_from_usize!(MOrder);
impl MOrder {
    pub const fn new(o: usize) -> Self {
        Self(o)
    }

    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub const fn to_usize(&self) -> usize {
        self.0
    }

    pub fn from_offset(size: MSize, max: MOrder) -> Self {
        let mut order = 0usize;
        let max_order = max.to_usize();
        while size > MSize::new(1 << order) {
            order += 1;
            if order >= max_order {
                return max;
            }
        }
        MOrder::new(order)
    }

    pub const fn to_offset(&self) -> MSize {
        MSize(1 << self.0)
    }

    pub const fn to_page_order(&self) -> MPageOrder {
        if self.0 > PAGE_SHIFT {
            MPageOrder::new(self.0 - PAGE_SHIFT)
        } else {
            MPageOrder::new(0)
        }
    }
}

/* MPageOrder */
into_and_from_usize!(MPageOrder);
impl MPageOrder {
    pub const fn new(o: usize) -> Self {
        Self(o)
    }

    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub const fn to_usize(&self) -> usize {
        self.0
    }

    pub fn from_offset(size: MSize, max: MPageOrder) -> Self {
        MOrder::from_offset(size, max.to_order()).to_page_order()
    }

    pub const fn to_offset(&self) -> MSize {
        self.to_order().to_offset()
    }

    pub const fn to_order(&self) -> MOrder {
        MOrder::new(self.0 + PAGE_SHIFT)
    }
}

/* MIndex */
into_and_from_usize!(MIndex);
impl MIndex {
    pub const fn new(i: usize) -> Self {
        Self(i)
    }

    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub const fn to_usize(&self) -> usize {
        self.0
    }

    pub const fn from_offset(offset: MSize) -> Self {
        Self(offset.0 >> PAGE_SHIFT)
    }

    pub const fn to_offset(&self) -> MSize {
        //use core::usize;
        //assert!(*self <= Self::from_offset(MSize(usize::MAX)));
        MSize(self.0 << PAGE_SHIFT)
    }
}

#[rustfmt::skip]
impl const Add for MIndex {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

#[rustfmt::skip]
impl const AddAssign for MIndex {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

#[rustfmt::skip]
impl const Sub for MIndex {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

#[rustfmt::skip]
impl const SubAssign for MIndex {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

unsafe impl Step for MIndex {
    #[inline]
    fn steps_between(start: &Self, end: &Self) -> Option<usize> {
        if start.0 <= end.0 {
            Some(end.0 - start.0)
        } else {
            None
        }
    }

    #[inline]
    fn forward_checked(start: Self, count: usize) -> Option<Self> {
        if let Some(n) = start.0.checked_add(count) {
            Some(n.into())
        } else {
            None
        }
    }

    #[inline]
    fn backward_checked(start: Self, count: usize) -> Option<Self> {
        if let Some(n) = start.0.checked_sub(count) {
            Some(n.into())
        } else {
            None
        }
    }
}

/* MemoryPermissionFlags */
impl MemoryPermissionFlags {
    pub const fn new(read: bool, write: bool, execute: bool, user_access: bool) -> Self {
        Self(
            ((read as u8) << 0)
                | ((write as u8) << 1)
                | ((execute as u8) << 2)
                | ((user_access as u8) << 3),
        )
    }

    pub const fn rodata() -> Self {
        Self::new(true, false, false, false)
    }

    pub const fn data() -> Self {
        Self::new(true, true, false, false)
    }

    pub fn is_readable(&self) -> bool {
        self.0 & (1 << 0) != 0
    }

    pub fn is_writable(&self) -> bool {
        self.0 & (1 << 1) != 0
    }

    pub fn is_executable(&self) -> bool {
        self.0 & (1 << 2) != 0
    }

    pub fn is_user_accessible(&self) -> bool {
        self.0 & (1 << 3) != 0
    }
}

/* MemoryOptionFlags */
impl MemoryOptionFlags {
    pub const NORMAL: Self = Self(0);
    pub const PRE_RESERVED: Self = Self(1 << 0);
    pub const DO_NOT_FREE_PHYSICAL_ADDRESS: Self = Self(1 << 1);
    pub const WIRED: Self = Self(1 << 2);
    pub const DEV_MAP: Self = Self(1 << 3);
    pub const DIRECT_MAP: Self = Self(1 << 4);

    pub fn is_pre_reserved(&self) -> bool {
        (*self & Self::PRE_RESERVED).0 != 0
    }

    pub fn should_not_free_phy_address(&self) -> bool {
        (*self & Self::DO_NOT_FREE_PHYSICAL_ADDRESS).0 != 0
    }

    pub fn is_wired(&self) -> bool {
        (*self & Self::WIRED).0 != 0
    }

    pub fn is_dev_map(&self) -> bool {
        (*self & Self::DEV_MAP).0 != 0
    }

    pub fn is_direct_mapped(&self) -> bool {
        (*self & Self::DIRECT_MAP).0 != 0
    }
}

impl const BitAnd<Self> for MemoryOptionFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl const BitOr<Self> for MemoryOptionFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl const BitXor<Self> for MemoryOptionFlags {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self::Output {
        Self(self.0 ^ rhs.0)
    }
}
