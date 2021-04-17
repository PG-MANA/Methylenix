//!
//! ACPI Machine Language
//!
//! This is the parser for AML.

mod data_object;
mod evaluator;
mod expression_opcode;
mod name_object;
mod named_object;
mod namespace_modifier_object;
mod opcode;
mod parser;
mod statement_opcode;
mod term_object;

pub use self::data_object::{eisa_id_to_dword, ConstData, DataRefObject};
use self::evaluator::Evaluator;
pub use self::name_object::NameString;
use self::named_object::{Device, Method, NamedObject};
use self::namespace_modifier_object::NamespaceModifierObject;
use self::parser::{ContentObject, ParseHelper};
use self::statement_opcode::StatementOpcode;
use self::term_object::{TermList, TermObj};

use crate::arch::target_arch::device::acpi::{read_io, read_memory, write_io, write_memory};

use crate::kernel::memory_manager::data_type::{Address, MSize, PAddress, VAddress};
use crate::kernel::sync::spin_lock::Mutex;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

type AcpiInt = usize;

pub struct AmlParser {
    base_address: VAddress,
    size: MSize,
    parse_helper: Option<ParseHelper>,
}

#[derive(Clone)]
pub struct AmlStream {
    pointer: VAddress,
    limit: VAddress,
}

#[derive(Debug)]
pub enum AmlError {
    AccessOutOfRange,
    InvalidSizeChange,
    InvalidType,
    InvalidMethodName(NameString),
    InvalidScope(NameString),
    InvalidOperation,
    MutexError,
    ObjectTreeError,
    NestedSearch,
    UnsupportedType,
}

pub struct IntIter {
    stream: AmlStream,
    remaining_elements: usize,
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

#[derive(Debug, Clone)]
pub enum AmlVariable {
    Uninitialized,
    ConstData(ConstData),
    String(String),
    Buffer(Vec<u8>),
    Io((usize, usize)),
    MMIo((usize, usize)),
    BitField(AmlBitFiled),
    ByteField(AmlByteFiled),
    Package(Vec<AmlPackage>),
    Method(Method),
    Reference((Arc<Mutex<Self>>, Option<usize /* For Index Of */>)),
}

impl IntIter {
    pub fn new(stream: AmlStream, num_of_elements: usize) -> Self {
        Self {
            stream,
            remaining_elements: num_of_elements,
        }
    }

    fn get_next(&mut self) -> Result<Option<AcpiInt>, AmlError> {
        if self.remaining_elements == 0 {
            Ok(None)
        } else {
            let d = DataRefObject::parse(&mut self.stream, &NameString::current())?;
            self.remaining_elements -= 1;
            Ok(d.get_const_data())
        }
    }
}

impl Iterator for IntIter {
    type Item = AcpiInt;
    fn next(&mut self) -> Option<Self::Item> {
        match self.get_next() {
            Ok(o) => o,
            Err(e) => {
                pr_err!("{:?}", e);
                None
            }
        }
    }
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
        assert!(data.is_constant_data(), "data is not constant({:?})", data);

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
            Self::ConstData(_) | Self::String(_) | Self::Uninitialized => {
                unreachable!() /* FIX: BitFiled may contain those objects. */
            }
            Self::Method(m) => {
                pr_err!("Writing data into Method({}) is invalid.", m.get_name());
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
                if (byte_offset + (num_of_bits >> 3)) > b.len() {
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
            Self::BitField(_) => false,
            Self::ByteField(_) => false,
            Self::Package(_) => true,
            Self::Uninitialized => true,
            Self::Method(_) => false,
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
        assert!(!self.is_constant_data());
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
                    pr_info!(
                        "ReadAddress:{:#X}, Offset:{}, BitIndex: {}, Align:{}, NumberOfBits:{}",
                        *address,
                        byte_offset,
                        adjusted_bit_index,
                        access_align,
                        num_of_bits
                    );
                    Ok(Self::ConstData(read_memory(
                        PAddress::new(*address + byte_offset),
                        adjusted_bit_index,
                        access_align,
                        num_of_bits,
                    )?))
                }
            }
            Self::ConstData(_)
            | Self::String(_)
            | Self::Buffer(_)
            | Self::Uninitialized
            | Self::Method(_) => unreachable!(),

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
            Self::Package(_) => {
                pr_err!(
                    "Reading data from Package({:?}) without index is invalid.",
                    self
                );
                Err(AmlError::InvalidOperation)
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
            | Self::Package(_) => Ok(self.clone()),
            Self::Io(_)
            | Self::MMIo(_)
            | Self::BitField(_)
            | Self::ByteField(_)
            | Self::Reference(_) => self._read(0, 0, false, 0, 0),
            Self::Method(m) => {
                pr_err!("Reading Method({}) is invalid.", m.get_name());
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
            Self::Io(_) => self.get_constant_data()?.to_int(),
            Self::MMIo(_) => self.get_constant_data()?.to_int(),
            Self::BitField(_) => self.get_constant_data()?.to_int(),
            Self::ByteField(_) => self.get_constant_data()?.to_int(),
            Self::Package(_) => self.get_constant_data()?.to_int(),
            Self::Uninitialized => Err(AmlError::InvalidType),
            Self::Method(_) => Err(AmlError::InvalidType),
            Self::Reference(_) => self.get_constant_data()?.to_int(),
        }
    }

