//!
//! ACPI Machine Language  Expression Opcodes
//!
//!
#![allow(dead_code)]

use super::data_object::{NameString, PkgLength, SimpleName, SuperName, Target};
use super::named_object::FieldList;
use super::opcode;
use super::term_object::{MethodInvocation, TermArg, TermList};
use super::{AcpiInt, AmlError, AmlStream};

use crate::ignore_invalid_type_error;

#[derive(Debug)]
pub struct Package {
    num_elements: AcpiInt,
    stream: AmlStream,
}

type Operand = TermArg;
type BCDValue = TermArg;

impl Package {
    pub fn try_parse(stream: &mut AmlStream) -> Result<Self, AmlError> {
        if stream.peek_byte()? == opcode::PACKAGE_OP {
            stream.read_byte()?;
            let pkg_length = PkgLength::parse(stream)?;
            let mut element_list_stream = stream.clone();
            stream.seek(pkg_length.actual_length)?;
            drop(stream); /* Avoid using this */
            element_list_stream.change_size(pkg_length.actual_length)?;
            let num_elements = element_list_stream.read_byte()?;
            Ok(Self {
                num_elements: num_elements as AcpiInt,
                stream: element_list_stream,
            })
        } else {
            Err(AmlError::InvalidType)
        }
    }
}

#[derive(Debug)]
pub struct VarPackage {
    stream: AmlStream,
}

impl VarPackage {
    pub fn try_parse(stream: &mut AmlStream) -> Result<Self, AmlError> {
        if stream.peek_byte()? == opcode::VAR_PACKAGE_OP {
            stream.read_byte()?;
            let pkg_length = PkgLength::parse(stream)?;
            let mut element_list_stream = stream.clone();
            stream.seek(pkg_length.actual_length)?;
            drop(stream); /* Avoid using this */
            element_list_stream.change_size(pkg_length.actual_length)?;
            Ok(Self {
                stream: element_list_stream,
            })
        } else {
            Err(AmlError::InvalidType)
        }
    }
}

#[derive(Debug)]
pub enum ObjReference {
    ObjectReference(AcpiInt),
    String(&'static str),
}

#[derive(Debug)]
pub struct Index {
    buffer_pkg_str_obj: TermArg,
    index: TermArg,
    target: SuperName,
}

impl Index {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* IndexOp was read */
        let buffer_pkg_str_obj = TermArg::parse(stream, current_scope)?;
        let index = TermArg::parse_integer(stream, current_scope)?;
        let target = SuperName::parse(stream, current_scope)?;
        Ok(Self {
            buffer_pkg_str_obj,
            index,
            target,
        })
    }
}

#[derive(Debug)]
pub enum ReferenceTypeOpcode {
    DefRefOf(SuperName),
    DefDerefOf(TermArg),
    DefIndex(Index),
    UserTermObj, /* What is this? */
}

impl ReferenceTypeOpcode {
    pub fn try_parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        match stream.peek_byte()? {
            opcode::REF_OF_OP => {
                stream.seek(1)?;
                Ok(Self::DefRefOf(SuperName::parse(stream, current_scope)?))
            }
            opcode::DEREF_OF_OP => {
                stream.seek(1)?;
                Ok(Self::DefDerefOf(TermArg::parse(stream, current_scope)?))
            }
            opcode::INDEX_OP => {
                stream.seek(1)?;
                Ok(Self::DefIndex(Index::parse(stream, current_scope)?))
            }
            _ => Err(AmlError::InvalidType),
        }
    }
}

#[derive(Debug)]
pub struct ByteList {
    stream: AmlStream,
    current_scope: NameString,
}

impl ByteList {
    pub(crate) fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
    ) -> Result<Self, AmlError> {
        /* BufferOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut byte_list_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        drop(stream); /* Avoid using this */
        byte_list_stream.change_size(pkg_length.actual_length)?;
        //let buffer_size = TermArg::parse_integer(&mut byte_list_stream,current_scope)?;
        /* ATTENTION: When eval, buffer_size must be eval first. */
        Ok(Self {
            stream: byte_list_stream,
            current_scope: current_scope.clone(),
        })
    }
}

