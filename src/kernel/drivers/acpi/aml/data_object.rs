//!
//! ACPI Machine Language Data Objects
//!

use super::expression_opcode;
use super::name_object::NameString;
use super::opcode;
use super::{AcpiData, AcpiInt, AmlError, AmlStream, IntIter};

use crate::ignore_invalid_type_error;
use crate::kernel::memory_manager::data_type::Address;

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

    pub fn to_int_iter(&self) -> Option<IntIter> {
        match self {
            ComputationalData::ConstData(_) => None,
            ComputationalData::StringData(_) => None,
            ComputationalData::ConstObj(_) => None,
            ComputationalData::Revision => None,
            ComputationalData::DefBuffer(_) => {
                unimplemented!()
            }
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

    pub fn to_int_iter(&self) -> Option<IntIter> {
        match self {
            DataRefObject::DataObject(d) => match d {
                DataObject::ComputationalData(c_d) => c_d.to_int_iter(),
                DataObject::DefPackage(d_p) => Some(d_p.to_int_iter()),
                DataObject::DefVarPackage(d_v) => Some(d_v.to_int_iter()),
            },
            DataRefObject::ObjectReference(_) => None,
        }
    }

    pub fn get_const_data(&self) -> Option<AcpiInt> {
        match self {
            DataRefObject::DataObject(d) => match d {
                DataObject::ComputationalData(c) => match c {
                    ComputationalData::ConstData(c) => Some(*c as AcpiInt),
                    ComputationalData::StringData(_) => None,
                    ComputationalData::ConstObj(c) => Some(*c as AcpiInt),
                    ComputationalData::Revision => None,
                    ComputationalData::DefBuffer(_) => None,
                },
                DataObject::DefPackage(_) => None,
                DataObject::DefVarPackage(_) => None,
            },
            DataRefObject::ObjectReference(_) => {
                unimplemented!()
            }
        }
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