    pub fn get_byte_size(&self) -> Result<usize, AmlError> {
        match self {
            Self::ConstData(c) => Ok(c.get_byte_size()),
            Self::String(s) => Ok(s.len()),
            Self::Buffer(b) => Ok(b.len()),
            Self::Io(_) => self.get_constant_data()?.get_byte_size(),
            Self::MMIo(_) => self.get_constant_data()?.get_byte_size(),
            Self::BitField(_) => self.get_constant_data()?.get_byte_size(),
            Self::ByteField(_) => self.get_constant_data()?.get_byte_size(),
            Self::Package(_) => self.get_constant_data()?.get_byte_size(),
            Self::Uninitialized => Err(AmlError::InvalidType),
            Self::Method(_) => Err(AmlError::InvalidType),
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
            Self::Io(_) => self.get_constant_data()?.convert_to_aml_package(),
            Self::MMIo(_) => self.get_constant_data()?.convert_to_aml_package(),
            Self::BitField(_) => self.get_constant_data()?.convert_to_aml_package(),
            Self::ByteField(_) => self.get_constant_data()?.convert_to_aml_package(),
            Self::Package(p) => Ok(AmlPackage::Package(p)),
            Self::Method(_) => Err(AmlError::InvalidType),
            Self::Reference(_) => self.get_constant_data()?.convert_to_aml_package(),
        }
    }
}

#[macro_export]
macro_rules! ignore_invalid_type_error {
    ($f:expr, $ok_stmt:expr) => {
        match $f {
            Ok(t) => return $ok_stmt(t),
            Err(AmlError::InvalidType) => { /* Ignore */ }
            Err(e) => return Err(e),
        };
    };
}

impl AmlParser {
    pub const fn new(address: VAddress, size: MSize) -> Self {
        /* memory area must be accessible. */
        Self {
            base_address: address,
            size,
            parse_helper: None,
        }
    }

    pub fn init(&mut self) -> bool {
        if self.parse_helper.is_some() {
            return true;
        }
        let root_name = NameString::root();
        let root_term_list = TermList::new(
            AmlStream::new(self.base_address, self.size),
            root_name.clone(),
        );
        let mut parse_helper = ParseHelper::new(root_term_list.clone(), &root_name);
        if let Err(e) = parse_helper.init() {
            println!("Cannot Init ParseHelper:{:?}", e);
            return false;
        }
        self.parse_helper = Some(parse_helper);
        return true;
    }

