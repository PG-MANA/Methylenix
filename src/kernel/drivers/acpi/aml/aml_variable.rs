//!
//! AML Variable
//!

use super::data_object::ConstData;
use super::name_object::NameString;
use super::named_object::Method;
use super::{AcpiInt, AmlError};

use crate::arch::target_arch::device::acpi::{read_io, write_io};

use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
};
use crate::kernel::memory_manager::io_remap;
use crate::kernel::sync::spin_lock::Mutex;

use core::sync::atomic::{AtomicU8, Ordering};

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

pub type AmlFunction = fn(&[Arc<Mutex<AmlVariable>>]) -> Result<AmlVariable, AmlError>;

struct RecursiveIndex<'a> {
    index: usize,
    next: Option<&'a Self>,
}

#[derive(Debug, Clone)]
pub struct AmlPciConfig {
    pub bus: u16,
    pub device: u16,
    pub function: u16,
    pub offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone)]
pub struct AmlBitFiled {
    pub source: Arc<Mutex<AmlVariable>>,
    pub bit_index: usize,
    pub num_of_bits: usize,
    pub access_align: usize,
    pub should_lock_global_lock: bool,
}

#[derive(Debug, Clone)]
pub struct AmlByteFiled {
    pub source: Arc<Mutex<AmlVariable>>,
    pub byte_index: usize,
    pub num_of_bytes: usize,
    pub should_lock_global_lock: bool,
}

#[derive(Debug, Clone)]
pub struct AmlIndexField {
    pub index_register: Arc<Mutex<AmlVariable>>,
    pub data_register: Arc<Mutex<AmlVariable>>,
    pub bit_index: usize,
    pub num_of_bits: usize,
    pub access_align: usize,
    pub should_lock_global_lock: bool,
}

#[derive(Debug, Clone)]
pub enum AmlPackage {
    ConstData(ConstData),
    String(String),
    Buffer(Vec<u8>),
    NameString(NameString),
    Package(Vec<AmlPackage>),
}

#[derive(Clone)]
pub enum AmlVariable {
    Uninitialized,
    ConstData(ConstData),
    String(String),
    Buffer(Vec<u8>),
    Io((usize, usize)),
    MMIo((usize, usize)),
    EcIo((usize, usize)),
    PciConfig(AmlPciConfig),
    BitField(AmlBitFiled),
    ByteField(AmlByteFiled),
    IndexField(AmlIndexField),
    Package(Vec<AmlPackage>),
    Method(Method),
    BuiltInMethod((AmlFunction, u8)),
    Mutex(Arc<(AtomicU8, u8)>),
    Reference((Arc<Mutex<Self>>, Option<usize /* For Index Of */>)),
}

