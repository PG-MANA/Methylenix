//!
//! ACPI Machine Language Data Objects
//!
#![allow(dead_code)]
use super::expression_opcode;
use super::opcode;
use super::parser::ParseHelper;
use super::{AcpiData, AcpiInt, AmlError, AmlStream};

use crate::ignore_invalid_type_error;
use crate::kernel::memory_manager::data_type::Address;

use crate::kernel::drivers::acpi::aml::data_object::NameStringData::Normal;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct PkgLength {
    pub length: usize,
    pub actual_length: usize,
}

#[derive(Debug, Clone)]
pub enum ComputationalData {
    ConstData(AcpiData),
    StringData(&'static str),
    ConstObj(u8),
    Revision,
    DefBuffer(expression_opcode::ByteList),
}

impl ComputationalData {
    const BYTE_PREFIX: u8 = 0x0A;
    const WORD_PREFIX: u8 = 0x0B;
    const DWORD_PREFIX: u8 = 0x0C;
    const QWORD_PREFIX: u8 = 0x0E;
    const STRING_PREFIX: u8 = 0x0D;
    const NULL_CHAR: u8 = 0x00;

    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* println!("DataObject: {:#X}", stream.peek_byte()?); */
        match stream.read_byte()? {
            Self::BYTE_PREFIX => Ok(Self::ConstData(stream.read_byte()? as AcpiData)),
            Self::WORD_PREFIX => Ok(Self::ConstData(stream.read_word()? as AcpiData)),
            Self::DWORD_PREFIX => Ok(Self::ConstData(stream.read_dword()? as AcpiData)),
            Self::QWORD_PREFIX => Ok(Self::ConstData(stream.read_qword()? as AcpiData)),
            Self::STRING_PREFIX => {
                let start = stream.get_pointer();
                while stream.read_byte()? != Self::NULL_CHAR && !stream.is_end_of_stream() {}
                let end = stream.get_pointer();
                Ok(Self::StringData(
                    core::str::from_utf8(unsafe {
                        core::slice::from_raw_parts(
                            start.to_usize() as *const u8,
                            (end - start).to_usize() - 1,
                        )
                    })
                    .or(Err(AmlError::InvalidType))?,
                ))
            }
            opcode::BUFFER_OP => Ok(Self::DefBuffer(expression_opcode::ByteList::parse(
                stream,
                current_scope,
            )?)),
            opcode::ZERO_OP => Ok(Self::ConstObj(opcode::ZERO_OP)),
            opcode::ONE_OP => Ok(Self::ConstObj(opcode::ONE_OP)),
            opcode::ONES_OP => Ok(Self::ConstObj(opcode::ONES_OP)),
            opcode::EXT_OP_PREFIX => {
                if stream.read_byte()? == opcode::REVISION_OP {
                    Ok(Self::Revision)
                } else {
                    Err(AmlError::InvalidType)
                }
            }
            _ => Err(AmlError::InvalidType),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DataObject {
    ComputationalData(ComputationalData),
    DefPackage(expression_opcode::Package),
    DefVarPackage(expression_opcode::VarPackage),
}

impl DataObject {
    pub fn try_parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        let backup = stream.clone();
        ignore_invalid_type_error!(ComputationalData::parse(stream, current_scope), |t| {
            Ok(Self::ComputationalData(t))
        });
        stream.roll_back(&backup);
        ignore_invalid_type_error!(expression_opcode::Package::try_parse(stream), |l| {
            Ok(Self::DefPackage(l))
        });
        ignore_invalid_type_error!(expression_opcode::VarPackage::try_parse(stream), |l| {
            Ok(Self::DefVarPackage(l))
        });
        Err(AmlError::InvalidType)
    }
}

#[derive(Debug, Clone)]
pub enum DataRefObject {
    DataObject(DataObject),
    ObjectReference(AcpiInt),
}

impl DataRefObject {
    pub fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        ignore_invalid_type_error!(DataObject::try_parse(stream, current_scope), |d| {
            Ok(Self::DataObject(d))
        });
        Ok(Self::ObjectReference(parse_integer(stream)?))
    }
}

impl PkgLength {
    pub fn parse(stream: &mut AmlStream) -> Result<Self, AmlError> {
        let pkg_lead_byte = stream.read_byte()?;
        let byte_data_count = pkg_lead_byte >> 6;
        if byte_data_count == 0 {
            Ok(Self {
                length: pkg_lead_byte as usize,
                actual_length: pkg_lead_byte as usize - 1,
            })
        } else {
            let mut result = ((stream.read_byte()? as usize) << 4) | (pkg_lead_byte & 0xf) as usize;
            if byte_data_count > 1 {
                result |= (stream.read_byte()? as usize) << 12;
            }
            if byte_data_count > 2 {
                result |= (stream.read_byte()? as usize) << 20;
            }
            Ok(Self {
                length: result,
                actual_length: result - byte_data_count as usize - 1,
            })
        }
    }
}

pub fn parse_integer(stream: &mut AmlStream) -> Result<AcpiInt, AmlError> {
    let mut val = 0;
    let mut c = stream.read_byte()? as char;
    if c == '0' {
        c = stream.read_byte()? as char;
        if c == 'x' {
            loop {
                c = stream.peek_byte()? as char;
                if '0' <= c && c <= '9' {
                    val <<= 4; /* val *= 0x10 */
                    val += c as usize - 0x30/* '0' */;
                } else if 'a' <= c && c <= 'f' {
                    val <<= 4; /* val *= 0x10 */
                    val += c as usize - 0x61/* 'a' */ + 0xa;
                } else {
                    return Ok(val);
                }
                stream.seek(1)?;
            }
        } else if c == 'X' {
            loop {
                c = stream.peek_byte()? as char;
                if '0' <= c && c <= '9' {
                    val <<= 4; /* val *= 0x10 */
                    val += c as usize - 0x30/* '0' */;
                } else if 'A' <= c && c <= 'F' {
                    val <<= 4; /* val *= 0x10 */
                    val += c as usize - 0x41/* 'A' */ + 0xa;
                } else {
                    return Ok(val);
                }
                stream.seek(1)?;
            }
        } else {
            loop {
                c = stream.peek_byte()? as char;
                if '0' <= c && c <= '7' {
                    val <<= 3; /* val *= 0o10 */
                    val += c as usize - 0x30 /* '0' */;
                } else {
                    return Ok(val);
                }
                stream.seek(1)?;
            }
        }
    } else {
        loop {
            c = stream.peek_byte()? as char;
            if '0' <= c && c <= '9' {
                val *= 10;
                val += c as usize - 0x30/* '0' */;
            } else {
                return Ok(val);
            }
            stream.seek(1)?;
        }
    }
}

#[derive(Clone)]
enum NameStringData {
    Normal(([[u8; 4]; 7], u8)),
    Ex(Vec<[u8; 4]>),
}

#[derive(Clone, Eq, PartialEq)]
enum NameStringFlag {
    RelativePath,
    AbsolutePath,
    NullName,
}

#[derive(Clone)]
pub struct NameString {
    data: NameStringData,
    flag: NameStringFlag,
}

impl NameString {
    const ROOT_CHAR: u8 = 0x5C;
    const PARENT_PREFIX_CHAR: u8 = 0x5E;