    fn get_content_object(&mut self, name: &NameString) -> Option<ContentObject> {
        if self.parse_helper.is_none() {
            return None;
        }
        match self
            .parse_helper
            .as_mut()
            .unwrap()
            .search_object_from_list_with_parsing_term_list(name)
        {
            Ok(Some(d)) => Some(d),
            Ok(None) => None,
            Err(e) => {
                pr_err!("Cannot parse AML: {:?}", e);
                None
            }
        }
    }

    pub fn get_data_ref_object(&mut self, name: &NameString) -> Option<DataRefObject> {
        if let Some(c) = self.get_content_object(name) {
            match c {
                ContentObject::NamedObject(n) => {
                    pr_err!("Expected DataRefObject, but found NamedObject: {:?}", n);
                    None
                }
                ContentObject::DataRefObject(d) => Some(d),
                ContentObject::Scope(s) => {
                    pr_err!("Expected DataRefObject, but found Scope: {:?}", s);
                    None
                }
            }
        } else {
            None
        }
    }

    pub fn get_device(&mut self, name: &NameString, hid: &[u8; 7]) -> Option<Device> {
        let hid = eisa_id_to_dword(hid);
        if let Some(c) = self.get_content_object(name) {
            match c {
                ContentObject::NamedObject(n) => {
                    if let NamedObject::DefDevice(d) = n {
                        match d.get_hid(self.parse_helper.as_mut().unwrap()) {
                            Ok(Some(d_id)) => {
                                if d_id == hid {
                                    return Some(d);
                                } else {
                                    pr_info!(
                                        "Miss matched HID: Searching({}):{}, Found: {}",
                                        d.get_name(),
                                        hid,
                                        d_id
                                    );
                                }
                            }
                            Ok(None) => {
                                pr_info!("{} has no HID", d.get_name());
                            }
                            Err(e) => {
                                pr_err!("Parsing AML was failed: {:?}", e)
                            }
                        }
                    } else {
                        pr_err!("Expected Device, but found NamedObject: {:?}", n);
                    }
                }
                ContentObject::DataRefObject(d) => {
                    pr_err!("Expected Device, but found DataRefObject: {:?}", d);
                }
                ContentObject::Scope(s) => {
                    pr_err!("Expected Device, but found Scope: {:?}", s);
                }
            }
        }
        return None;
    }