impl AmlVariable {
    fn _write(
        &mut self,
        data: Self,
        byte_index: usize,
        bit_index: usize,
        should_lock: bool,
        access_align: usize,
        num_of_bits: usize,
    ) -> Result<(), AmlError> {
        match self {
            Self::Io((port, limit)) => {
                if let Self::ConstData(c) = data {
                    let byte_offset = byte_index + (bit_index >> 3);
                    let adjusted_bit_index = bit_index & 0b111;
                    if byte_offset > *limit {
                        pr_err!(
                            "Offset({}) is out of I/O area(Port: {:#X}, Limit:{:#X}).",
                            byte_offset,
                            port,
                            limit
                        );
                        Err(AmlError::InvalidOperation)
                    } else {
                        write_io(*port + byte_offset, adjusted_bit_index, access_align, c)
                    }
                } else {
                    pr_err!("Writing {:?} into I/O({}) is invalid.", data, port);
                    Err(AmlError::InvalidOperation)
                }
            }
            Self::MMIo((address, limit)) => {
                if let Self::ConstData(c) = data {
                    let byte_offset = byte_index + (bit_index >> 3);
                    let adjusted_bit_index = bit_index & 0b111;
                    if byte_offset > *limit {
                        pr_err!(
                            "Offset({}) is out of Memory area(Address: {:#X}, Limit:{:#X}).",
                            byte_offset,
                            address,
                            limit
                        );
                        Err(AmlError::InvalidOperation)
                    } else {
                        Self::write_memory(
                            PAddress::new(*address + byte_offset),
                            adjusted_bit_index,
                            access_align,
                            c,
                            num_of_bits,
                        )
                    }
                } else {
                    pr_err!(
                        "Writing {:?} into Memory area({}) is invalid.",
                        data,
                        address
                    );
                    Err(AmlError::InvalidOperation)
                }
            }
            Self::EcIo((address, limit)) => {
                let adjusted_byte_index = byte_index + (bit_index >> 3);
                let adjusted_bit_index = bit_index & 0b111;
                if adjusted_byte_index >= *limit {
                    pr_err!(
                        "Offset({}) is out of Embedded Controller area(Address: {:#X}, Limit:{:#X}).",
                        adjusted_byte_index,
                        address,
                        limit
                    );
                    Err(AmlError::InvalidOperation)
                } else {
                    let to_write_data = if adjusted_bit_index != 0 || num_of_bits != 8 {
                        if num_of_bits == 0 || (num_of_bits + adjusted_bit_index) > 8 {
                            pr_err!(
                                "Invalid BitField: BitIndex: {}, NumOfBits: {}",
                                adjusted_bit_index,
                                num_of_bits
                            );
                            return Err(AmlError::InvalidOperation);
                        }
                        let original_data =
                            Self::read_embedded_controller((*address + adjusted_byte_index) as u8)?;
                        let mut bit_mask = 0;
                        for _ in 0..num_of_bits {
                            bit_mask <<= 1;
                            bit_mask |= 1;
                        }
                        (original_data & !(bit_mask << adjusted_bit_index))
                            | (((data.to_int()? as u8) & bit_mask) << adjusted_bit_index)
                    } else {
                        data.to_int()? as u8
                    };
                    Self::write_embedded_controller(
                        (*address + adjusted_byte_index) as u8,
                        to_write_data,
                    )
                }
            }
            Self::PciConfig(pci_config) => {
                if let AmlVariable::ConstData(c) = data {
                    let byte_offset = byte_index + (bit_index >> 3);
                    let adjusted_bit_index = bit_index % 8;
                    if byte_offset > pci_config.length {
                        pr_err!(
                            "Offset({}) is out of PciConfig Area({:?}).",
                            byte_offset,
                            pci_config
                        );
                        Err(AmlError::InvalidOperation)
                    } else {
                        Self::write_pci(
                            pci_config.clone(),
                            byte_offset,
                            adjusted_bit_index,
                            access_align,
                            num_of_bits,
                            c,
                        )?;
                        Ok(())
                    }
                } else {
                    pr_err!(
                        "Writing {:?} into Pci_Config({:?}) is invalid.",
                        data,
                        pci_config
                    );
                    Err(AmlError::InvalidOperation)
                }
            }
            Self::Uninitialized => {
                *self = data;
                Ok(())
            }

            Self::ConstData(_) | Self::String(_) => Err(AmlError::UnsupportedType),
            Self::Method(m) => {
                pr_err!("Writing data into Method({}) is invalid.", m.get_name());
                Err(AmlError::InvalidOperation)
            }
            Self::BuiltInMethod(_) => {
                pr_err!("Writing data into BuiltInMethod is invalid.");
                Err(AmlError::InvalidOperation)
            }
            Self::BitField(b_f) => b_f.source.try_lock().or(Err(AmlError::MutexError))?._write(
                data,
                byte_index,
                bit_index + b_f.bit_index,
                b_f.should_lock_global_lock | should_lock,
                b_f.access_align.max(access_align),
                b_f.num_of_bits,
            ),
            Self::ByteField(b_f) => b_f.source.try_lock().or(Err(AmlError::MutexError))?._write(
                data,
                byte_index + b_f.byte_index,
                bit_index,
                b_f.should_lock_global_lock | should_lock,
                b_f.num_of_bytes.max(access_align),
                b_f.num_of_bytes << 3,
            ),
            Self::IndexField(i_f) => {
                let byte_offset = byte_index + ((i_f.bit_index + bit_index) >> 3);
                let aligned_byte_offset = if i_f.access_align > 1 {
                    byte_offset & !(i_f.access_align - 1)
                } else {
                    byte_offset
                };
                let adjusted_bit_index =
                    (i_f.bit_index + bit_index) % 8 + (byte_offset - aligned_byte_offset);
                let index_data = match i_f.access_align {
                    0 | 1 => {
                        if aligned_byte_offset > u8::MAX as _ {
                            pr_err!("Index({}) is out of u8 range.", aligned_byte_offset);
                            return Err(AmlError::InvalidOperation);
                        }
                        AmlVariable::ConstData(ConstData::Byte(aligned_byte_offset as u8))
                    }
                    2 => {
                        if aligned_byte_offset > u16::MAX as _ {
                            pr_err!("Index({}) is out of u16 range.", aligned_byte_offset);
                            return Err(AmlError::InvalidOperation);
                        }
                        AmlVariable::ConstData(ConstData::Word(aligned_byte_offset as u16))
                    }
                    4 => {
                        if aligned_byte_offset > u32::MAX as _ {
                            pr_err!("Index({}) is out of u32 range.", aligned_byte_offset);
                            return Err(AmlError::InvalidOperation);
                        }
                        AmlVariable::ConstData(ConstData::DWord(aligned_byte_offset as u32))
                    }
                    8 => {
                        if aligned_byte_offset > u64::MAX as _ {
                            pr_err!("Index({}) is out of u64 range.", aligned_byte_offset);
                            return Err(AmlError::InvalidOperation);
                        }
                        AmlVariable::ConstData(ConstData::QWord(aligned_byte_offset as u64))
                    }
                    _ => {
                        pr_err!("Invalid Align.");
                        return Err(AmlError::InvalidOperation);
                    }
                };
                if let Ok(i) = i_f.index_register.try_lock() {
                    pr_info!("Index Register: {:?} <= {:?}", *i, index_data);
                }
                i_f.index_register
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    ._write(
                        index_data,
                        0,
                        0,
                        should_lock | i_f.should_lock_global_lock,
                        0,
                        8,
                    )?;

                i_f.data_register
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    ._write(
                        data,
                        0,
                        adjusted_bit_index,
                        should_lock | i_f.should_lock_global_lock,
                        i_f.access_align,
                        i_f.num_of_bits,
                    )?;

                Ok(())
            }
            Self::Buffer(b) => {
                let byte_offset = byte_index + (bit_index >> 3);
                let adjusted_bit_index = bit_index % 8;
                if (byte_offset + (num_of_bits >> 3)) >= b.len() {
                    pr_err!(
                        "Offset({}) is out of Buffer(Limit:{:#X}).",
                        byte_offset,
                        b.len()
                    );
                    return Err(AmlError::InvalidOperation);
                }
                let mut bit_mask = 0usize;
                for _ in 0..num_of_bits {
                    bit_mask <<= 1;
                    bit_mask |= 1;
                }
                let write_address = b.as_mut_ptr() as usize + byte_offset;

                match num_of_bits >> 3 {
                    1 => {
                        let mut original_data = unsafe { *(write_address as *const u8) };
                        original_data &= !(bit_mask as u8) << adjusted_bit_index;
                        original_data |=
                            ((data.to_int()? & (bit_mask as usize)) << adjusted_bit_index) as u8;
                        unsafe { *(write_address as *mut u8) = original_data };
                    }
                    2 => {
                        let mut original_data = unsafe { *(write_address as *const u16) };
                        original_data &= !(bit_mask as u16) << adjusted_bit_index;
                        original_data |=
                            ((data.to_int()? & (bit_mask as usize)) << adjusted_bit_index) as u16;
                        unsafe { *(write_address as *mut u16) = original_data };
                    }
                    4 => {
                        let mut original_data = unsafe { *(write_address as *const u32) };
                        original_data &= !(bit_mask as u32) << adjusted_bit_index;
                        original_data |=
                            ((data.to_int()? & (bit_mask as usize)) << adjusted_bit_index) as u32;
                        unsafe { *(write_address as *mut u32) = original_data };
                    }
                    8 => {
                        let mut original_data = unsafe { *(write_address as *const u64) };
                        original_data &= !(bit_mask as u64) << adjusted_bit_index;
                        original_data |=
                            ((data.to_int()? & (bit_mask as usize)) << adjusted_bit_index) as u64;
                        unsafe { *(write_address as *mut u64) = original_data };
                    }
                    _ => {
                        pr_err!("Invalid memory operation.");
                        return Err(AmlError::InvalidOperation);
                    }
                }
                Ok(())
            }
            Self::Package(_) => {
                pr_err!(
                    "Writing data({:?}) into Package({:?}) without index is invalid.",
                    data,
                    self
                );
                Err(AmlError::InvalidOperation)
            }
            Self::Mutex(_) => {
                pr_err!(
                    "Writing data({:?}) into Mutex({:?}) is invalid.",
                    data,
                    self
                );
                Err(AmlError::InvalidOperation)
            }
            Self::Reference((source, index)) => {
                if let Some(index) = index {
                    source
                        .try_lock()
                        .or(Err(AmlError::MutexError))?
                        .write_buffer_with_index(data, *index)
                } else {
                    source.try_lock().or(Err(AmlError::MutexError))?._write(
                        data,
                        byte_index,
                        bit_index,
                        should_lock,
                        access_align,
                        num_of_bits,
                    )
                }
            }
        }
    }