#[derive(Debug)]
pub struct BinaryOperation {
    opcode: u8,
    operand1: Operand,
    operand2: Operand,
    target: Target,
}

impl BinaryOperation {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        let opcode = stream.read_byte()?;
        let operand1 = TermArg::parse_integer(stream, current_scope)?;
        let operand2 = TermArg::parse_integer(stream, current_scope)?;
        let target = Target::parse(stream, current_scope)?;
        Ok(Self {
            opcode,
            operand1,
            operand2,
            target,
        })
    }
}

#[derive(Debug)]
enum ConcatDataType {
    ComputationalData(TermArg),
    Buffer(TermArg),
}

#[derive(Debug)]
pub struct Concat {
    data1: ConcatDataType,
    data2: ConcatDataType,
    target: Target,
}

impl Concat {
    fn parse_concat(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* ConcatOp was read */
        let data1 = TermArg::parse(stream, current_scope)?;
        let data2 = TermArg::parse(stream, current_scope)?;
        let target = Target::parse(stream, current_scope)?;
        Ok(Self {
            data1: ConcatDataType::ComputationalData(data1),
            data2: ConcatDataType::ComputationalData(data2),
            target,
        })
    }

    fn parse_concat_res(
        stream: &mut AmlStream,
        current_scope: &NameString,
    ) -> Result<Self, AmlError> {
        /* ConcatResOp was read */
        let data1 = TermArg::parse(stream, current_scope)?;
        let data2 = TermArg::parse(stream, current_scope)?;
        let target = Target::parse(stream, current_scope)?;
        Ok(Self {
            data1: ConcatDataType::Buffer(data1),
            data2: ConcatDataType::Buffer(data2),
            target,
        })
    }
}

#[derive(Debug)]
pub struct Divide {
    dividend: TermArg,
    divisor: TermArg,
    remainder: Target,
    quotient: Target,
}

impl Divide {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* DivideOp was read */
        let dividend = TermArg::parse_integer(stream, current_scope)?;
        let divisor = TermArg::parse_integer(stream, current_scope)?;
        let remainder = Target::parse(stream, current_scope)?;
        let quotient = Target::parse(stream, current_scope)?;
        Ok(Self {
            dividend,
            divisor,
            remainder,
            quotient,
        })
    }
}

#[derive(Debug)]
pub struct Match {
    search_pkg: TermArg,
    match_opcode_1: u8,
    operand_1: Operand,
    match_opcode_2: u8,
    operand_2: Operand,
    start_index: TermArg,
}

impl Match {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* MatchOp was read */
        let search_pkg = TermArg::parse(stream, current_scope)?;
        let match_opcode_1 = stream.read_byte()?;
        let operand_1 = TermArg::parse_integer(stream, current_scope)?;
        let match_opcode_2 = stream.read_byte()?;
        let operand_2 = TermArg::parse_integer(stream, current_scope)?;
        let start_index = TermArg::parse_integer(stream, current_scope)?;
        Ok(Self {
            search_pkg,
            match_opcode_1,
            operand_1,
            match_opcode_2,
            operand_2,
            start_index,
        })
    }
}

#[derive(Debug)]
pub struct Mid {
    mid_obj: TermArg,
    term_arg_1: TermArg,
    term_arg_2: TermArg,
    target: Target,
}

impl Mid {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* MidOp was read */
        let mid_obj = TermArg::parse(stream, current_scope)?;
        let term_arg_1 = TermArg::parse(stream, current_scope)?;
        let term_arg_2 = TermArg::parse(stream, current_scope)?;
        let target = Target::parse(stream, current_scope)?;
        Ok(Self {
            mid_obj,
            term_arg_1,
            term_arg_2,
            target,
        })
    }
}

#[derive(Debug)]
pub struct Method {
    name: NameString,
    method_flags: u8,
    field_list: TermList,
}