    pub fn evaluate_method(&mut self, method_name: &NameString) -> Option<AmlVariable> {
        if self.parse_helper.is_none() {
            return None;
        }
        if method_name.is_null_name() {
            pr_warn!("NullName");
            return None;
        }
        let mut method_parser = self.parse_helper.as_ref().unwrap().clone();
        match method_parser.setup_for_method_evaluation(method_name) {
            Ok(m) => {
                let mut evaluator = Evaluator::new(method_parser);
                match evaluator.eval_method(&m) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        pr_err!("AML Evaluator Error: {:?}", e);
                        None
                    }
                }
            }
            Err(e) => {
                pr_err!("AML Parser Error: {:?}", e);
                None
            }
        }
    }

    pub fn get_parse_helper(&mut self) -> &mut ParseHelper {
        self.parse_helper.as_mut().unwrap()
    }

    #[allow(dead_code)]
    pub fn debug(&mut self) {
        if self.parse_helper.is_none() {
            return;
        }
        println!("AML Size: {:#X}", self.size.to_usize());
        let root_name = NameString::root();
        let root_term_list = TermList::new(
            AmlStream::new(self.base_address, self.size),
            root_name.clone(),
        );
        match Self::debug_term_list(root_term_list, &mut self.parse_helper.as_mut().unwrap()) {
            Ok(_) => {
                println!("AML End");
            }
            Err(e) => {
                println!("ParseError: {:?}", e);
            }
        }
    }

    #[allow(dead_code)]
    fn debug_term_list(
        mut term_list: TermList,
        parse_helper: &mut ParseHelper,
    ) -> Result<(), AmlError> {
        while let Some(term_obj) = term_list.next(parse_helper)? {
            match term_obj {
                TermObj::NamespaceModifierObj(n) => match n {
                    NamespaceModifierObject::DefAlias(d_a) => {
                        println!("DefAlias({} => {})", d_a.get_name(), d_a.get_source());
                    }
                    NamespaceModifierObject::DefName(d_n) => {
                        println!(
                            "DefName({}) => {:?}",
                            d_n.get_name(),
                            d_n.get_data_ref_object()
                        );
                    }
                    NamespaceModifierObject::DefScope(d_s) => {
                        println!("DefScope({}) => {{", d_s.get_name());
                        parse_helper.move_into_term_list(d_s.get_term_list().clone())?;
                        if let Err(e) =
                            Self::debug_term_list(d_s.get_term_list().clone(), parse_helper)
                        {
                            pr_err!(
                                "Cannot parse {} Error: {:?}. Continue...",
                                d_s.get_name(),
                                e
                            );
                        }
                        parse_helper.move_out_from_current_term_list()?;
                        println!("}}");
                    }
                },
                TermObj::NamedObj(n_o) => {
                    println!("{:?}", n_o);
                    if let Some(mut field_list) = n_o.get_field_list() {
                        println!(
                            "FieldList({}) => {{",
                            n_o.get_name().unwrap_or(term_list.get_scope_name())
                        );
                        while let Some(field_element) = field_list.next()? {
                            println!("{:?}", field_element);
                        }
                        println!("}}");
                    } else if let Some(object_term_list) = n_o.get_term_list() {
                        let name = n_o.get_name().unwrap();
                        println!("TermList({}) => {{", name);
                        parse_helper.move_into_term_list(object_term_list.clone())?;
                        if let Err(e) = Self::debug_term_list(object_term_list, parse_helper) {
                            pr_err!("Cannot parse {} Error: {:?}. Continue...", name, e);
                        }
                        parse_helper.move_out_from_current_term_list()?;
                        println!("}}");
                    }
                }
                TermObj::StatementOpcode(s_o) => {
                    match s_o {
                        StatementOpcode::DefBreak => {
                            println!("break;");
                        }
                        StatementOpcode::DefBreakPoint => {
                            println!("(BreakPoint);");
                        }
                        StatementOpcode::DefContinue => {
                            println!("continue;");
                        }
                        StatementOpcode::DefFatal(f) => {
                            println!("{:?}", f);
                        }
                        StatementOpcode::DefIfElse(i_e) => {
                            println!("if({:?}) {{", i_e.get_predicate());
                            parse_helper
                                .move_into_term_list(i_e.get_if_true_term_list().clone())?;
                            if let Err(e) = Self::debug_term_list(
                                i_e.get_if_true_term_list().clone(),
                                parse_helper,
                            ) {
                                pr_err!(
                                    "Cannot parse if statement of {} Error: {:?}. Continue...",
                                    term_list.get_scope_name(),
                                    e
                                );
                            }
                            parse_helper.move_out_from_current_term_list()?;

                            if let Some(else_term_list) = i_e.get_if_false_term_list() {
                                println!("}} else {{");
                                parse_helper.move_into_term_list(else_term_list.clone())?;
                                if let Err(e) =
                                    Self::debug_term_list(else_term_list.clone(), parse_helper)
                                {
                                    pr_err!("Cannot parse else statement of {} Error: {:?}. Continue...",term_list.get_scope_name(),e);
                                }
                                parse_helper.move_out_from_current_term_list()?;
                            }
                            println!("}}");
                        }
                        StatementOpcode::DefNoop => {
                            println!("(Noop);")
                        }
                        StatementOpcode::DefNotify(notify) => {
                            println!("{:?}", notify);
                        }
                        StatementOpcode::DefRelease(release) => {
                            println!("Release(Mutex:{:?});", release);
                        }
                        StatementOpcode::DefReset(reset) => {
                            println!("Reset({:?})", reset)
                        }
                        StatementOpcode::DefReturn(return_value) => {
                            println!("return {:?};", return_value);
                        }
                        StatementOpcode::DefSignal(signal) => {
                            println!("Signal({:?})", signal);
                        }
                        StatementOpcode::DefSleep(sleep_time) => {
                            println!("Sleep(microsecond:{:?});", sleep_time);
                        }
                        StatementOpcode::DefStall(u_sec_time) => {
                            println!("Stall(millisecond:{:?})", u_sec_time);
                        }
                        StatementOpcode::DefWhile(w) => {
                            println!("while({:?}) {{", w.get_predicate());
                            parse_helper.move_into_term_list(w.get_term_list().clone())?;
                            if let Err(e) =
                                Self::debug_term_list(w.get_term_list().clone(), parse_helper)
                            {
                                pr_err!(
                                    "Cannot parse while statement of {} Error: {:?}. Continue...",
                                    term_list.get_scope_name(),
                                    e
                                );
                            }
                            parse_helper.move_out_from_current_term_list()?;
                            println!("}}");
                        }
                    }
                }
                TermObj::ExpressionOpcode(e_o) => {
                    println!("{:?}", e_o);
                }
            }
        }
        Ok(())
    }
}

