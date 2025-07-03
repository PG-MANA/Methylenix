//!
//! Memory Manager Data Type
//!
//! This module has basic data types for Memory Manager.

use crate::arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};

use core::convert::Into;
use core::iter::Step;
use core::ops::{
    Add, AddAssign, BitAnd, BitOr, BitXor, Mul, Shl, ShlAssign, Shr, ShrAssign, Sub, SubAssign,
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

#[const_trait]
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
        impl BitAnd<usize> for $t {
            type Output = usize;
            fn bitand(self, rhs: usize) -> Self::Output {
                self.0 & rhs
            }
        }

        impl BitOr<usize> for $t {
            type Output = usize;
            fn bitor(self, rhs: usize) -> Self::Output {
                self.0 | rhs
            }
        }

        impl BitXor<usize> for $t {
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
        impl const Add<MSize> for $t {
            type Output = Self;
            #[inline]
            fn add(self, rhs: MSize) -> Self::Output {
                Self(self.0 + rhs.0)
            }
        }

        impl AddAssign<MSize> for $t {
            #[inline]
            fn add_assign(&mut self, rhs: MSize) {
                self.0 += rhs.0;
            }
        }

        impl Sub<MSize> for $t {
            type Output = Self;
            #[inline]
            fn sub(self, rhs: MSize) -> Self::Output {
                Self(self.0 - rhs.0)
            }
        }

        impl SubAssign<MSize> for $t {
            #[inline]
            fn sub_assign(&mut self, rhs: MSize) {
                self.0 -= rhs.0;
            }
        }

        impl Shr<MSize> for $t {
            type Output = Self;
            #[inline]
            fn shr(self, rhs: MSize) -> Self::Output {
                Self(self.0 >> rhs.0)
            }
        }

        impl ShrAssign<MSize> for $t {
            #[inline]
            fn shr_assign(&mut self, rhs: MSize) {
                self.0 >>= rhs.0;
            }
        }

        impl Shl<MSize> for $t {
            type Output = Self;
            #[inline]
            fn shl(self, rhs: MSize) -> Self::Output {
                Self(self.0 << rhs.0)
            }
        }

        impl ShlAssign<MSize> for $t {
            #[inline]
            fn shl_assign(&mut self, rhs: MSize) {
                self.0 <<= rhs.0;
            }
        }
    };
}

macro_rules! into_and_from_usize {
    ($t:ty) => {
        impl Into<usize> for $t {
            fn into(self) -> usize {
                self.0
            }
        }

        impl From<usize> for $t {
            fn from(s: usize) -> Self {
                Self(s)
            }
        }
    };
}

macro_rules! display {
    ($t:ty) => {
        impl core::fmt::Display for $t {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_fmt(format_args!("{:#X}", self.0))
            }
        }
    };
}

/* VAddress */
address!(VAddress);
address_bit_operation!(VAddress);
add_and_sub_shift_with_m_size!(VAddress);
into_and_from_usize!(VAddress);
display!(VAddress);

impl VAddress {
    /// Casting from VAddress to PAddress without any memory mapping.
    /// This is used to cast address when using direct memory mapping.
    pub const unsafe fn to_direct_mapped_p_address(&self) -> PAddress {
        PAddress::new(self.0)
    }

    pub const fn to<T>(self) -> *mut T {
        self.0 as *mut T
    }
}

impl<T: Sized> From<VAddress> for *mut T {
    fn from(val: VAddress) -> Self {
        val.to_usize() as *mut T
    }
}

impl<T: Sized> From<VAddress> for *const T {
    fn from(val: VAddress) -> Self {
        val.to_usize() as *const T
    }
}

impl<T: Sized> From<*const T> for VAddress {
    fn from(val: *const T) -> Self {
        VAddress::new(val as usize)
    }
}

impl<T: Sized> From<*mut T> for VAddress {
    fn from(val: *mut T) -> Self {
        VAddress::new(val as usize)
    }
}

impl Sub<Self> for VAddress {
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
display!(PAddress);

impl PAddress {
    /// Casting from PAddress to VAddress without any memory mapping.
    /// This is used to cast address when using direct memory mapping.
    pub const unsafe fn to_direct_mapped_v_address(&self) -> VAddress {
        VAddress::new(self.0)
    }
}

impl Sub<Self> for PAddress {
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
display!(MSize);

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
        MOrder::from_offset(*self, max.unwrap_or(MOrder::new(usize::MAX)))
    }