    pub fn is_constant_data(&self) -> bool {
        match self {
            Self::ConstData(_) => true,
            Self::String(_) => true,
            Self::Buffer(_) => true,
            Self::Io(_) => false,
            Self::MMIo(_) => false,
            Self::EcIo(_) => false,
            Self::PciConfig(_) => false,
            Self::BitField(_) => false,
            Self::ByteField(_) => false,
            Self::IndexField(_) => false,
            Self::Package(_) => true,
            Self::Uninitialized => true,
            Self::Method(_) => false,
            Self::BuiltInMethod(_) => false,
            Self::Mutex(_) => true,
            Self::Reference(_) => false,
        }
    }

    fn _read(
        &self,
        byte_index: usize,
        bit_index: usize,
        should_lock: bool,
        access_align: usize,
        num_of_bits: usize,
    ) -> Result<Self, AmlError> {
        match self {
            Self::Io((port, limit)) => {
                let byte_offset = byte_index + (bit_index >> 3);
                let adjusted_bit_index = bit_index % 8;
                if byte_offset > *limit {
                    pr_err!(
                        "Offset({}) is out of I/O area(port: {:#X}, Limit:{:#X}).",
                        byte_offset,
                        port,
                        limit
                    );
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok(Self::ConstData(read_io(
                        *port + byte_offset,
                        adjusted_bit_index,
                        access_align,
                        num_of_bits,
                    )?))
                }
            }
            Self::MMIo((address, limit)) => {
                let byte_offset = byte_index + (bit_index >> 3);
                let adjusted_bit_index = bit_index % 8;
                if byte_offset > *limit {
                    pr_err!(
                        "Offset({}) is out of Memory area(Address: {:#X}, Limit:{:#X}).",
                        byte_offset,
                        address,
                        limit
                    );
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok(Self::ConstData(Self::read_memory(
                        PAddress::new(*address + byte_offset),
                        adjusted_bit_index,
                        access_align,
                        num_of_bits,
                    )?))
                }
            }
            Self::EcIo((address, limit)) => {
                let adjusted_byte_index = byte_index + (bit_index >> 3);
                let adjusted_bit_index = bit_index & 0b111;
                if adjusted_byte_index >= *limit {
                    pr_err!(
                        "Offset({}) is out of Embedded Controller area(Address: {:#X}, Limit:{:#X}).",
                        adjusted_byte_index,
                        address,
                        limit
                    );
                    Err(AmlError::InvalidOperation)
                } else {
                    if num_of_bits == 0 || (num_of_bits + adjusted_bit_index) > 8 {
                        pr_err!(
                            "Invalid BitField: BitIndex: {}, NumOfBits: {}",
                            adjusted_bit_index,
                            num_of_bits
                        );
                        return Err(AmlError::InvalidOperation);
                    }
                    let mut read_data =
                        Self::read_embedded_controller((*address + adjusted_byte_index) as u8)?;
                    if adjusted_bit_index != 0 || num_of_bits != 8 {
                        let mut bit_mask: u8 = 0;
                        for _ in 0..num_of_bits {
                            bit_mask <<= 1;
                            bit_mask |= 1;
                        }
                        read_data = ((read_data) >> adjusted_bit_index) & bit_mask;
                    }
                    Ok(Self::ConstData(ConstData::Byte(read_data)))
                }
            }
            Self::PciConfig(pci_config) => {
                let byte_offset = byte_index + (bit_index >> 3);
                let adjusted_bit_index = bit_index % 8;
                if byte_offset > pci_config.length {
                    pr_err!(
                        "Offset({}) is out of PciConfig Area({:?}).",
                        byte_offset,
                        pci_config
                    );
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok(Self::ConstData(Self::read_pci(
                        pci_config.clone(),
                        byte_offset,
                        adjusted_bit_index,
                        access_align,
                        num_of_bits,
                    )?))
                }
            }
            Self::ConstData(_)
            | Self::Uninitialized
            | Self::Mutex(_)
            | Self::Method(_)
            | Self::BuiltInMethod(_) => Ok(self.clone()),
            Self::String(_) | Self::Buffer(_) | Self::Package(_) => {
                let adjusted_byte_index = byte_index + (bit_index >> 3);
                let adjusted_bit_index = bit_index - ((bit_index >> 3) << 3);
                if adjusted_bit_index != 0 {
                    pr_err!(
                        "Reading String, Buffer, or Package with bit_index({}) is invalid.",
                        adjusted_bit_index
                    );
                    Err(AmlError::InvalidOperation)
                } else if adjusted_byte_index != 0 {
                    self.read_buffer_with_index(adjusted_byte_index)
                } else {
                    Ok(self.clone())
                }
            }
            Self::BitField(b_f) => b_f.source.try_lock().or(Err(AmlError::MutexError))?._read(
                byte_index,
                bit_index + b_f.bit_index,
                b_f.should_lock_global_lock | should_lock,
                b_f.access_align.max(access_align),
                b_f.num_of_bits,
            ),
            Self::ByteField(b_f) => b_f.source.try_lock().or(Err(AmlError::MutexError))?._read(
                byte_index + b_f.byte_index,
                bit_index,
                b_f.should_lock_global_lock | should_lock,
                b_f.num_of_bytes.max(access_align),
                b_f.num_of_bytes << 3,
            ),
            Self::IndexField(i_f) => {
                let byte_offset = byte_index + ((i_f.bit_index + bit_index) >> 3);
                let aligned_byte_offset = if i_f.access_align > 1 {
                    byte_offset & !(i_f.access_align - 1)
                } else {
                    byte_offset
                };
                let adjusted_bit_index =
                    (i_f.bit_index + bit_index) % 8 + (byte_offset - aligned_byte_offset);

                let index_data = match i_f.access_align {
                    0 | 1 => {
                        if aligned_byte_offset > u8::MAX as _ {
                            pr_err!("Index({}) is out of u8 range.", aligned_byte_offset);
                            return Err(AmlError::InvalidOperation);
                        }
                        AmlVariable::ConstData(ConstData::Byte(aligned_byte_offset as u8))
                    }
                    2 => {
                        if aligned_byte_offset > u16::MAX as _ {
                            pr_err!("Index({}) is out of u16 range.", aligned_byte_offset);
                            return Err(AmlError::InvalidOperation);
                        }
                        AmlVariable::ConstData(ConstData::Word(aligned_byte_offset as u16))
                    }
                    4 => {
                        if aligned_byte_offset > u32::MAX as _ {
                            pr_err!("Index({}) is out of u32 range.", aligned_byte_offset);
                            return Err(AmlError::InvalidOperation);
                        }
                        AmlVariable::ConstData(ConstData::DWord(aligned_byte_offset as u32))
                    }
                    8 => {
                        if aligned_byte_offset > u64::MAX as _ {
                            pr_err!("Index({}) is out of u64 range.", aligned_byte_offset);
                            return Err(AmlError::InvalidOperation);
                        }
                        AmlVariable::ConstData(ConstData::QWord(aligned_byte_offset as u64))
                    }
                    _ => {
                        pr_err!("Invalid Align.");
                        return Err(AmlError::InvalidOperation);
                    }
                };

                i_f.index_register
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    ._write(
                        index_data,
                        0,
                        0,
                        should_lock | i_f.should_lock_global_lock,
                        0,
                        8,
                    )?;
                let data = i_f
                    .data_register
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    ._read(
                        0,
                        adjusted_bit_index,
                        should_lock | i_f.should_lock_global_lock,
                        i_f.access_align,
                        i_f.num_of_bits,
                    )?;
                Ok(data)
            }
            Self::Reference((source, index)) => {
                if let Some(index) = index {
                    source
                        .try_lock()
                        .or(Err(AmlError::MutexError))?
                        .read_buffer_with_index(*index)
                } else {
                    source.try_lock().or(Err(AmlError::MutexError))?._read(
                        byte_index,
                        bit_index,
                        should_lock,
                        access_align,
                        num_of_bits,
                    )
                }
            }
        }
    }

