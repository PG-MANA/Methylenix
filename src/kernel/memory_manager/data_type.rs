/*
 * Memory Manager Data Type
 * basic data type for Memory Manager
 */

use arch::target_arch::paging::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};

use core::convert::{From, Into};
use core::ops::{Add, AddAssign, Sub, SubAssign};

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VAddress(usize);

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PAddress(usize);

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct MSize(usize);
type MOffset = MSize;

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct MOrder(usize);

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct MIndex(usize);

macro_rules! to_usize {
    ($t:ty) => {
        impl $t {
            pub const fn to_usize(&self) -> usize {
                self.0
            }
        }
    };
}

macro_rules! add_and_sub_with_m_size {
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
to_usize!(VAddress);
add_and_sub_with_m_size!(VAddress);
into_and_from_usize!(VAddress);

impl Sub<Self> for VAddress {
    type Output = MSize;
    fn sub(self, rhs: Self) -> Self::Output {
        MSize(self.0 - rhs.0)
    }
}

/* PAddress */
to_usize!(PAddress);
add_and_sub_with_m_size!(PAddress);
into_and_from_usize!(PAddress);

impl Sub<Self> for PAddress {
    type Output = MSize;
    fn sub(self, rhs: Self) -> Self::Output {
        MSize(self.0 - rhs.0)
    }
}

/* MSize */
to_usize!(MSize);
into_and_from_usize!(MSize);
add_and_sub_with_m_size!(MSize);

impl MSize {
    pub const fn from_address<T: Into<usize> + Ord>(start_address: T, end_address: T) -> Self {
        assert!(start_address <= end_address);
        Self(end_address.into() - start_address.into() + 1)
    }

    pub const fn to_end_address<T: From<usize> + Into<usize>>(&self, mut start_address: T) -> T {
        T::from(start_address.into() + self.0 - 1)
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
impl MOrder {
    pub const fn from_usize(order: usize) -> Self {
        Self(order)
    }

    pub const fn to_usize(&self) -> usize {
        self.0
    }

    pub fn from_offset(size: MSize) -> Self {
        if size <= MSize(PAGE_SIZE) {
            return Self(0);
        }
        let mut page_count = (((size.to_usize() - 1) & PAGE_MASK) >> PAGE_SHIFT) + 1;
        let mut order = if page_count & (page_count - 1) == 0 {
            0usize
        } else {
            1usize
        };
        while page_count != 0 {
            page_count >>= 1;
            order += 1;
        }
        MOrder(order)
    }

    pub const fn to_offset(&self) -> MSize {
        MSize(2 << self.0)
    }
}

/* MIndex */
impl MIndex {
    pub const fn from_offset(offset: MSize) -> Self {
        Self(offset.0 >> PAGE_SHIFT)
    }

    pub const fn to_offset(&self) -> MSize {
        //use core::usize;
        //assert!(*self <= Self::from_offset(MSize(usize::MAX)));
        MSize(self.0 << PAGE_SHIFT)
    }
}