impl Method {
    pub(crate) fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
    ) -> Result<Self, AmlError> {
        /* MethodOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut method_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        drop(stream); /* Avoid using this */
        method_stream.change_size(pkg_length.actual_length)?;
        let name = NameString::parse(&mut method_stream, Some(&current_scope))?;
        let field_flags = method_stream.read_byte()?;
        let term_list = TermList::new(method_stream, name.clone());
        Ok(Self {
            name,
            method_flags: field_flags,
            field_list: term_list,
        })
    }
}

#[derive(Debug)]
pub struct Field {
    name: NameString,
    field_flags: u8,
    field_list: FieldList,
}

impl Field {
    pub(crate) fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
    ) -> Result<Self, AmlError> {
        /* FieldOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut field_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        drop(stream); /* Avoid using this */
        field_stream.change_size(pkg_length.actual_length)?;
        let name = NameString::parse(&mut field_stream, Some(&current_scope))?;
        let field_flags = field_stream.read_byte()?;
        let field_list = FieldList::new(field_stream)?;
        Ok(Self {
            name,
            field_flags,
            field_list,
        })
    }
}

#[derive(Debug)]
pub struct IndexField {
    name1: NameString,
    name2: NameString,
    field_flags: u8,
    field_list: FieldList,
}

impl IndexField {
    pub(crate) fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
    ) -> Result<Self, AmlError> {
        /* IndexFieldOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut index_field_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        drop(stream); /* Avoid using this */
        index_field_stream.change_size(pkg_length.actual_length)?;
        let name1 = NameString::parse(&mut index_field_stream, Some(&current_scope))?;
        let name2 = NameString::parse(&mut index_field_stream, Some(&current_scope))?;
        let field_flags = index_field_stream.read_byte()?;
        let field_list = FieldList::new(index_field_stream)?;
        Ok(Self {
            name1,
            name2,
            field_flags,
            field_list,
        })
    }
}

#[derive(Debug)]
pub enum ExpressionOpcode {
    BinaryOperation(BinaryOperation),
    DefAcquire((SuperName, u16)),
    DefBuffer(ByteList),
    DefConcat(Concat),
    DefConcatRes(Concat),
    DefCondRefOf((SuperName, Target)),
    DefCopyObject(TermArg, SimpleName),
    DefDecrement(SuperName),
    DefDivide(Divide),
    DefFindSetLeftBit((Operand, Target)),
    DefFindSetRightBit((Operand, Target)),
    DefFromBCD((BCDValue, Target)),
    DefIncrement(SuperName),
    DefLAnd((Operand, Operand)),
    DefLEqual((Operand, Operand)),
    DefLGreater((Operand, Operand)),
    DefLGreaterEqual((Operand, Operand)),
    DefLLess((Operand, Operand)),
    DefLLessEqual((Operand, Operand)),
    DefLNot(Operand),
    DefLNotEqual((Operand, Operand)),
    DefLoad((NameString, Target)),
    DefLoadTable([TermArg; 6]),
    DefLOr((Operand, Operand)),
    DefMatch(Match),
    DefMid(Mid),
    DefNot(Target),
    DefObjectType(SuperName),
    DefVarPackage(VarPackage),
    DefSizeOf(SuperName),
    DefStore((TermArg, SuperName)),
    DefTimer,
    DefField(Field),
    DefDevice((NameString, TermList)),
    DefEvent(NameString),
    DefIndexField(IndexField),
    DefMethod(Method),
    DefMutex((NameString, u8)),
    DefProcessor,
    DefToBCD((Operand, Target)),
    DefToBuffer((Operand, Target)),
    DefToDecimalString((Operand, Target)),
    DefToHexString((Operand, Target)),
    DefToInteger((Operand, Target)),
    DefToString(((TermArg, TermArg), Target)),
    DefWait((SuperName, Operand)),
    ReferenceTypeOpcode(ReferenceTypeOpcode),
    MethodInvocation(MethodInvocation),
}

