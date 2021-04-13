//!
//! ACPI Machine Language  Expression Opcodes
//!
//!
#![allow(dead_code)]

use super::data_object::{PackageElement, PkgLength};
use super::name_object::{NameString, SimpleName, SuperName, Target};
use super::opcode;
use super::parser::ParseHelper;
use super::term_object::{MethodInvocation, TermArg};
use super::{AcpiInt, AmlError, AmlStream, DataRefObject, IntIter};

use crate::ignore_invalid_type_error;

#[derive(Debug, Clone)]
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

    pub fn get_number_of_remaining_elements(&self) -> usize {
        self.num_elements
    }

    pub fn get_next_element(
        &mut self,
        current_scope: &NameString,
    ) -> Result<Option<PackageElement>, AmlError> {
        if self.num_elements == 0 {
            return Ok(None);
        }
        let backup = self.stream.clone();
        ignore_invalid_type_error!(DataRefObject::parse(&mut self.stream, current_scope), |e| {
            self.num_elements -= 1;
            return Ok(Some(PackageElement::DataRefObject(e)));
        });
        self.stream.roll_back(&backup);
        let name_string = NameString::parse(&mut self.stream, Some(current_scope))?;
        self.num_elements -= 1;
        return Ok(Some(PackageElement::NameString(name_string)));
    }

    pub fn to_int_iter(&self) -> IntIter {
        IntIter::new(self.stream.clone(), self.num_elements)
    }
}

#[derive(Debug, Clone)]
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

    pub fn to_int_iter(&self) -> IntIter {
        unimplemented!()
    }
}

#[derive(Debug, Clone)]
pub enum ObjReference {
    ObjectReference(AcpiInt),
    String(&'static str),
}

#[derive(Debug, Clone)]
pub struct Index {
    buffer_pkg_str_obj: TermArg,
    index: TermArg,
    target: SuperName,
}

impl Index {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* IndexOp was read */
        let buffer_pkg_str_obj = TermArg::try_parse(stream, current_scope, parse_helper)?;
        let index = TermArg::parse_integer(stream, current_scope, parse_helper)?;
        let target = SuperName::try_parse(stream, current_scope, parse_helper)?;
        Ok(Self {
            buffer_pkg_str_obj,
            index,
            target,
        })
    }
}

#[derive(Debug, Clone)]
pub enum ReferenceTypeOpcode {
    DefRefOf(SuperName),
    DefDerefOf(TermArg),
    DefIndex(Index),
    UserTermObj, /* What is this? */
}

