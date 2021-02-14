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
                while stream.read_byte()? != Self::NULL_CHAR {}
                let end = stream.get_pointer();
                Ok(Self::StringData(
                    core::str::from_utf8(unsafe {
                        core::slice::from_raw_parts(
                            start.to_usize() as *const u8,
                            (end - start).to_usize() - 1,
                        )
                    })
                    .or(Err(AmlError::InvalidData))?,
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

#[derive(Clone, Eq, PartialEq)]
pub enum NameString {
    Normal(([[u8; 4]; 7], u8)),
    Ex(Vec<[u8; 4]>),
}

impl NameString {
    const ROOT_CHAR: u8 = 0x5C;
    const PARENT_PREFIX_CHAR: u8 = 0x5E;

    pub fn root() -> Self {
        Self::Normal(([[0; 4]; 7], 0))
    }

    pub fn parse(stream: &mut AmlStream, current_scope: Option<&Self>) -> Result<Self, AmlError> {
        let mut result = Self::root();
        let mut c = stream.read_byte()?;

        if c == Self::ROOT_CHAR {
            c = stream.read_byte()?;
        } else {
            if let Some(c_s) = current_scope {
                result = c_s.clone();
            }
            while c == Self::PARENT_PREFIX_CHAR {
                result.up_to_parent_name_space();
                c = stream.read_byte()?;
            }
        }
        /* Name Path */
        let num_name_path = if c == 0x00 {
            0
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
        if let Self::Normal((array, count)) = result {
            if count + num_name_path > 7 {
                let mut v: Vec<[u8; 4]> = Vec::with_capacity((count + num_name_path) as usize);
                for i in 0..count {
                    v.push(array[i as usize]);
                }
                result = Self::Ex(v);
            }
        }

        for count in 0..num_name_path {
            if count != 0 {
                c = stream.read_byte()?;
            }
            if !c.is_ascii_uppercase() && c != '_' as u8 {
                return Err(AmlError::InvalidData);
            }
            let mut name: [u8; 4] = [0; 4];
            name[0] = c;
            for i in 1..4 {
                c = stream.read_byte()?;
                if c == '_' as u8 {
                    name[i] = 0;
                } else {
                    if !c.is_ascii_uppercase() && !c.is_ascii_digit() {
                        return Err(AmlError::InvalidData);
                    }
                    name[i] = c;
                }
            }
            match &mut result {
                Self::Normal((array, count)) => {
                    array[*count as usize] = name;
                    *count += 1;
                }
                Self::Ex(v) => {
                    v.push(name);
                }
            }
        }
        return Ok(result);
    }

    pub fn up_to_parent_name_space(&mut self) {
        match self {
            Self::Normal((_, count)) => {
                if *count == 0 {
                    return;
                }
                *count -= 1;
            }
            Self::Ex(v) => {
                v.pop();
                if v.len() <= 7 {
                    let mut i = 0;
                    let mut array: [[u8; 4]; 7] = [[0; 4]; 7];
                    for e in v {
                        array[i] = *e;
                        i += 1;
                    }
                    *self = Self::Normal((array, i as u8));
                }
            }
        }
    }

    pub fn to_full_path_string(&self) -> String {
        let mut result = String::from('\\');
        let mut is_root = true;
        match self {
            Self::Normal((array, len)) => {
                for count in 0..(*len as usize) {
                    if is_root {
                        is_root = false;
                    } else {
                        result += ".";
                    }
                    result += core::str::from_utf8(&array[count]).unwrap_or("");
                }
            }
            Self::Ex(v) => {
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
        match self {
            Self::Normal((array, len)) => {
                if index >= (*len as usize) {
                    None
                } else {
                    Some(&array[index])
                }
            }
            Self::Ex(v) => v.get(index),
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

    pub fn get_relative_path(&self) -> Self {
        /* To fix... */
        let mut result = [[0; 4]; 7];
        match self {
            Self::Normal((array, len)) => {
                if *len == 0 {
                    self.clone()
                } else {
                    result[0] = array[*len as usize - 1];
                    Self::Normal((result, 1))
                }
            }
            Self::Ex(v) => {
                if let Some(l) = v.last() {
                    result[0] = *l;
                    Self::Normal((result, 1))
                } else {
                    self.clone()
                }
            }
        }
    }
}

impl core::fmt::Display for NameString {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write;
        f.write_char('\\')?;
        let mut is_root = true;
        match self {
            Self::Normal((array, len)) => {
                for count in 0..(*len as usize) {
                    if is_root {
                        is_root = false;
                    } else {
                        f.write_char('.')?;
                    }
                    f.write_str(core::str::from_utf8(&array[count]).unwrap_or("!!!!"))?;
                }
            }
            Self::Ex(v) => {
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
        f.write_fmt(format_args!("NameString({})", self))
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
        Ok(Self::ReferenceTypeOpcode(Box::new(
            expression_opcode::ReferenceTypeOpcode::try_parse(stream, current_scope, parse_helper)?,
        )))
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