    pub fn root() -> Self {
        Self {
            data: NameStringData::Normal(([[0; 4]; 7], 0)),
            flag: NameStringFlag::AbsolutePath,
        }
    }

    pub fn is_null_name(&self) -> bool {
        self.flag == NameStringFlag::NullName
    }

    pub fn parse(stream: &mut AmlStream, current_scope: Option<&Self>) -> Result<Self, AmlError> {
        let mut result = Self::root();
        let mut c = stream.read_byte()?;
        let mut may_be_null_name = true;

        if c == Self::ROOT_CHAR {
            may_be_null_name = false;
            result.flag = NameStringFlag::AbsolutePath;
            c = stream.read_byte()?;
        } else {
            result.flag = NameStringFlag::AbsolutePath;
            if let Some(c_s) = current_scope {
                if !c_s.is_null_name() {
                    result = c_s.clone();
                }
            }
            while c == Self::PARENT_PREFIX_CHAR {
                may_be_null_name = false;
                result.up_to_parent_name_space();
                c = stream.read_byte()?;
            }
        }
        /* Name Path */
        let num_name_path = if c == 0x00 {
            if may_be_null_name {
                result.flag = NameStringFlag::NullName;
                return Ok(result);
            } else {
                0
            }
        } else if c == 0x2E {
            c = stream.read_byte()?;
            2
        } else if c == 0x2F {
            let seg_count = stream.read_byte()?;
            c = stream.read_byte()?;
            seg_count
        } else {
            1
        };
        if let NameStringData::Normal((array, count)) = result.data {
            if count + num_name_path > 7 {
                let mut v: Vec<[u8; 4]> = Vec::with_capacity((count + num_name_path) as usize);
                for i in 0..count {
                    v.push(array[i as usize]);
                }
                result.data = NameStringData::Ex(v);
            }
        }

        for count in 0..num_name_path {
            if count != 0 {
                c = stream.read_byte()?;
            }
            if !c.is_ascii_uppercase() && c != '_' as u8 {
                return Err(AmlError::InvalidType);
            }
            let mut name: [u8; 4] = [0; 4];
            name[0] = c;
            for i in 1..4 {
                c = stream.read_byte()?;
                if !c.is_ascii_uppercase() && !c.is_ascii_digit() && c != '_' as u8 {
                    return Err(AmlError::InvalidType);
                }
                name[i] = c;
            }
            for i in (1..4).rev() {
                if name[i] == '_' as u8 {
                    name[i] = 0;
                } else {
                    break;
                }
            }
            match &mut result.data {
                NameStringData::Normal((array, count)) => {
                    array[*count as usize] = name;
                    *count += 1;
                }
                NameStringData::Ex(v) => {
                    v.push(name);
                }
            }
        }
        return Ok(result);
    }