impl ExpressionOpcode {
    pub fn try_parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* println!(
            "ExpressionOpcode: {:#X},{:#X}",
            stream.peek_byte()?,
            stream.peek_byte_with_pos(1)?
        ); */
        match stream.peek_byte()? {
            opcode::EXT_OP_PREFIX => match stream.peek_byte_with_pos(1)? {
                opcode::ACQUIRE_OP => {
                    stream.seek(2)?;
                    let mutex_object = SuperName::parse(stream, current_scope)?;
                    let timeout = stream.read_word()?;
                    Ok(Self::DefAcquire((mutex_object, timeout)))
                }
                opcode::COND_REF_OF_OP => {
                    stream.seek(2)?;
                    let super_name = SuperName::parse(stream, current_scope)?;
                    let target = Target::parse(stream, current_scope)?;
                    Ok(Self::DefCondRefOf((super_name, target)))
                }
                opcode::FROM_BCD_OP => {
                    stream.seek(2)?;
                    let bcd_value = TermArg::parse_integer(stream, current_scope)?;
                    let target = Target::parse(stream, current_scope)?;
                    Ok(Self::DefFromBCD((bcd_value, target)))
                }
                opcode::FIELD_OP => {
                    stream.seek(2)?;
                    Ok(Self::DefField(Field::parse(stream, current_scope)?))
                }
                opcode::INDEX_FIELD_OP => {
                    stream.seek(2)?;
                    Ok(Self::DefIndexField(IndexField::parse(
                        stream,
                        current_scope,
                    )?))
                }
                opcode::DEVICE_OP => {
                    stream.seek(2)?;
                    let pkg_length = PkgLength::parse(stream)?;
                    let mut device_stream = stream.clone();
                    stream.seek(pkg_length.actual_length)?;
                    device_stream.change_size(pkg_length.actual_length)?;
                    let device_name = NameString::parse(&mut device_stream, Some(&current_scope))?;
                    let term_list = TermList::new(device_stream, device_name.clone());
                    Ok(Self::DefDevice((device_name, term_list)))
                }
                opcode::EVENT_OP => {
                    stream.seek(2)?;
                    Ok(Self::DefEvent(NameString::parse(
                        stream,
                        Some(current_scope),
                    )?))
                }
                opcode::LOAD_OP => {
                    stream.seek(2)?;
                    let name = NameString::parse(stream, Some(&current_scope))?;
                    let target = Target::parse(stream, current_scope)?;
                    Ok(Self::DefLoad((name, target)))
                }
                opcode::LOAD_TABLE_OP => {
                    stream.seek(2)?;
                    let mut table: [TermArg; 6] =
                        unsafe { core::mem::MaybeUninit::uninit().assume_init() };
                    for i in 0..table.len() {
                        /* Using this style instead of Iter */
                        table[i] = TermArg::parse(stream, current_scope)?;
                    }
                    Ok(Self::DefLoadTable(table))
                }
                opcode::TIMER_OP => {
                    stream.seek(2)?;
                    Ok(Self::DefTimer)
                }
                opcode::TO_BCD_OP => {
                    stream.seek(2)?;
                    let operand = TermArg::parse_integer(stream, current_scope)?;
                    let target = Target::parse(stream, current_scope)?;
                    Ok(Self::DefToBCD((operand, target)))
                }
                opcode::MUTEX_OP => {
                    stream.seek(2)?;
                    let name = NameString::parse(stream, Some(current_scope))?;
                    let flags = stream.read_byte()?;
                    Ok(Self::DefMutex((name, flags)))
                }
                opcode::PROCESSOR_OP => {
                    stream.seek(2)?;
                    let pkg_length = PkgLength::parse(stream)?;
                    stream.seek(pkg_length.actual_length)?;
                    Ok(Self::DefProcessor) /* Ignore(From ACPI 6.4, DefProcessor was deleted) */
                }
                opcode::WAIT_OP => {
                    stream.seek(2)?;
                    let event_object = SuperName::parse(stream, current_scope)?;
                    let operand = TermArg::parse_integer(stream, current_scope)?;
                    Ok(Self::DefWait((event_object, operand)))
                }
                _ => Err(AmlError::InvalidType),
            },
            opcode::ADD_OP
            | opcode::AND_OP
            | opcode::MULTIPLY_OP
            | opcode::NAND_OP
            | opcode::MOD_OP
            | opcode::NOR_OP
            | opcode::OR_OP
            | opcode::SHIFT_LEFT_OP
            | opcode::SHIFT_RIGHT_OP
            | opcode::SUBTRACT_OP
            | opcode::XOR_OP => Ok(Self::BinaryOperation(BinaryOperation::parse(
                stream,
                current_scope,
            )?)),
            opcode::BUFFER_OP => {
                stream.seek(1)?;
                Ok(Self::DefBuffer(ByteList::parse(stream, current_scope)?))
            }
            opcode::CONCAT_OP => {
                stream.seek(1)?;
                Ok(Self::DefConcat(Concat::parse_concat(
                    stream,
                    current_scope,
                )?))
            }
            opcode::CONCAT_RES_OP => {
                stream.seek(1)?;
                Ok(Self::DefConcatRes(Concat::parse_concat_res(
                    stream,
                    current_scope,
                )?))
            }
            opcode::COPY_OBJECT_OP => {
                stream.seek(1)?;
                let term_arg = TermArg::parse(stream, current_scope)?;
                let simple_name = SimpleName::parse(stream, current_scope)?;
                Ok(Self::DefCopyObject(term_arg, simple_name))
            }
            opcode::DECREMENT_OP => {
                stream.seek(1)?;
                Ok(Self::DefDecrement(SuperName::parse(stream, current_scope)?))
            }
            opcode::DIVIDE_OP => {
                stream.seek(1)?;
                Ok(Self::DefDivide(Divide::parse(stream, current_scope)?))
            }
            opcode::FIND_SET_LEFT_BIT_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope)?;
                let target = Target::parse(stream, current_scope)?;
                Ok(Self::DefFindSetLeftBit((operand, target)))
            }
            opcode::FIND_SET_RIGHT_BIT_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope)?;
                let target = Target::parse(stream, current_scope)?;
                Ok(Self::DefFindSetRightBit((operand, target)))
            }
            opcode::INCREMENT_OP => {
                stream.seek(1)?;
                Ok(Self::DefIncrement(SuperName::parse(stream, current_scope)?))
            }
            opcode::L_AND_OP => {
                stream.seek(1)?;
                let operand1 = TermArg::parse_integer(stream, current_scope)?;
                let operand2 = TermArg::parse_integer(stream, current_scope)?;
                Ok(Self::DefLAnd((operand1, operand2)))
            }
            opcode::L_EQUAL_OP => {
                stream.seek(1)?;
                let operand1 = TermArg::parse_integer(stream, current_scope)?;
                let operand2 = TermArg::parse_integer(stream, current_scope)?;
                Ok(Self::DefLEqual((operand1, operand2)))
            }
            opcode::L_GREATER_OP => {
                stream.seek(1)?;
                let operand1 = TermArg::parse_integer(stream, current_scope)?;
                let operand2 = TermArg::parse_integer(stream, current_scope)?;
                Ok(Self::DefLGreater((operand1, operand2)))
            }
            opcode::L_LESS_OP => {
                stream.seek(1)?;
                let operand1 = TermArg::parse_integer(stream, current_scope)?;
                let operand2 = TermArg::parse_integer(stream, current_scope)?;
                Ok(Self::DefLLess((operand1, operand2)))
            }
            opcode::L_NOT_OP => {
                stream.seek(1)?;
                match stream.peek_byte()? {
                    opcode::L_LESS_OP => {
                        stream.seek(1)?;
                        let operand1 = TermArg::parse_integer(stream, current_scope)?;
                        let operand2 = TermArg::parse_integer(stream, current_scope)?;
                        Ok(Self::DefLGreaterEqual((operand1, operand2)))
                    }
                    opcode::L_GREATER_OP => {
                        stream.seek(1)?;
                        let operand1 = TermArg::parse_integer(stream, current_scope)?;
                        let operand2 = TermArg::parse_integer(stream, current_scope)?;
                        Ok(Self::DefLLessEqual((operand1, operand2)))
                    }
                    opcode::L_EQUAL_OP => {
                        stream.seek(1)?;
                        let operand1 = TermArg::parse_integer(stream, current_scope)?;
                        let operand2 = TermArg::parse_integer(stream, current_scope)?;
                        Ok(Self::DefLNotEqual((operand1, operand2)))
                    }
                    _ => Ok(Self::DefLNot(TermArg::parse_integer(
                        stream,
                        current_scope,
                    )?)),
                }
            }
            opcode::L_OR_OP => {
                stream.seek(1)?;
                let operand1 = TermArg::parse_integer(stream, current_scope)?;
                let operand2 = TermArg::parse_integer(stream, current_scope)?;
                Ok(Self::DefLOr((operand1, operand2)))
            }
            opcode::MATCH_OP => {
                stream.seek(1)?;
                Ok(Self::DefMatch(Match::parse(stream, current_scope)?))
            }
            opcode::MID_OP => {
                stream.seek(1)?;
                Ok(Self::DefMid(Mid::parse(stream, current_scope)?))
            }
            opcode::NOT_OP => {
                stream.seek(1)?;
                Ok(Self::DefNot(Target::parse(stream, current_scope)?))
            }
            opcode::OBJECT_TYPE_OP => {
                stream.seek(1)?;
                Ok(Self::DefObjectType(SuperName::parse(
                    stream,
                    current_scope,
                )?))
            }
            opcode::VAR_PACKAGE_OP => {
                /* OpCode will be read in VarPackage::try_parse */
                Ok(Self::DefVarPackage(VarPackage::try_parse(stream)?))
            }
            opcode::SIZE_OF_OP => {
                stream.seek(1)?;
                Ok(Self::DefSizeOf(SuperName::parse(stream, current_scope)?))
            }
            opcode::STORE_OP => {
                stream.seek(1)?;
                let term_arg = TermArg::parse(stream, current_scope)?;
                let super_name = SuperName::parse(stream, current_scope)?;
                Ok(Self::DefStore((term_arg, super_name)))
            }
            opcode::TO_BUFFER_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope)?;
                let target = Target::parse(stream, current_scope)?;
                Ok(Self::DefToBuffer((operand, target)))
            }
            opcode::TO_DECIMAL_STRING_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope)?;
                let target = Target::parse(stream, current_scope)?;
                Ok(Self::DefToDecimalString((operand, target)))
            }
            opcode::TO_HEX_STRING_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope)?;
                let target = Target::parse(stream, current_scope)?;
                Ok(Self::DefToHexString((operand, target)))
            }
            opcode::TO_INTEGER_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope)?;
                let target = Target::parse(stream, current_scope)?;
                Ok(Self::DefToInteger((operand, target)))
            }
            opcode::TO_STRING_OP => {
                stream.seek(1)?;
                let term_arg = TermArg::parse(stream, current_scope)?;
                let length_arg = TermArg::parse_integer(stream, current_scope)?;
                let target = Target::parse(stream, current_scope)?;
                Ok(Self::DefToString(((term_arg, length_arg), target)))
            }
            opcode::METHOD_OP => {
                stream.seek(1)?;
                Ok(Self::DefMethod(Method::parse(stream, current_scope)?))
            }
            _ => {
                ignore_invalid_type_error!(
                    ReferenceTypeOpcode::try_parse(stream, current_scope),
                    |r| {
                        return Ok(Self::ReferenceTypeOpcode(r));
                    }
                );
                Ok(Self::MethodInvocation(MethodInvocation::try_parse(
                    stream,
                    current_scope,
                )?))
            }
        }
    }
}