    pub fn get_constant_data(&self) -> Result<Self, AmlError> {
        match self {
            Self::Uninitialized
            | Self::ConstData(_)
            | Self::String(_)
            | Self::Buffer(_)
            | Self::Mutex(_)
            | Self::Package(_) => Ok(self.clone()),
            Self::Io(_)
            | Self::MMIo(_)
            | Self::EcIo(_)
            | Self::PciConfig(_)
            | Self::BitField(_)
            | Self::ByteField(_)
            | Self::IndexField(_)
            | Self::Reference(_) => self._read(0, 0, false, 0, 0),
            Self::Method(m) => {
                pr_err!("Reading Method({}) is invalid.", m.get_name());
                Err(AmlError::InvalidOperation)
            }
            Self::BuiltInMethod(_) => {
                pr_err!("Reading BuiltInMethod is invalid.");
                Err(AmlError::InvalidOperation)
            }
        }
    }

    pub fn write(&mut self, data: Self) -> Result<(), AmlError> {
        let constant_data = if data.is_constant_data() {
            data
        } else {
            data.get_constant_data()?
        };
        if self.is_constant_data() {
            *self = constant_data;
            Ok(())
        } else {
            self._write(constant_data, 0, 0, false, 0, 1 /*Is it ok?*/)
        }
    }

    fn write_data_into_package_recursively(
        &mut self,
        index: &RecursiveIndex,
        data: AmlPackage,
    ) -> Result<(), AmlError> {
        if let Self::Package(v) = self {
            let mut deref_index = Some(index);
            let mut v = v;
            while let Some(i) = deref_index {
                if v.len() <= i.index {
                    pr_err!("Index({}) is out of buffer(len: {}).", i.index, v.len());
                    return Err(AmlError::InvalidOperation);
                }
                if i.next.is_none() {
                    v[i.index] = data;
                    return Ok(());
                }
                if let AmlPackage::Package(next) = &mut v[i.index] {
                    v = next;
                } else {
                    return Err(AmlError::InvalidType);
                }
                deref_index = i.next;
            }
            unreachable!()
        } else if let Self::Reference((source, additional_index)) = self {
            if let Some(additional) = additional_index {
                let new_index = RecursiveIndex {
                    index: *additional,
                    next: Some(index),
                };
                source
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    .write_data_into_package_recursively(&new_index, data)
            } else {
                source
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    .write_data_into_package_recursively(index, data)
            }
        } else {
            pr_err!("Expected a package, but found {:?}", self);
            Err(AmlError::InvalidType)
        }
    }