    pub fn up_to_parent_name_space(&mut self) {
        if self.flag == NameStringFlag::NullName {
            return;
        }
        match &mut self.data {
            NameStringData::Normal((_, count)) => {
                if *count == 0 {
                    return;
                }
                *count -= 1;
            }
            NameStringData::Ex(v) => {
                v.pop();
                if v.len() <= 7 {
                    let mut i = 0;
                    let mut array: [[u8; 4]; 7] = [[0; 4]; 7];
                    for e in v {
                        array[i] = *e;
                        i += 1;
                    }
                    self.data = NameStringData::Normal((array, i as u8));
                }
            }
        }
    }

    pub fn to_string(&self) -> String {
        if self.flag == NameStringFlag::NullName {
            return String::with_capacity(0);
        }
        let mut result = if self.flag == NameStringFlag::AbsolutePath {
            String::from('\\')
        } else {
            String::new()
        };
        let mut is_root = true;
        match &self.data {
            NameStringData::Normal((array, len)) => {
                for count in 0..(*len as usize) {
                    if is_root {
                        is_root = false;
                    } else {
                        result += ".";
                    }
                    result += core::str::from_utf8(&array[count]).unwrap_or("");
                }
            }
            NameStringData::Ex(v) => {
                for e in v.iter() {
                    if is_root {
                        is_root = false;
                    } else {
                        result += ".";
                    }
                    result += core::str::from_utf8(e).unwrap_or("");
                }
            }
        }
        return result;
    }

    fn get_element(&self, index: usize) -> Option<&[u8; 4]> {
        if self.flag == NameStringFlag::NullName {
            return None;
        }
        match &self.data {
            NameStringData::Normal((array, len)) => {
                if index >= (*len as usize) {
                    None
                } else {
                    Some(&array[index])
                }
            }
            NameStringData::Ex(v) => v.get(index),
        }
    }

    pub fn get_element_as_name_string(&self, index: usize) -> Option<Self> {
        if self.flag == NameStringFlag::NullName {
            return None;
        }
        if let Some(e) = self.get_element(index) {
            let mut array = [[0u8; 4]; 7];
            array[0] = *e;
            Some(Self {
                data: Normal((array, 1)),
                flag: NameStringFlag::RelativePath,
            })
        } else {
            None
        }
    }

    pub fn is_child(&self, child: &Self) -> bool {
        for index in 0.. {
            let s1 = self.get_element(index);
            let s2 = child.get_element(index);
            if s1.is_none() {
                return if s2.is_some() { true } else { false };
            }
            if s1 != s2 {
                return false;
            }
        }
        return false;
    }

    pub fn get_relative_name(&self, scope_name: &Self) -> Option<Self> {
        if !scope_name.is_child(self) {
            return None;
        }
        for index in 0.. {
            let s1 = self.get_element(index);
            let s2 = scope_name.get_element(index);
            if s2.is_none() {
                if s1.is_none() {
                    return None;
                }
                let mut buffer = [[0u8; 4]; 7];
                let mut vector: Option<Vec<[u8; 4]>> = None;
                let mut counter = 1;
                buffer[0] = *s1.unwrap();
                let mut index = index + 1;
                while let Some(d) = self.get_element(index) {
                    if counter >= 7 {
                        let mut v: Vec<[u8; 4]> = Vec::with_capacity(7);
                        for i in 0..7 {
                            v.push(buffer[i]);
                        }
                        vector = Some(v);
                    }
                    if let Some(v) = vector.as_mut() {
                        v.push(*d);
                    } else {
                        buffer[counter] = *d;
                        counter += 1;
                    }
                    index += 1;
                }
                return Some(Self {
                    data: if let Some(v) = vector {
                        NameStringData::Ex(v)
                    } else {
                        NameStringData::Normal((buffer, counter as u8))
                    },
                    flag: NameStringFlag::RelativePath,
                });
            }
            if s1 != s2 {
                return None;
            }
        }
        unreachable!()
    }

