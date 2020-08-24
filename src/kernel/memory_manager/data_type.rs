/*
 * Memory Manager Data Type
 * basic data type for Memory Manager
 */

use arch::target_arch::paging::{PAGE_SHIFT, PAGE_SIZE};

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
pub struct MIndex(usize);

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
        impl Add<MSize> for $t {
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

impl<T: Sized> Into<*mut T> for VAddress {
    fn into(self) -> *mut T {
        self.0 as usize as *mut T
    }
}

impl<T: Sized> Into<*const T> for VAddress {
    fn into(self) -> *const T {
        self.0 as usize as *const T
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

impl PAddress {
    /// Casting from VAddress to PAddress without mapping
    /// This is used to cast address when using direct map
    pub const fn to_direct_mapped_v_address(&self) -> VAddress {
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

impl MSize {
    pub const fn new(s: usize) -> Self {
        Self(s)
    }

    pub fn from_address<T: Address>(start_address: T, end_address: T) -> Self {
        assert!(start_address <= end_address);
        Self(end_address.to_usize() - start_address.to_usize() + 1)
    }

    pub fn to_end_address<T: Address>(&self, mut start_address: T) -> T {
        T::from(start_address.to_usize() + self.0 - 1)
    }

    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub const fn to_index(&self) -> MIndex {
        MIndex::from_offset(*self)
    }

    pub fn to_order(&self, max: Option<MOrder>) -> MOrder {
        use core::usize;
        MOrder::from_offset(*self, max.unwrap_or(MOrder::new(usize::MAX)))
    }
}

impl Add<VAddress> for MSize {
    type Output = VAddress;
    fn add(self, rhs: VAddress) -> Self::Output {
        VAddress(self.0 + rhs.0)
    }
}

impl Add<PAddress> for MSize {
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

impl Add for MIndex {
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

unsafe impl Step for MIndex {
    #[inline]
    fn steps_between(start: &Self, end: &Self) -> Option<usize> {
        if start.0 <= end.0 {
            Some(end.0 - start.0)
        } else {
            None
        }
    }

    fn forward_checked(start: Self, count: usize) -> Option<Self> {
        if let Some(n) = start.0.checked_add(count) {
            Some(n.into())
        } else {
            None
        }
    }

    fn backward_checked(start: Self, count: usize) -> Option<Self> {
        if let Some(n) = start.0.checked_sub(count) {
            Some(n.into())
        } else {
            None
        }
    }
}