    pub fn write_buffer_with_index(&mut self, data: Self, index: usize) -> Result<(), AmlError> {
        if let Self::Buffer(s) = self {
            let const_data = if data.is_constant_data() {
                data
            } else {
                data.get_constant_data()?
            };
            if let Self::ConstData(ConstData::Byte(byte)) = const_data {
                if s.len() <= index {
                    pr_err!("index({}) is out of buffer(len: {}).", index, s.len());
                    return Err(AmlError::InvalidOperation);
                }
                s[index] = byte;
                return Ok(());
            }
        } else if let Self::Package(v) = self {
            if index < v.len() {
                v[index] = data.convert_to_aml_package()?;
                return Ok(());
            } else {
                pr_err!("index({}) is out of package(len: {}).", index, v.len());
            }
        } else if let Self::String(s) = self {
            if index < s.len() {
                unsafe { s.as_bytes_mut()[index] = data.to_int()? as u8 };
                return Ok(());
            } else {
                pr_err!("index({}) is out of string(len: {}).", index, s.len());
            }
        } else if let Self::Reference((source, additional_index)) = self {
            return if additional_index.is_some() {
                let rec_index = RecursiveIndex { index, next: None };
                if let Err(e) = self
                    .write_data_into_package_recursively(&rec_index, data.convert_to_aml_package()?)
                {
                    pr_err!("Invalid Package: (Self: {:?}, Index: {})", self, index);
                    Err(e)
                } else {
                    Ok(())
                }
            } else {
                source
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    .write_buffer_with_index(data, index)
            };
        } else {
            pr_err!("Invalid Data Type: {:?} <- {:?}", self, data);
        }
        return Err(AmlError::InvalidOperation);
    }

    pub fn read_buffer_with_index(&self, index: usize) -> Result<Self, AmlError> {
        if let Self::Buffer(s) = self {
            if index < s.len() {
                Ok(Self::ConstData(ConstData::Byte(s[index])))
            } else {
                pr_err!("index({}) is out of buffer(len: {}).", index, s.len());
                Err(AmlError::InvalidOperation)
            }
        } else if let Self::Package(v) = self {
            if index < v.len() {
                Self::from_aml_package(v[index].clone())
            } else {
                pr_err!("index({}) is out of package(len: {}).", index, v.len());
                Err(AmlError::InvalidOperation)
            }
        } else if let Self::String(s) = self {
            if index < s.len() {
                Ok(Self::ConstData(ConstData::Byte(s.as_bytes()[index])))
            } else {
                pr_err!("index({}) is out of string(len: {}).", index, s.len());
                Err(AmlError::InvalidOperation)
            }
        } else if let Self::Reference((source, additional_index)) = self {
            if let Some(additional) = additional_index {
                if let AmlVariable::Package(package) = source
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    .read_buffer_with_index(*additional)?
                {
                    if index < package.len() {
                        Self::from_aml_package(package[index].clone())
                    } else {
                        pr_err!(
                            "index({}) is out of package(len: {}).",
                            index,
                            package.len()
                        );
                        Err(AmlError::InvalidOperation)
                    }
                } else {
                    pr_err!("Invalid Reference Data Type: {:?}", self);
                    Err(AmlError::InvalidOperation)
                }
            } else {
                source
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    .read_buffer_with_index(index)
            }
        } else {
            pr_err!("Invalid Data Type: {:?}[{}]", self, index);
            Err(AmlError::InvalidOperation)
        }
    }

    pub fn to_int(&self) -> Result<AcpiInt, AmlError> {
        match self {
            Self::ConstData(c) => Ok(c.to_int()),
            Self::String(_) => Err(AmlError::InvalidType),
            Self::Buffer(_) => Err(AmlError::InvalidType),
            Self::Io(_)
            | Self::MMIo(_)
            | Self::EcIo(_)
            | Self::PciConfig(_)
            | Self::BitField(_)
            | Self::ByteField(_)
            | Self::IndexField(_)
            | Self::Package(_)
            | Self::Reference(_) => self.get_constant_data()?.to_int(),
            Self::Uninitialized => Err(AmlError::InvalidType),
            Self::Method(_) => Err(AmlError::InvalidType),
            Self::BuiltInMethod(_) => Err(AmlError::InvalidType),
            Self::Mutex(d) => Ok(d.0.load(Ordering::Relaxed) as usize),
        }
    }

    pub fn get_byte_size(&self) -> Result<usize, AmlError> {
        match self {
            Self::ConstData(c) => Ok(c.get_byte_size()),
            Self::String(s) => Ok(s.len()),
            Self::Buffer(b) => Ok(b.len()),
            Self::Io(_)
            | Self::MMIo(_)
            | Self::EcIo(_)
            | Self::PciConfig(_)
            | Self::BitField(_)
            | Self::ByteField(_)
            | Self::IndexField(_)
            | Self::Package(_) => self.get_constant_data()?.get_byte_size(),
            Self::Uninitialized => Err(AmlError::InvalidType),
            Self::Method(_) => Err(AmlError::InvalidType),
            Self::BuiltInMethod(_) => Err(AmlError::InvalidType),
            Self::Mutex(_) => Err(AmlError::InvalidType),
            Self::Reference((_, index)) => {
                if index.is_some() {
                    Ok(8) /*Vec<u8>*/
                } else {
                    self.get_constant_data()?.get_byte_size()
                }
            }
        }
    }

    fn from_aml_package(p: AmlPackage) -> Result<Self, AmlError> {
        match p {
            AmlPackage::ConstData(c) => Ok(Self::ConstData(c)),
            AmlPackage::String(s) => Ok(Self::String(s)),
            AmlPackage::Buffer(b) => Ok(Self::Buffer(b)),
            AmlPackage::NameString(_) => Err(AmlError::InvalidType),
            AmlPackage::Package(child_p) => Ok(Self::Package(child_p)),
        }
    }