impl ReferenceTypeOpcode {
    pub fn try_parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        match stream.peek_byte()? {
            opcode::REF_OF_OP => {
                stream.seek(1)?;
                Ok(Self::DefRefOf(SuperName::try_parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            opcode::DEREF_OF_OP => {
                stream.seek(1)?;
                Ok(Self::DefDerefOf(TermArg::try_parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            opcode::INDEX_OP => {
                stream.seek(1)?;
                Ok(Self::DefIndex(Index::parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            _ => Err(AmlError::InvalidType),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ByteList {
    stream: AmlStream,
    current_scope: NameString,
}

impl ByteList {
    pub fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
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

    pub fn get_buffer_size(&mut self, parse_helper: &mut ParseHelper) -> Result<TermArg, AmlError> {
        TermArg::parse_integer(&mut self.stream, &self.current_scope, parse_helper)
    }

    pub fn read_next(&mut self) -> Result<u8, AmlError> {
        self.stream.read_byte()
    }
}

#[derive(Debug, Clone)]
pub struct BinaryOperation {
    opcode: u8,
    operand1: Operand,
    operand2: Operand,
    target: Target,
}

impl BinaryOperation {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        let opcode = stream.read_byte()?;
        let operand1 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
        let operand2 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
        let target = Target::parse(stream, current_scope, parse_helper)?;
        Ok(Self {
            opcode,
            operand1,
            operand2,
            target,
        })
    }

    pub const fn get_left_operand(&self) -> &Operand {
        &self.operand1
    }

    pub const fn get_right_operand(&self) -> &Operand {
        &self.operand2
    }

    pub const fn get_target(&self) -> &Target {
        &self.target
    }

    pub const fn get_opcode(&self) -> u8 {
        self.opcode
    }
}

#[derive(Debug, Clone)]
enum ConcatDataType {
    ComputationalData(TermArg),
    Buffer(TermArg),
}

#[derive(Debug, Clone)]
pub struct Concat {
    data1: ConcatDataType,
    data2: ConcatDataType,
    target: Target,
}

impl Concat {
    fn parse_concat(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* ConcatOp was read */
        let data1 = TermArg::try_parse(stream, current_scope, parse_helper)?;
        let data2 = TermArg::try_parse(stream, current_scope, parse_helper)?;
        let target = Target::parse(stream, current_scope, parse_helper)?;
        Ok(Self {
            data1: ConcatDataType::ComputationalData(data1),
            data2: ConcatDataType::ComputationalData(data2),
            target,
        })
    }

    fn parse_concat_res(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* ConcatResOp was read */
        let data1 = TermArg::try_parse(stream, current_scope, parse_helper)?;
        let data2 = TermArg::try_parse(stream, current_scope, parse_helper)?;
        let target = Target::parse(stream, current_scope, parse_helper)?;
        Ok(Self {
            data1: ConcatDataType::Buffer(data1),
            data2: ConcatDataType::Buffer(data2),
            target,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Divide {
    dividend: TermArg,
    divisor: TermArg,
    remainder: Target,
    quotient: Target,
}

impl Divide {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* DivideOp was read */
        let dividend = TermArg::parse_integer(stream, current_scope, parse_helper)?;
        let divisor = TermArg::parse_integer(stream, current_scope, parse_helper)?;
        let remainder = Target::parse(stream, current_scope, parse_helper)?;
        let quotient = Target::parse(stream, current_scope, parse_helper)?;
        Ok(Self {
            dividend,
            divisor,
            remainder,
            quotient,
        })
    }

    pub fn get_dividend(&self) -> &TermArg {
        &self.dividend
    }

    pub fn get_divisor(&self) -> &TermArg {
        &self.divisor
    }

    pub fn get_remainder(&self) -> &Target {
        &self.remainder
    }

    pub fn get_quotient(&self) -> &Target {
        &self.quotient
    }
}

#[derive(Debug, Clone)]
pub struct Match {
    search_pkg: TermArg,
    match_opcode_1: u8,
    operand_1: Operand,
    match_opcode_2: u8,
    operand_2: Operand,
    start_index: TermArg,
}

impl Match {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* MatchOp was read */
        let search_pkg = TermArg::try_parse(stream, current_scope, parse_helper)?;
        let match_opcode_1 = stream.read_byte()?;
        let operand_1 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
        let match_opcode_2 = stream.read_byte()?;
        let operand_2 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
        let start_index = TermArg::parse_integer(stream, current_scope, parse_helper)?;
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

#[derive(Debug, Clone)]
pub struct Mid {
    mid_obj: TermArg,
    term_arg_1: TermArg,
    term_arg_2: TermArg,
    target: Target,
}

impl Mid {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* MidOp was read */
        let mid_obj = TermArg::try_parse(stream, current_scope, parse_helper)?;
        let term_arg_1 = TermArg::try_parse(stream, current_scope, parse_helper)?;
        let term_arg_2 = TermArg::try_parse(stream, current_scope, parse_helper)?;
        let target = Target::parse(stream, current_scope, parse_helper)?;
        Ok(Self {
            mid_obj,
            term_arg_1,
            term_arg_2,
            target,
        })
    }
}

#[derive(Debug, Clone)]
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
    DefNot((Operand, Target)),
    DefObjectType(SuperName),
    DefPackage(Package),
    DefVarPackage(VarPackage),
    DefSizeOf(SuperName),
    DefStore((TermArg, SuperName)),
    DefTimer,
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
    pub fn try_parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* println!(
            "ExpressionOpcode: {:#X},{:#X}",
            stream.peek_byte()?,
            stream.peek_byte_with_pos(1)?
        ); */
        match stream.peek_byte()? {
            opcode::EXT_OP_PREFIX => match stream.peek_byte_with_pos(1)? {
                opcode::ACQUIRE_OP => {
                    stream.seek(2)?;
                    let mutex_object = SuperName::try_parse(stream, current_scope, parse_helper)?;
                    let timeout = stream.read_word()?;
                    Ok(Self::DefAcquire((mutex_object, timeout)))
                }
                opcode::COND_REF_OF_OP => {
                    stream.seek(2)?;
                    let source = SuperName::try_parse(stream, current_scope, parse_helper)?;
                    let result = Target::parse(stream, current_scope, parse_helper)?;
                    Ok(Self::DefCondRefOf((source, result)))
                }
                opcode::FROM_BCD_OP => {
                    stream.seek(2)?;
                    let bcd_value = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                    let target = Target::parse(stream, current_scope, parse_helper)?;
                    Ok(Self::DefFromBCD((bcd_value, target)))
                }

                opcode::LOAD_OP => {
                    stream.seek(2)?;
                    let name = NameString::parse(stream, Some(&current_scope))?;
                    let target = Target::parse(stream, current_scope, parse_helper)?;
                    Ok(Self::DefLoad((name, target)))
                }
                opcode::LOAD_TABLE_OP => {
                    stream.seek(2)?;
                    let mut table: [TermArg; 6] =
                        unsafe { core::mem::MaybeUninit::uninit().assume_init() };
                    for i in 0..table.len() {
                        /* Using this style instead of Iter */
                        table[i] = TermArg::try_parse(stream, current_scope, parse_helper)?;
                    }
                    Ok(Self::DefLoadTable(table))
                }
                opcode::TIMER_OP => {
                    stream.seek(2)?;
                    Ok(Self::DefTimer)
                }
                opcode::TO_BCD_OP => {
                    stream.seek(2)?;
                    let operand = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                    let target = Target::parse(stream, current_scope, parse_helper)?;
                    Ok(Self::DefToBCD((operand, target)))
                }
                opcode::PROCESSOR_OP => {
                    stream.seek(2)?;
                    let pkg_length = PkgLength::parse(stream)?;
                    stream.seek(pkg_length.actual_length)?;
                    Ok(Self::DefProcessor) /* Ignore(From ACPI 6.4, DefProcessor was deleted) */
                }
                opcode::WAIT_OP => {
                    stream.seek(2)?;
                    let event_object = SuperName::try_parse(stream, current_scope, parse_helper)?;
                    let operand = TermArg::parse_integer(stream, current_scope, parse_helper)?;
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
                parse_helper,
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
                    parse_helper,
                )?))
            }
            opcode::CONCAT_RES_OP => {
                stream.seek(1)?;
                Ok(Self::DefConcatRes(Concat::parse_concat_res(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            opcode::COPY_OBJECT_OP => {
                stream.seek(1)?;
                let term_arg = TermArg::try_parse(stream, current_scope, parse_helper)?;
                let simple_name = SimpleName::parse(stream, current_scope)?;
                Ok(Self::DefCopyObject(term_arg, simple_name))
            }
            opcode::DECREMENT_OP => {
                stream.seek(1)?;
                Ok(Self::DefDecrement(SuperName::try_parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            opcode::DIVIDE_OP => {
                stream.seek(1)?;
                Ok(Self::DefDivide(Divide::parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            opcode::FIND_SET_LEFT_BIT_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let target = Target::parse(stream, current_scope, parse_helper)?;
                Ok(Self::DefFindSetLeftBit((operand, target)))
            }
            opcode::FIND_SET_RIGHT_BIT_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let target = Target::parse(stream, current_scope, parse_helper)?;
                Ok(Self::DefFindSetRightBit((operand, target)))
            }
            opcode::INCREMENT_OP => {
                stream.seek(1)?;
                Ok(Self::DefIncrement(SuperName::try_parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            opcode::L_AND_OP => {
                stream.seek(1)?;
                let operand1 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let operand2 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                Ok(Self::DefLAnd((operand1, operand2)))
            }
            opcode::L_EQUAL_OP => {
                stream.seek(1)?;
                let operand1 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let operand2 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                Ok(Self::DefLEqual((operand1, operand2)))
            }
            opcode::L_GREATER_OP => {
                stream.seek(1)?;
                let operand1 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let operand2 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                Ok(Self::DefLGreater((operand1, operand2)))
            }
            opcode::L_LESS_OP => {
                stream.seek(1)?;
                let operand1 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let operand2 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                Ok(Self::DefLLess((operand1, operand2)))
            }
            opcode::L_NOT_OP => {
                stream.seek(1)?;
                match stream.peek_byte()? {
                    opcode::L_LESS_OP => {
                        stream.seek(1)?;
                        let operand1 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                        let operand2 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                        Ok(Self::DefLGreaterEqual((operand1, operand2)))
                    }
                    opcode::L_GREATER_OP => {
                        stream.seek(1)?;
                        let operand1 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                        let operand2 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                        Ok(Self::DefLLessEqual((operand1, operand2)))
                    }
                    opcode::L_EQUAL_OP => {
                        stream.seek(1)?;
                        let operand1 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                        let operand2 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                        Ok(Self::DefLNotEqual((operand1, operand2)))
                    }
                    _ => Ok(Self::DefLNot(TermArg::parse_integer(
                        stream,
                        current_scope,
                        parse_helper,
                    )?)),
                }
            }
            opcode::L_OR_OP => {
                stream.seek(1)?;
                let operand1 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let operand2 = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                Ok(Self::DefLOr((operand1, operand2)))
            }
            opcode::MATCH_OP => {
                stream.seek(1)?;
                Ok(Self::DefMatch(Match::parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            opcode::MID_OP => {
                stream.seek(1)?;
                Ok(Self::DefMid(Mid::parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            opcode::NOT_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let target = Target::parse(stream, current_scope, parse_helper)?;
                Ok(Self::DefNot((operand, target)))
            }
            opcode::OBJECT_TYPE_OP => {
                stream.seek(1)?;
                Ok(Self::DefObjectType(SuperName::try_parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            opcode::VAR_PACKAGE_OP => {
                /* OpCode will be read in VarPackage::try_parse */
                Ok(Self::DefVarPackage(VarPackage::try_parse(stream)?))
            }
            opcode::PACKAGE_OP => {
                /* OpCode will be read in Package::try_parse */
                Ok(Self::DefPackage(Package::try_parse(stream)?))
            }
            opcode::SIZE_OF_OP => {
                stream.seek(1)?;
                Ok(Self::DefSizeOf(SuperName::try_parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            opcode::STORE_OP => {
                stream.seek(1)?;
                let term_arg = TermArg::try_parse(stream, current_scope, parse_helper)?;
                let super_name = SuperName::try_parse(stream, current_scope, parse_helper)?;
                Ok(Self::DefStore((term_arg, super_name)))
            }
            opcode::TO_BUFFER_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let target = Target::parse(stream, current_scope, parse_helper)?;
                Ok(Self::DefToBuffer((operand, target)))
            }
            opcode::TO_DECIMAL_STRING_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let target = Target::parse(stream, current_scope, parse_helper)?;
                Ok(Self::DefToDecimalString((operand, target)))
            }
            opcode::TO_HEX_STRING_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let target = Target::parse(stream, current_scope, parse_helper)?;
                Ok(Self::DefToHexString((operand, target)))
            }
            opcode::TO_INTEGER_OP => {
                stream.seek(1)?;
                let operand = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let target = Target::parse(stream, current_scope, parse_helper)?;
                Ok(Self::DefToInteger((operand, target)))
            }
            opcode::TO_STRING_OP => {
                stream.seek(1)?;
                let term_arg = TermArg::try_parse(stream, current_scope, parse_helper)?;
                let length_arg = TermArg::parse_integer(stream, current_scope, parse_helper)?;
                let target = Target::parse(stream, current_scope, parse_helper)?;
                Ok(Self::DefToString(((term_arg, length_arg), target)))
            }
            _ => {
                ignore_invalid_type_error!(
                    ReferenceTypeOpcode::try_parse(stream, current_scope, parse_helper),
                    |r| {
                        return Ok(Self::ReferenceTypeOpcode(r));
                    }
                );
                Ok(Self::MethodInvocation(MethodInvocation::try_parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
        }
    }
}
