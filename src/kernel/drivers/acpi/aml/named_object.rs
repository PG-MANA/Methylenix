//!
//! ACPI Machine Language Named Objects
//!
#![allow(dead_code)]
use super::data_object::{NameString, PkgLength};
use super::opcode;
use super::term_object::{TermArg, TermList};
use super::{AmlError, AmlStream};

#[derive(Debug)]
pub struct BankField {
    names: [NameString; 2],
    bank_value: TermArg,
    field_flags: u8,
    field_list: FieldList,
}

impl BankField {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* BankFieldOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut bank_field_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        drop(stream); /* Avoid using this */
        bank_field_stream.change_size(pkg_length.actual_length)?;
        let name1 = NameString::parse(&mut bank_field_stream, Some(&current_scope))?;
        let name2 = NameString::parse(&mut bank_field_stream, Some(&current_scope))?;
        let bank_value = TermArg::parse_integer(&mut bank_field_stream, current_scope)?;
        let field_flags = bank_field_stream.read_byte()?;
        let field_list = FieldList::new(bank_field_stream)?;
        Ok(Self {
            names: [name1, name2],
            bank_value,
            field_flags,
            field_list,
        })
    }
}

#[derive(Eq, PartialEq, Debug)]
enum CreateFieldType {
    Bit,
    Byte,
    Word,
    DWord,
    QWord,
    Other,
}

#[derive(Debug)]
pub struct CreateField {
    size: CreateFieldType,
    source_buffer: TermArg,
    index: TermArg,
    name: NameString,
    optional_size: Option<TermArg>,
}

impl CreateField {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        field_type: CreateFieldType,
    ) -> Result<Self, AmlError> {
        /* Op was read */
        let source_buffer = TermArg::parse(stream, current_scope)?;
        let index = TermArg::parse_integer(stream, current_scope)?;
        let optional_size = if field_type == CreateFieldType::Other {
            Some(TermArg::parse_integer(stream, current_scope)?)
        } else {
            None
        };
        let name = NameString::parse(stream, Some(current_scope))?;
        Ok(Self {
            size: field_type,
            source_buffer,
            index,
            name,
            optional_size,
        })
    }
}

#[derive(Debug)]
pub struct DataRegion {
    name: NameString,
    term_args: [TermArg; 3],
}

impl DataRegion {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* DataRegionOp was read */
        let name = NameString::parse(stream, Some(current_scope))?;
        let term_arg1 = TermArg::parse(stream, current_scope)?;
        let term_arg2 = TermArg::parse(stream, current_scope)?;
        let term_arg3 = TermArg::parse(stream, current_scope)?;
        Ok(Self {
            name,
            term_args: [term_arg1, term_arg2, term_arg3],
        })
    }
}

#[derive(Debug)]
pub struct External {
    name: NameString,
    object_type: u8,
    argument_count: u8,
}

impl External {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* ExternalOp was read */
        let name = NameString::parse(stream, Some(current_scope))?;
        let object_type = stream.read_byte()?;
        let argument_count = stream.read_byte()?;
        Ok(Self {
            name,
            object_type,
            argument_count,
        })
    }
}

#[derive(Debug)]
pub struct OpRegion {
    name: NameString,
    region_scope: u8,
    region_offset: TermArg,
    region_len: TermArg,
}

impl OpRegion {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* OpRegionOp was read */
        let name = NameString::parse(stream, Some(current_scope))?;
        let region_scope = stream.read_byte()?;
        let region_offset = TermArg::parse_integer(stream, current_scope)?;
        let region_len = TermArg::parse_integer(stream, current_scope)?;
        Ok(Self {
            name,
            region_scope,
            region_offset,
            region_len,
        })
    }
}

#[derive(Debug)]
pub struct PowerRes {
    name: NameString,
    system_level: u8,
    resource_order: u16,
    term_list: TermList,
}

impl PowerRes {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* PowerResOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut power_res_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        drop(stream); /* Avoid using this */
        power_res_stream.change_size(pkg_length.actual_length)?;

        let name = NameString::parse(&mut power_res_stream, Some(current_scope))?;
        let system_level = power_res_stream.read_byte()?;
        let resource_order = power_res_stream.read_word()?;
        let term_list = TermList::new(power_res_stream, name.clone());
        Ok(Self {
            name,
            system_level,
            resource_order,
            term_list,
        })
    }
}