    fn convert_to_aml_package(self) -> Result<AmlPackage, AmlError> {
        match self {
            Self::Uninitialized => Err(AmlError::InvalidType),
            Self::ConstData(c) => Ok(AmlPackage::ConstData(c)),
            Self::String(s) => Ok(AmlPackage::String(s)),
            Self::Buffer(b) => Ok(AmlPackage::Buffer(b)),
            Self::Io(_)
            | Self::MMIo(_)
            | Self::EcIo(_)
            | Self::PciConfig(_)
            | Self::BitField(_)
            | Self::ByteField(_)
            | Self::IndexField(_)
            | Self::Reference(_) => self.get_constant_data()?.convert_to_aml_package(),
            Self::Package(p) => Ok(AmlPackage::Package(p)),
            Self::Mutex(_) => Err(AmlError::InvalidType),
            Self::Method(_) => Err(AmlError::InvalidType),
            Self::BuiltInMethod(_) => Err(AmlError::InvalidType),
        }
    }

    fn read_memory(
        address: PAddress,
        bit_index: usize,
        align: usize,
        num_of_bits: usize,
    ) -> Result<ConstData, AmlError> {
        let size = MSize::new(((bit_index + num_of_bits) >> 3).max(1));
        let virtual_address = io_remap!(
            address,
            size,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS
        )
        .or_else(|e| {
            pr_err!(
                "Failed to io_map(PhysicalAddress: {}, Size: {}): {:?}",
                address,
                size,
                e
            );
            Err(AmlError::InvalidOperation)
        })?;
        let result = try {
            unsafe {
                match align {
                    0 | 1 => {
                        let mut bit_mask = 0;
                        for _ in 0..num_of_bits {
                            bit_mask <<= 1;
                            bit_mask |= 1;
                        }
                        let data =
                            core::ptr::read_volatile(virtual_address.to_usize() as *const u8);
                        ConstData::Byte((data >> bit_index) & bit_mask)
                    }
                    2 => {
                        let aligned_address = virtual_address.to_usize() & !0b1;
                        let mut bit_mask = 0;
                        for _ in 0..num_of_bits {
                            bit_mask <<= 1;
                            bit_mask |= 1;
                        }
                        let data = core::ptr::read_volatile(aligned_address as *const u16);
                        ConstData::Word(
                            data >> ((((virtual_address.to_usize() - aligned_address) << 3)
                                + bit_index)
                                & bit_mask),
                        )
                    }
                    4 => {
                        let aligned_address = virtual_address.to_usize() & !0b11;
                        let mut bit_mask = 0;
                        for _ in 0..num_of_bits {
                            bit_mask <<= 1;
                            bit_mask |= 1;
                        }
                        let data = core::ptr::read_volatile(aligned_address as *const u32);
                        ConstData::DWord(
                            data >> ((((virtual_address.to_usize() - aligned_address) << 3)
                                + bit_index)
                                & bit_mask),
                        )
                    }
                    8 => {
                        let aligned_address = virtual_address.to_usize() & !0b111;
                        let mut bit_mask = 0;
                        for _ in 0..num_of_bits {
                            bit_mask <<= 1;
                            bit_mask |= 1;
                        }
                        let data = core::ptr::read_volatile(aligned_address as *const u64);
                        ConstData::QWord(
                            data >> ((((virtual_address.to_usize() - aligned_address) << 3)
                                + bit_index)
                                & bit_mask),
                        )
                    }
                    _ => {
                        pr_err!("Invalid memory operation.");
                        Err(AmlError::InvalidOperation)?
                    }
                }
            }
        };
        if let Err(e) = get_kernel_manager_cluster()
            .kernel_memory_manager
            .free(virtual_address)
        {
            pr_warn!("Failed to free memory map: {:?}", e);
        }
        pr_debug!(
            "Read (Address: {}[VirtualAddress: {}], BitIndex: {}, NumOfBits: {}) => {:?}",
            address,
            virtual_address,
            bit_index,
            num_of_bits,
            result
        );
        return result;
    }

