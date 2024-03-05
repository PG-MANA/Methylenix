//!
//! ACPI Machine Language Data Objects
//!

use super::expression_opcode;
use super::name_object::NameString;
use super::opcode;
use super::{AcpiInt, AmlError, AmlStream};

use crate::ignore_invalid_type_error;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

#[derive(Clone, Debug)]
pub struct PkgLength {
    pub length: usize,
    pub actual_length: usize,
}

#[derive(Debug, Copy, Clone, PartialOrd, PartialEq, Ord, Eq)]
pub enum ConstData {
    Byte(u8),
    Word(u16),
    DWord(u32),
    QWord(u64),
}

#[derive(Debug, Clone)]
pub enum ComputationalData {
    ConstData(ConstData),
    StringData(&'static str),
    ConstObj(u8),
    Revision,
    DefBuffer(expression_opcode::ByteList),
}

#[derive(Debug, Clone)]
pub enum PackageElement {
    DataRefObject(DataRefObject),
    NameString(NameString),
}

impl ConstData {
    pub fn to_int(&self) -> AcpiInt {
        match self {
            ConstData::Byte(b) => *b as AcpiInt,
            ConstData::Word(w) => *w as AcpiInt,
            ConstData::DWord(d) => *d as AcpiInt,
            ConstData::QWord(q) => *q as AcpiInt,
        }
    }

    pub fn get_byte_size(&self) -> usize {
        match self {
            ConstData::Byte(_) => 1,
            ConstData::Word(_) => 2,
            ConstData::DWord(_) => 4,
            ConstData::QWord(_) => 8,
        }
    }

    pub fn from_usize(data: usize, byte_size: usize) -> Result<Self, AmlError> {
        match byte_size {
            1 => {
                if data > u8::MAX as _ {
                    Self::from_usize(data, 2)
                } else {
                    Ok(ConstData::Byte(data as _))
                }
            }
            2 => {
                if data > u16::MAX as _ {
                    Self::from_usize(data, 4)
                } else {
                    Ok(ConstData::Word(data as _))
                }
            }
            4 => {
                if data > u32::MAX as _ {
                    Self::from_usize(data, 8)
                } else {
                    Ok(ConstData::DWord(data as _))
                }
            }
            8 => Ok(ConstData::QWord(data as _)),
            _ => Err(AmlError::InvalidType),
        }
    }
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
            Self::BYTE_PREFIX => Ok(Self::ConstData(ConstData::Byte(stream.read_byte()?))),
            Self::WORD_PREFIX => Ok(Self::ConstData(ConstData::Word(stream.read_word()?))),
            Self::DWORD_PREFIX => Ok(Self::ConstData(ConstData::DWord(stream.read_dword()?))),
            Self::QWORD_PREFIX => Ok(Self::ConstData(ConstData::QWord(stream.read_qword()?))),
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

    pub fn get_const_data(&self) -> Option<AcpiInt> {
        match self {
            DataRefObject::DataObject(d) => match d {
                DataObject::ComputationalData(c) => match c {
                    ComputationalData::ConstData(c) => Some(c.to_int()),
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

pub fn parse_integer_from_buffer(buffer: &[u8]) -> Result<AcpiInt, AmlError> {
    parse_integer(&mut AmlStream::new(
        VAddress::new(buffer.as_ptr() as usize),
        MSize::new(buffer.len()),
    ))
}

pub fn parse_integer(stream: &mut AmlStream) -> Result<AcpiInt, AmlError> {
    let mut val = 0;
    let mut c = stream.read_byte()? as char;
    if c == '0' {
        c = stream.read_byte()? as char;
        if c == 'x' {
            loop {
                c = stream.peek_byte()? as char;
                if c.is_ascii_digit() {
                    val <<= 4; /* val *= 0x10 */
                    val += c as usize - 0x30/* '0' */;
                } else if ('a'..='f').contains(&c) {
                    val <<= 4; /* val *= 0x10 */
                    val += c as usize - 0x61/* 'a' */ + 0xa;
                } else {
                    return if val == 0 {
                        Err(AmlError::InvalidType)
                    } else {
                        Ok(val)
                    };
                }
                stream.seek(1)?;
            }
        } else if c == 'X' {
            loop {
                c = stream.peek_byte()? as char;
                if c.is_ascii_digit() {
                    val <<= 4; /* val *= 0x10 */
                    val += c as usize - 0x30/* '0' */;
                } else if ('A'..='F').contains(&c) {
                    val <<= 4; /* val *= 0x10 */
                    val += c as usize - 0x41/* 'A' */ + 0xa;
                } else {
                    return if val == 0 {
                        Err(AmlError::InvalidType)
                    } else {
                        Ok(val)
                    };
                }
                stream.seek(1)?;
            }
        } else {
            loop {
                c = stream.peek_byte()? as char;
                if ('0'..='7').contains(&c) {
                    val <<= 3; /* val *= 0o10 */
                    val += c as usize - 0x30 /* '0' */;
                } else {
                    return if val == 0 {
                        Err(AmlError::InvalidType)
                    } else {
                        Ok(val)
                    };
                }
                stream.seek(1)?;
            }
        }
    } else {
        loop {
            c = stream.peek_byte()? as char;
            if c.is_ascii_digit() {
                val *= 10;
                val += c as usize - 0x30/* '0' */;
            } else {
                return if val == 0 {
                    Err(AmlError::InvalidType)
                } else {
                    Ok(val)
                };
            }
            stream.seek(1)?;
        }
    }
}

pub fn eisa_id_to_dword(id: &[u8; 7]) -> u32 {
    let to_compressed_mfg_code = |c: u8| -> u32 { ((c - 0x40) & 0b11111) as u32 };
    let to_hex = |c: u8| -> u32 {
        (if c.is_ascii_digit() {
            c - b'0'
        } else {
            c - b'A' + 0xA
        }) as u32
    };

    ((to_compressed_mfg_code(id[0]) << 2) | (to_compressed_mfg_code(id[1]) >> 3))
        | ((((to_compressed_mfg_code(id[1]) & 0b111) << 5) | to_compressed_mfg_code(id[2])) << 8)
        | (((to_hex(id[3]) << 4) | to_hex(id[4])) << 16)
        | (((to_hex(id[5]) << 4) | to_hex(id[6])) << 24)
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