    pub const fn page_align_up(&self) -> Self {
        if self.is_zero() {
            return *self;
        }
        Self::new((self.0 - 1) & PAGE_MASK) + PAGE_SIZE
    }
}

impl const Add<VAddress> for MSize {
    type Output = VAddress;
    fn add(self, rhs: VAddress) -> Self::Output {
        VAddress(self.0 + rhs.0)
    }
}

impl const Add<PAddress> for MSize {
    type Output = PAddress;
    fn add(self, rhs: PAddress) -> Self::Output {
        PAddress(self.0 + rhs.0)
    }
}

impl Mul<Self> for MSize {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        MSize(self.0 * rhs.0)
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
display!(MPageOrder);
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
display!(MIndex);
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

impl const Add for MIndex {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for MIndex {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for MIndex {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for MIndex {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Step for MIndex {
    #[inline]
    fn steps_between(start: &Self, end: &Self) -> (usize, Option<usize>) {
        if start.0 <= end.0 {
            let r = end.0.overflowing_sub(start.0);
            if r.1 {
                (usize::MAX, None)
            } else {
                (r.0, Some(r.0))
            }
        } else {
            (0, None)
        }
    }

    #[inline]
    fn forward_checked(start: Self, count: usize) -> Option<Self> {
        start.0.checked_add(count).map(Self)
    }

    #[inline]
    fn backward_checked(start: Self, count: usize) -> Option<Self> {
        start.0.checked_sub(count).map(Self)
    }
}

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

    pub const fn rodata() -> Self {
        Self::new(true, false, false, false)
    }

    pub const fn data() -> Self {
        Self::new(true, true, false, false)
    }

    pub const fn user_data() -> Self {
        Self::new(true, true, false, true)
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
    pub const KERNEL: Self = Self(0);
    pub const USER: Self = Self(1 << 0);
    pub const PRE_RESERVED: Self = Self(1 << 1);
    pub const DO_NOT_FREE_PHYSICAL_ADDRESS: Self = Self(1 << 2);
    pub const WIRED: Self = Self(1 << 3); /* Disallow swap out */
    pub const IO_MAP: Self = Self(1 << 4);
    pub const ALLOC: Self = Self(1 << 5);
    pub const NO_WAIT: Self = Self(1 << 6);
    pub const CRITICAL: Self = Self(1 << 7);
    pub const DEVICE_MEMORY: Self = Self(1 << 8);
    pub const STACK: Self = Self(1 << 9);

    pub fn is_for_kernel(&self) -> bool {
        !self.is_for_user()
    }

    pub fn is_for_user(&self) -> bool {
        (*self & Self::USER).0 != 0
    }

    pub fn is_pre_reserved(&self) -> bool {
        (*self & Self::PRE_RESERVED).0 != 0
    }

    pub fn should_not_free_phy_address(&self) -> bool {
        (*self & Self::DO_NOT_FREE_PHYSICAL_ADDRESS).0 != 0
    }

    pub fn is_wired(&self) -> bool {
        (*self & Self::WIRED).0 != 0
    }

    pub fn is_io_map(&self) -> bool {
        (*self & Self::IO_MAP).0 != 0
    }

    pub fn is_alloc_area(&self) -> bool {
        (*self & Self::ALLOC).0 != 0
    }

    pub fn is_no_wait(&self) -> bool {
        (*self & Self::NO_WAIT).0 != 0
    }

    pub fn is_critical(&self) -> bool {
        (*self & Self::CRITICAL).0 != 0
    }

    pub fn is_device_memory(&self) -> bool {
        (*self & Self::DEVICE_MEMORY).0 != 0
    }

    pub fn is_stack(&self) -> bool {
        (*self & Self::STACK).0 != 0
    }
}

impl BitAnd<Self> for MemoryOptionFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitOr<Self> for MemoryOptionFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitXor<Self> for MemoryOptionFlags {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self::Output {
        Self(self.0 ^ rhs.0)
    }
}