    pub fn get_full_name_path(&self, scope_name: &Self) -> Self {
        if self.flag == NameStringFlag::NullName {
            return scope_name.clone();
        } else if scope_name.flag == NameStringFlag::NullName {
            return self.clone();
        }
        let mut result = scope_name.clone();
        let mut index = 0;
        while let Some(d) = self.get_element(index) {
            match &mut result.data {
                NameStringData::Normal((array, counter)) => {
                    if *counter >= 7 {
                        let mut v: Vec<[u8; 4]> = Vec::with_capacity(8);
                        for i in 0..7 {
                            v.push(array[i]);
                        }
                        v.push(*d);
                        result.data = NameStringData::Ex(v);
                    } else {
                        array[*counter as usize] = *d;
                        *counter += 1;
                    }
                }
                NameStringData::Ex(v) => v.push(*d),
            }
            index += 1;
        }
        return result;
    }

    pub fn from_array(array: &[[u8; 4]], is_absolute: bool) -> Self {
        if array.len() >= 7 {
            Self {
                data: NameStringData::Ex(Vec::from(array)),
                flag: if is_absolute {
                    NameStringFlag::AbsolutePath
                } else {
                    NameStringFlag::RelativePath
                },
            }
        } else {
            let mut buf = [[0u8; 4]; 7];
            for index in 0..array.len() {
                buf[index] = array[index];
            }
            Self {
                data: NameStringData::Normal((buf, array.len() as u8)),
                flag: if is_absolute {
                    NameStringFlag::AbsolutePath
                } else {
                    NameStringFlag::RelativePath
                },
            }
        }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        let is_absolute = s.starts_with('\\');
        let split = s.as_bytes().split(|c| *c == b'\\');
        let to_u8_4_array = |e: &[u8]| -> [u8; 4] {
            let mut result = [0u8; 4];
            for i in 0..e.len() {
                result[i] = e[i];
            }
            for i in (1..4).rev() {
                if result[i] == '_' as u8 {
                    result[i] = 0;
                } else {
                    break;
                }
            }
            result
        };

        let count = if is_absolute {
            split.clone().count() - 1
        } else {
            split.clone().count()
        };
        if count >= 7 {
            let mut vec: Vec<[u8; 4]> = Vec::with_capacity(count);
            for e in split.into_iter() {
                if e.len() > 4 {
                    return None;
                }
                if e.len() != 0 {
                    vec.push(to_u8_4_array(e))
                }
            }
            Some(Self {
                data: NameStringData::Ex(vec),
                flag: if is_absolute {
                    NameStringFlag::AbsolutePath
                } else {
                    NameStringFlag::RelativePath
                },
            })
        } else {
            let mut buf = [[0u8; 4]; 7];
            let mut index = 0;
            for e in split.into_iter() {
                if e.len() > 4 {
                    return None;
                }
                if e.len() != 0 {
                    buf[index] = to_u8_4_array(e);
                    index += 1;
                }
            }
            Some(Self {
                data: NameStringData::Normal((buf, index as u8)),
                flag: if is_absolute {
                    NameStringFlag::AbsolutePath
                } else {
                    NameStringFlag::RelativePath
                },
            })
        }
    }

    pub fn len(&self) -> usize {
        match &self.data {
            NameStringData::Normal((_, c)) => *c as usize,
            NameStringData::Ex(v) => v.len(),
        }
    }