impl AmlStream {
    pub const fn new(address: VAddress, size: MSize) -> Self {
        Self {
            pointer: address,
            limit: address + size,
        }
    }

    fn check_pointer(&self, read_size: usize) -> Result<(), AmlError> {
        if self.pointer + MSize::new(read_size) > self.limit {
            Err(AmlError::AccessOutOfRange)
        } else {
            Ok(())
        }
    }

    pub fn is_end_of_stream(&self) -> bool {
        self.pointer == self.limit
    }

    fn get_available_size(&self) -> usize {
        (self.limit - self.pointer).to_usize()
    }

    fn read<T: ?Sized + Copy>(&mut self) -> Result<T, AmlError> {
        self.check_pointer(core::mem::size_of::<T>())?;
        let d = unsafe { *((self.pointer).to_usize() as *const T) };
        self.pointer += MSize::new(core::mem::size_of::<T>());
        return Ok(d);
    }

    fn read_byte(&mut self) -> Result<u8, AmlError> {
        self.read::<u8>()
    }

    fn read_word(&mut self) -> Result<u16, AmlError> {
        Ok(u16::from_le(self.read::<_>()?))
    }

    fn read_dword(&mut self) -> Result<u32, AmlError> {
        Ok(u32::from_le(self.read::<_>()?))
    }

    fn read_qword(&mut self) -> Result<u64, AmlError> {
        Ok(u64::from_le(self.read::<_>()?))
    }

    fn peek_byte(&self) -> Result<u8, AmlError> {
        self.check_pointer(0)?;
        Ok(unsafe { *(self.pointer.to_usize() as *const u8) })
    }

    fn peek_byte_with_pos(&self, pos_forward_from_current: usize) -> Result<u8, AmlError> {
        self.check_pointer(pos_forward_from_current)?;
        Ok(unsafe {
            *((self.pointer + MSize::new(pos_forward_from_current)).to_usize() as *const u8)
        })
    }

    fn seek(&mut self, bytes_to_forward: usize) -> Result<(), AmlError> {
        self.pointer += MSize::new(bytes_to_forward);
        if self.pointer > self.limit {
            Err(AmlError::AccessOutOfRange)
        } else {
            Ok(())
        }
    }

    fn change_size(&mut self, new_size_from_current_point: usize) -> Result<(), AmlError> {
        let new_limit = self.pointer + MSize::new(new_size_from_current_point);
        if new_limit > self.limit {
            Err(AmlError::InvalidSizeChange)
        } else {
            self.limit = new_limit;
            Ok(())
        }
    }

    fn get_pointer(&self) -> VAddress {
        self.pointer
    }

    fn roll_back(&mut self, backup: &Self) {
        self.pointer = backup.pointer;
        self.limit = backup.limit;
    }
}

impl core::fmt::Debug for AmlStream {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "AmlStream(Base: {:#X}, Size: {:#X})",
            self.get_pointer().to_usize(),
            self.get_available_size()
        ))
    }
}