    fn write_memory(
        address: PAddress,
        bit_index: usize,
        align: usize,
        data: ConstData,
        num_of_bits: usize,
    ) -> Result<(), AmlError> {
        let access_size = (match data {
            ConstData::Byte(_) => 1,
            ConstData::Word(_) => 2,
            ConstData::DWord(_) => 4,
            ConstData::QWord(_) => 8,
        })
        .max(align);

        let size = MSize::new(access_size);
        let virtual_address = io_remap!(
            address,
            size,
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS
        )
        .or_else(|e| {
            pr_err!(
                "Failed to io_map(PhysicalAddress: {:#X}, Size: {:#X}): {:?}",
                address.to_usize(),
                size.to_usize(),
                e
            );
            Err(AmlError::InvalidOperation)
        })?;
        pr_debug!(
            "Write (Address: {}[VirtualAddress: {}], BitIndex: {}, NumOfBits: {}) <= {:?}(AccessSize: {})",
            address,virtual_address,
            bit_index,
            num_of_bits,
            data,
            access_size
        );
        let result = try {
            unsafe {
                match align {
                    0 | 1 => {
                        let mut bit_mask = 0;
                        for _ in 0..num_of_bits {
                            bit_mask <<= 1;
                            bit_mask |= 1;
                        }
                        let mut original_data =
                            core::ptr::read_volatile(virtual_address.to_usize() as *const u8);
                        original_data &= !bit_mask << bit_index;
                        original_data |= ((data.to_int() & (bit_mask as usize)) << bit_index) as u8;
                        core::ptr::write_volatile(
                            virtual_address.to_usize() as *mut u8,
                            original_data,
                        );
                    }
                    2 => {
                        let aligned_address = virtual_address.to_usize() & !0b1;
                        let mut bit_mask = 0;
                        for _ in 0..num_of_bits {
                            bit_mask <<= 1;
                            bit_mask |= 1;
                        }
                        let mut original_data =
                            core::ptr::read_volatile(aligned_address as *const u16);
                        original_data &= !bit_mask
                            << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index);
                        original_data |= ((data.to_int() & (bit_mask as usize))
                            << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index))
                            as u16;
                        core::ptr::write_volatile(aligned_address as *mut u16, original_data);
                    }
                    4 => {
                        let aligned_address = virtual_address.to_usize() & !0b11;
                        let mut bit_mask = 0;
                        for _ in 0..num_of_bits {
                            bit_mask <<= 1;
                            bit_mask |= 1;
                        }
                        let mut original_data =
                            core::ptr::read_volatile(aligned_address as *const u32);
                        original_data &= !bit_mask
                            << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index);
                        original_data |= ((data.to_int() & (bit_mask as usize))
                            << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index))
                            as u32;
                        core::ptr::write_volatile(aligned_address as *mut u32, original_data);
                    }
                    8 => {
                        let aligned_address = virtual_address.to_usize() & !0b111;
                        let mut bit_mask = 0;
                        for _ in 0..num_of_bits {
                            bit_mask <<= 1;
                            bit_mask |= 1;
                        }
                        let mut original_data =
                            core::ptr::read_volatile(aligned_address as *const u64);
                        original_data &= !bit_mask
                            << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index);
                        original_data |= ((data.to_int() & (bit_mask as usize))
                            << (((virtual_address.to_usize() - aligned_address) << 3) + bit_index))
                            as u64;
                        core::ptr::write_volatile(aligned_address as *mut u64, original_data);
                    }
                    _ => {
                        pr_err!("Invalid memory operation.");
                        Err(AmlError::InvalidOperation)?
                    }
                }
            }
        };
        if let Err(e) = get_kernel_manager_cluster()
            .kernel_memory_manager
            .free(virtual_address)
        {
            pr_warn!("Failed to free memory map: {:?}", e);
        }
        return result;
    }

    fn read_pci(
        pci_config: AmlPciConfig,
        byte_index: usize,
        bit_index: usize,
        align: usize,
        num_of_bits: usize,
    ) -> Result<ConstData, AmlError> {
        let offset = pci_config.offset + byte_index;
        let aligned_offset = (offset & !0b11) & !(align.max(1) - 1);
        let bit_offset_base = (offset - aligned_offset) << 3;
        let bit_offset = bit_index + bit_offset_base;

        if bit_offset >= 32 {
            pr_err!("Invalid BitOffset: ({} + {})", bit_index, bit_offset_base);
            Err(AmlError::InvalidOperation)?;
        } else if num_of_bits > 64 {
            pr_err!("Unsupported BitMask({})", num_of_bits);
            Err(AmlError::InvalidOperation)?;
        }

        let mut bit_mask = 0;
        for _ in 0..num_of_bits {
            bit_mask <<= 1;
            bit_mask |= 1;
        }
        let mut result = 0u64;
        let first_byte_data = get_kernel_manager_cluster()
            .pci_manager
            .read_data_by_device_number(
                pci_config.bus as _,
                pci_config.device as _,
                pci_config.function as _,
                aligned_offset as _,
                4,
            )
            .or(Err(AmlError::InvalidOperation))?;

        result |= ((first_byte_data >> bit_offset) as u64) & bit_mask;
        let mut index = 1;
        bit_mask >>= 32 - bit_offset;

        while bit_mask != 0 {
            result |= get_kernel_manager_cluster()
                .pci_manager
                .read_data_by_device_number(
                    pci_config.bus as _,
                    pci_config.device as _,
                    pci_config.function as _,
                    (aligned_offset + index) as _,
                    4,
                )
                .or(Err(AmlError::InvalidOperation))? as u64;
            index += 1;
            bit_mask >>= 32;
        }

        pr_debug!(
            "Read PCI: {}:{}:{} offset: {}(bit_index: {}) => {}",
            pci_config.bus,
            pci_config.device,
            pci_config.function,
            aligned_offset,
            bit_offset,
            result
        );

        if num_of_bits <= 8 {
            Ok(ConstData::Byte(result as _))
        } else if num_of_bits <= 16 {
            Ok(ConstData::Word(result as _))
        } else if num_of_bits <= 32 {
            Ok(ConstData::DWord(result as _))
        } else {
            Ok(ConstData::QWord(result as _))
        }
    }

    fn write_pci(
        pci_config: AmlPciConfig,
        byte_index: usize,
        bit_index: usize,
        align: usize,
        num_of_bits: usize,
        data: ConstData,
    ) -> Result<(), AmlError> {
        if pci_config.function >= 8 {
            pr_err!("Function({}) must be less than 8.", pci_config.function);
            return Err(AmlError::InvalidOperation);
        }
        let offset = pci_config.offset + byte_index;
        let aligned_offset = (offset & !0b11) & !(align.max(1) - 1);
        let bit_offset_base = (offset - aligned_offset) << 3;
        let bit_offset = bit_index + bit_offset_base;

        if bit_offset >= 32 {
            pr_err!("Invalid BitOffset: ({} + {})", bit_index, bit_offset_base);
            Err(AmlError::InvalidOperation)?;
        } else if num_of_bits > 64 {
            pr_err!("Unsupported BitMask({})", num_of_bits);
            Err(AmlError::InvalidOperation)?;
        }

        pr_debug!(
            "Write PCI: {}:{}:{} offset: {}(bit_index: {}) <= {:?}",
            pci_config.bus,
            pci_config.device,
            pci_config.function,
            aligned_offset,
            bit_offset,
            data
        );

        let mut bit_mask = 0usize;
        for _ in 0..num_of_bits {
            bit_mask <<= 1;
            bit_mask |= 1;
        }
        let mut write_data = data.to_int();
        let first_byte_data = get_kernel_manager_cluster()
            .pci_manager
            .read_data_by_device_number(
                pci_config.bus as _,
                pci_config.device as _,
                pci_config.function as _,
                aligned_offset as _,
                4,
            )
            .or(Err(AmlError::InvalidOperation))?;
        let buffer = (first_byte_data & !(bit_mask << bit_offset) as u32)
            | ((write_data & bit_mask) << bit_offset) as u32;
        get_kernel_manager_cluster()
            .pci_manager
            .write_data_by_device_number(
                pci_config.bus as _,
                pci_config.device as _,
                pci_config.function as _,
                aligned_offset as _,
                buffer,
            )
            .or(Err(AmlError::InvalidOperation))?;

        let mut index = 1;
        bit_mask >>= 32 - bit_offset;
        write_data >>= 32 - bit_offset;

        while bit_mask != 0 {
            let read_data = get_kernel_manager_cluster()
                .pci_manager
                .read_data_by_device_number(
                    pci_config.bus as _,
                    pci_config.device as _,
                    pci_config.function as _,
                    (aligned_offset + index) as _,
                    4,
                )
                .or(Err(AmlError::InvalidOperation))?;
            let buffer =
                (read_data & !(bit_mask as u32)) | (write_data as u32 & bit_mask as u32) as u32;
            get_kernel_manager_cluster()
                .pci_manager
                .write_data_by_device_number(
                    pci_config.bus as _,
                    pci_config.device as _,
                    pci_config.function as _,
                    (aligned_offset + index) as _,
                    buffer,
                )
                .or(Err(AmlError::InvalidOperation))?;
            index += 1;
            bit_mask >>= 32;
            write_data >>= 32;
        }

        return Ok(());
    }

    fn read_embedded_controller(address: u8) -> Result<u8, AmlError> {
        if let Some(ec) = get_kernel_manager_cluster()
            .acpi_device_manager
            .get_embedded_controller()
        {
            Ok(ec.read_data(address))
        } else {
            pr_err!("Embedded Controller is not available.");
            Err(AmlError::InvalidOperation)
        }
    }

    fn write_embedded_controller(address: u8, data: u8) -> Result<(), AmlError> {
        if let Some(ec) = get_kernel_manager_cluster()
            .acpi_device_manager
            .get_embedded_controller()
        {
            ec.write_data(address, data);
            Ok(())
        } else {
            pr_err!("Embedded Controller is not available.");
            Err(AmlError::InvalidOperation)
        }
    }
}