#[derive(Debug)]
pub struct ThermalZone {
    name: NameString,
    term_list: TermList,
}

impl ThermalZone {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* PowerResOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut thermal_zone_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        drop(stream); /* Avoid using this */
        thermal_zone_stream.change_size(pkg_length.actual_length)?;

        let name = NameString::parse(&mut thermal_zone_stream, Some(current_scope))?;
        let term_list = TermList::new(thermal_zone_stream, name.clone());
        Ok(Self { name, term_list })
    }
}

#[derive(Debug)]
pub enum NamedObject {
    DefBankField(BankField),
    DefCreateField(CreateField),
    DefDataRegion(DataRegion),
    DefExternal(External),
    DefOpRegion(OpRegion),
    DefPowerRes(PowerRes),
    DefThermalZone(ThermalZone),
}

impl NamedObject {
    pub fn try_parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        let first_byte = stream.peek_byte()?;
        let second_byte = stream.peek_byte_with_pos(1)?;
        match first_byte {
            opcode::EXT_OP_PREFIX => {
                match second_byte {
                    opcode::BANK_FIELD_OP => {
                        /* DefBankField */
                        stream.seek(2)?;
                        Ok(Self::DefBankField(BankField::parse(stream, current_scope)?))
                    }
                    opcode::CREATE_FIELD_OP => {
                        /* DefCreateField */
                        stream.seek(2)?;
                        Ok(Self::DefCreateField(CreateField::parse(
                            stream,
                            current_scope,
                            CreateFieldType::Other,
                        )?))
                    }
                    opcode::DATA_REGION_OP => {
                        /* DefDataRegion */
                        stream.seek(2)?;
                        Ok(Self::DefDataRegion(DataRegion::parse(
                            stream,
                            current_scope,
                        )?))
                    }

                    opcode::OP_REGION_OP => {
                        /* DefOpRegion */
                        stream.seek(2)?;
                        Ok(Self::DefOpRegion(OpRegion::parse(stream, current_scope)?))
                    }
                    opcode::POWER_RES_OP => {
                        /* DefPowerRes */
                        stream.seek(2)?;
                        Ok(Self::DefPowerRes(PowerRes::parse(stream, current_scope)?))
                    }
                    opcode::THERMAL_ZONE_OP => {
                        /* DefThermalZone */
                        stream.seek(2)?;
                        Ok(Self::DefThermalZone(ThermalZone::parse(
                            stream,
                            current_scope,
                        )?))
                    }
                    _ => Err(AmlError::InvalidType),
                }
            }
            opcode::CREATE_BIT_FIELD_OP => {
                /* DefCreateBitField */
                stream.seek(1)?;
                Ok(Self::DefCreateField(CreateField::parse(
                    stream,
                    current_scope,
                    CreateFieldType::Bit,
                )?))
            }
            opcode::CREATE_BYTE_FIELD_OP => {
                /* DefCreateByteField */
                stream.seek(1)?;
                Ok(Self::DefCreateField(CreateField::parse(
                    stream,
                    current_scope,
                    CreateFieldType::Byte,
                )?))
            }
            opcode::CREATE_WORD_FIELD_OP => {
                /* DefCreateWordField */
                stream.seek(1)?;
                Ok(Self::DefCreateField(CreateField::parse(
                    stream,
                    current_scope,
                    CreateFieldType::Word,
                )?))
            }
            opcode::CREATE_DOUBLE_WORD_FIELD_OP => {
                /* DefCreateDWordField */
                stream.seek(1)?;
                Ok(Self::DefCreateField(CreateField::parse(
                    stream,
                    current_scope,
                    CreateFieldType::DWord,
                )?))
            }
            opcode::CREATE_QUAD_WORD_FIELD_OP => {
                /* DefCreateQWordField */
                stream.seek(1)?;
                Ok(Self::DefCreateField(CreateField::parse(
                    stream,
                    current_scope,
                    CreateFieldType::QWord,
                )?))
            }
            opcode::EXTERNAL_OP => {
                /* DefExternal */
                stream.seek(1)?;
                Ok(Self::DefExternal(External::parse(stream, current_scope)?))
            }
            _ => Err(AmlError::InvalidType),
        }
    }
}

#[derive(Debug)]
pub struct FieldList {
    stream: AmlStream,
}

impl FieldList {
    pub fn new(stream: AmlStream) -> Result<Self, AmlError> {
        Ok(Self { stream })
    }
}

enum FieldElement {}
