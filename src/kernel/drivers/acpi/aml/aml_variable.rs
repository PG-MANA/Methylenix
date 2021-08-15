//!
//! AML Variable
//!

use super::data_object::ConstData;
use super::name_object::NameString;
use super::named_object::Method;
use super::{AcpiInt, AmlError};

use crate::arch::target_arch::device::acpi::{
    read_embedded_controller, read_io, read_memory, read_pci, write_embedded_controller, write_io,
    write_memory, write_pci,
};

use crate::kernel::memory_manager::data_type::PAddress;
use crate::kernel::sync::spin_lock::Mutex;

use core::sync::atomic::{AtomicU8, Ordering};

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

pub type AmlFunction = fn(&[Arc<Mutex<AmlVariable>>]) -> Result<AmlVariable, AmlError>;

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
                    let bit_index = bit_index >> 3;
                    if byte_offset > *limit {
                        pr_err!(
                            "Offset({}) is out of I/O area(Port: {:#X}, Limit:{:#X}).",
                            byte_offset,
                            port,
                            limit
                        );
                        Err(AmlError::InvalidOperation)
                    } else {
                        write_io(*port + byte_offset, bit_index, access_align, c)
                    }
                } else {
                    pr_err!("Writing {:?} into I/O({}) is invalid.", data, port);
                    Err(AmlError::InvalidOperation)
                }
            }
            Self::MMIo((address, limit)) => {
                if let Self::ConstData(c) = data {
                    let byte_offset = byte_index + (bit_index >> 3);
                    let bit_index = bit_index >> 3;
                    if byte_offset > *limit {
                        pr_err!(
                            "Offset({}) is out of Memory area(Address: {:#X}, Limit:{:#X}).",
                            byte_offset,
                            address,
                            limit
                        );
                        Err(AmlError::InvalidOperation)
                    } else {
                        write_memory(
                            PAddress::new(*address + byte_offset),
                            bit_index,
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
                if (bit_index % 8) != 0 {
                    pr_err!("Bit Index is not supported in Embedded Controller.");
                    Err(AmlError::InvalidOperation)
                } else if adjusted_byte_index >= *limit {
                    pr_err!(
                        "Offset({}) is out of Embedded Controller area(Address: {:#X}, Limit:{:#X}).",
                        adjusted_byte_index,
                        address,
                        limit
                    );
                    Err(AmlError::InvalidOperation)
                } else {
                    write_embedded_controller(
                        (*address + adjusted_byte_index) as u8,
                        data.to_int()? as u8,
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
                        write_pci(
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
                    Ok(Self::ConstData(read_memory(
                        PAddress::new(*address + byte_offset),
                        adjusted_bit_index,
                        access_align,
                        num_of_bits,
                    )?))
                }
            }
            Self::EcIo((address, limit)) => {
                let adjusted_byte_index = byte_index + (bit_index >> 3);
                if (bit_index % 8) != 0 {
                    pr_err!("Bit Index is not supported in Embedded Controller.");
                    Err(AmlError::InvalidOperation)
                } else if adjusted_byte_index >= *limit {
                    pr_err!(
                        "Offset({}) is out of Embedded Controller area(Address: {:#X}, Limit:{:#X}).",
                        adjusted_byte_index,
                        address,
                        limit
                    );
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok(Self::ConstData(ConstData::Byte(read_embedded_controller(
                        (*address + adjusted_byte_index) as u8,
                    )?)))
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
                    Ok(Self::ConstData(read_pci(
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
            | Self::Reference(_) => self.get_constant_data()?.convert_to_aml_package(),
            Self::Package(p) => Ok(AmlPackage::Package(p)),
            Self::Mutex(_) => Err(AmlError::InvalidType),
            Self::Method(_) => Err(AmlError::InvalidType),
            Self::BuiltInMethod(_) => Err(AmlError::InvalidType),
        }
    }
}

impl core::fmt::Debug for AmlVariable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AmlVariable::Uninitialized => f.write_str("Uninitialized"),
            AmlVariable::ConstData(c) => f.write_fmt(format_args!("ConstantData({})", c.to_int())),
            AmlVariable::String(s) => f.write_fmt(format_args!("String({})", s)),
            AmlVariable::Buffer(b) => f.write_fmt(format_args!("Buffer({:?})", b)),
            AmlVariable::Io((port, limit)) => f.write_fmt(format_args!(
                "SystemI/O(Port: {:#X}, Limit: {:#X})",
                port, limit
            )),
            AmlVariable::MMIo((port, limit)) => f.write_fmt(format_args!(
                "MemoryI/O(Port: {:#X}, Limit: {:#X})",
                port, limit
            )),
            AmlVariable::EcIo((port, limit)) => f.write_fmt(format_args!(
                "EmbeddedControllerI/O(Port: {:#X}, Limit: {:#X})",
                port, limit
            )),
            AmlVariable::PciConfig(p) => f.write_fmt(format_args!("PCI_Config({:?})", p)),
            AmlVariable::BitField(b) => f.write_fmt(format_args!("{:?}", b)),
            AmlVariable::ByteField(b) => f.write_fmt(format_args!("{:?}", b)),
            AmlVariable::Package(p) => f.write_fmt(format_args!("Package({:?}", p)),
            AmlVariable::Method(m) => f.write_fmt(format_args!("Method({})", m.get_name())),
            AmlVariable::BuiltInMethod(m) => f.write_fmt(format_args!(
                "BuiltInMethod({})",
                core::any::type_name_of_val(m)
            )),
            AmlVariable::Mutex(m) => f.write_fmt(format_args!(
                "Mutex(Current: {:?}, SyncLevel: {})",
                m.0, m.1
            )),
            AmlVariable::Reference((s, i)) => {
                if let Ok(s) = s.try_lock() {
                    f.write_fmt(format_args!("Reference(Source: {:?}, Index: {:?})", *s, i))
                } else {
                    f.write_fmt(format_args!("Reference(Source: Locked, Index: {:?})", i))
                }
            }
        }
    }
}