    pub fn suffix_search(&self, other: &Self) -> bool {
        if self.flag == NameStringFlag::NullName || other.flag == NameStringFlag::NullName {
            return true;
        }

        let self_len = self.len();
        let other_len = other.len();
        if self_len == 0 || other_len == 0 {
            return true;
        }
        for (self_index, other_index) in (0..self_len).rev().zip((0..other_len).rev()) {
            let self_e = self.get_element(self_index).unwrap();
            let other_e = other.get_element(other_index).unwrap();
            if self_e != other_e {
                return false;
            }
        }
        return true;
    }
}

impl core::cmp::PartialEq for NameString {
    fn eq(&self, other: &Self) -> bool {
        if self.flag == NameStringFlag::NullName && other.flag == NameStringFlag::NullName {
            return true;
        }
        if self.len() != other.len() {
            return false;
        }
        for i in 0..self.len() {
            if self.get_element(i) != other.get_element(i) {
                return false;
            }
        }
        return true;
    }
}

impl core::fmt::Display for NameString {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write;
        if self.flag == NameStringFlag::NullName {
            return Ok(());
        }
        if self.flag == NameStringFlag::AbsolutePath {
            f.write_char('\\')?;
        }
        let mut is_root = true;
        match &self.data {
            NameStringData::Normal((array, len)) => {
                for count in 0..(*len as usize) {
                    if is_root {
                        is_root = false;
                    } else {
                        f.write_char('.')?;
                    }
                    f.write_str(core::str::from_utf8(&array[count]).unwrap_or("!!!!"))?;
                }
            }
            NameStringData::Ex(v) => {
                for e in v.iter() {
                    if is_root {
                        is_root = false;
                    } else {
                        f.write_char('.')?;
                    }
                    f.write_str(core::str::from_utf8(e).unwrap_or("!!!!"))?;
                }
            }
        }
        Ok(())
    }
}

impl core::fmt::Debug for NameString {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.flag == NameStringFlag::NullName {
            f.write_str("NameString(NullName)")
        } else {
            f.write_fmt(format_args!("NameString({})", self))
        }
    }
}

#[derive(Debug, Clone)]
pub enum Target {
    Null,
    SuperName(SuperName),
}

impl Target {
    pub fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        if stream.peek_byte()? == 0 {
            stream.seek(1)?;
            Ok(Self::Null)
        } else {
            Ok(Self::SuperName(SuperName::try_parse(
                stream,
                current_scope,
                parse_helper,
            )?))
        }
    }
}

#[derive(Debug, Clone)]
pub enum SimpleName {
    NameString(NameString),
    ArgObj(u8),
    LocalObj(u8),
}

impl SimpleName {
    pub fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        ignore_invalid_type_error!(try_parse_local_object(stream), |n| {
            return Ok(Self::LocalObj(n));
        });
        ignore_invalid_type_error!(try_parse_argument_object(stream), |n| {
            return Ok(Self::ArgObj(n));
        });
        Ok(Self::NameString(NameString::parse(
            stream,
            Some(current_scope),
        )?))
    }
}

#[derive(Debug, Clone)]
pub enum SuperName {
    SimpleName(SimpleName),
    DebugObj,
    ReferenceTypeOpcode(Box<expression_opcode::ReferenceTypeOpcode>),
}

impl SuperName {
    pub fn try_parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        ignore_invalid_type_error!(
            expression_opcode::ReferenceTypeOpcode::try_parse(stream, current_scope, parse_helper),
            |r_n| {
                return Ok(Self::ReferenceTypeOpcode(Box::new(r_n)));
            }
        );
        let backup = stream.clone();
        ignore_invalid_type_error!(SimpleName::parse(stream, current_scope), |n| {
            return Ok(Self::SimpleName(n));
        });
        stream.roll_back(&backup);
        let op = stream.peek_byte()?;
        if op == opcode::EXT_OP_PREFIX {
            let ext_op = stream.peek_byte_with_pos(1)?;
            if ext_op == opcode::DEBUG_OP {
                stream.seek(2)?;
                return Ok(Self::DebugObj);
            }
        }
        return Err(AmlError::InvalidType);
    }
}

/* Miscellaneous Objects */
fn try_parse_miscellaneous_object(
    stream: &mut AmlStream,
    start: u8,
    end: u8,
) -> Result<u8, AmlError> {
    if (start..=end).contains(&stream.peek_byte()?) {
        Ok(stream.read_byte()? - start)
    } else {
        Err(AmlError::InvalidType)
    }
}

pub fn try_parse_local_object(stream: &mut AmlStream) -> Result<u8, AmlError> {
    try_parse_miscellaneous_object(stream, opcode::LOCAL0_OP, opcode::LOCAL7_OP)
}

pub fn try_parse_argument_object(stream: &mut AmlStream) -> Result<u8, AmlError> {
    try_parse_miscellaneous_object(stream, opcode::ARG0_OP, opcode::ARG6_OP)
}