impl core::fmt::Debug for AmlVariable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AmlVariable::Uninitialized => f.write_str("Uninitialized"),
            AmlVariable::ConstData(c) => f.write_fmt(format_args!("ConstantData({})", c.to_int())),
            AmlVariable::String(s) => f.write_fmt(format_args!("String(\"{}\")", s)),
            AmlVariable::Buffer(b) => f.write_fmt(format_args!("Buffer({:?})", b)),
            AmlVariable::Io((port, limit)) => f
                .debug_struct("SystemI/O")
                .field("Port", &format_args!("{:#X}", port))
                .field("Limit", &format_args!("{:#X}", limit))
                .finish(),
            AmlVariable::MMIo((port, limit)) => f
                .debug_struct("MemoryI/O")
                .field("Port", &format_args!("{:#X}", port))
                .field("Limit", &format_args!("{:#X}", limit))
                .finish(),
            AmlVariable::EcIo((port, limit)) => f
                .debug_struct("EmbeddedControllerI/O")
                .field("Port", &format_args!("{:#X}", port))
                .field("Limit", &format_args!("{:#X}", limit))
                .finish(),
            AmlVariable::PciConfig(p) => f.write_fmt(format_args!("PCI_Config({:?})", p)),
            AmlVariable::BitField(b) => {
                if let Ok(s) = b.source.try_lock() {
                    f.debug_struct("BitField")
                        .field("Source", &*s)
                        .field("BitIndex", &b.bit_index)
                        .field("NumberOfBits", &b.num_of_bits)
                        .field("AccessAlign", &b.access_align)
                        .field("GlobalLock", &b.should_lock_global_lock)
                        .finish()
                } else {
                    f.debug_struct("BitField")
                        .field("BitIndex", &b.bit_index)
                        .field("NumberOfBits", &b.num_of_bits)
                        .field("AccessAlign", &b.access_align)
                        .field("GlobalLock", &b.should_lock_global_lock)
                        .finish_non_exhaustive()
                }
            }
            AmlVariable::ByteField(b) => {
                if let Ok(s) = b.source.try_lock() {
                    f.debug_struct("ByteField")
                        .field("Source", &*s)
                        .field("ByteIndex", &b.byte_index)
                        .field("NumberOfBytes", &b.num_of_bytes)
                        .field("GlobalLock", &b.should_lock_global_lock)
                        .finish()
                } else {
                    f.debug_struct("ByteField")
                        .field("ByteIndex", &b.byte_index)
                        .field("NumberOfBytes", &b.num_of_bytes)
                        .field("GlobalLock", &b.should_lock_global_lock)
                        .finish_non_exhaustive()
                }
            }
            AmlVariable::IndexField(b) => {
                let index = b.index_register.try_lock();
                let data = b.data_register.try_lock();
                if index.is_ok() && data.is_ok() {
                    f.debug_struct("IndexField")
                        .field("IndexRegister", &*index.unwrap())
                        .field("DataRegister", &*data.unwrap())
                        .field("BitIndex", &b.bit_index)
                        .field("NumberOfBits", &b.num_of_bits)
                        .field("AccessAlign", &b.access_align)
                        .field("GlobalLock", &b.should_lock_global_lock)
                        .finish()
                } else {
                    drop(index);
                    drop(data);
                    f.debug_struct("IndexField")
                        .field("BitIndex", &b.bit_index)
                        .field("NumberOfBits", &b.num_of_bits)
                        .field("AccessAlign", &b.access_align)
                        .field("GlobalLock", &b.should_lock_global_lock)
                        .finish_non_exhaustive()
                }
            }
            AmlVariable::Package(p) => f.write_fmt(format_args!("Package({:?})", p)),
            AmlVariable::Method(m) => f.write_fmt(format_args!("Method({})", m.get_name())),
            AmlVariable::BuiltInMethod(m) => f.write_fmt(format_args!(
                "BuiltInMethod({})",
                core::any::type_name_of_val(m)
            )),
            AmlVariable::Mutex(m) => f
                .debug_struct("Mutex")
                .field("Current", &m.0)
                .field("SyncLevel", &m.1)
                .finish(),
            AmlVariable::Reference((s, i)) => {
                if let Ok(s) = s.try_lock() {
                    f.debug_struct("Reference")
                        .field("Source", &*s)
                        .field("Index", i)
                        .finish()
                } else {
                    f.debug_struct("Reference")
                        .field("Index", i)
                        .finish_non_exhaustive()
                }
            }
        }
    }
}
