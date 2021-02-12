//!
//! ACPI Machine Language Term Objects
//!
#![allow(dead_code)]
use super::data_object::{ComputationalData, DataObject, NameString};
use super::expression_opcode::ExpressionOpcode;
use super::named_object::NamedObject;
use super::namespace_modifier_object::NamespaceModifierObject;
use super::opcode;
use super::statement_opcode::StatementOpcode;
use super::{AmlError, AmlStream};

use crate::ignore_invalid_type_error;

use alloc::boxed::Box;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct TermList {
    stream: AmlStream,
    current_scope: NameString,
}

impl TermList {
    pub const fn new(stream: AmlStream, current_scope: NameString) -> Self {
        Self {
            stream,
            current_scope,
        }
    }

    pub fn is_end_of_stream(&self) -> bool {
        self.stream.is_end_of_stream()
    }
}

impl Iterator for TermList {
    type Item = Result<TermObj, AmlError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.stream.is_end_of_stream() {
            None
        } else {
            Some(TermObj::parse(&mut self.stream, &self.current_scope))
        }
    }
}

#[derive(Debug)]
pub enum TermObj {
    NamespaceModifierObj(NamespaceModifierObject),
    NamedObj(NamedObject),
    StatementOpcode(StatementOpcode),
    ExpressionOpcode(ExpressionOpcode),
}

impl TermObj {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        ignore_invalid_type_error!(
            NamespaceModifierObject::try_parse(stream, current_scope),
            |o| {
                return Ok(Self::NamespaceModifierObj(o));
            }
        );
        ignore_invalid_type_error!(NamedObject::try_parse(stream, current_scope), |o| {
            return Ok(Self::NamedObj(o));
        });
        ignore_invalid_type_error!(StatementOpcode::try_parse(stream, current_scope), |o| {
            return Ok(Self::StatementOpcode(o));
        });
        ignore_invalid_type_error!(ExpressionOpcode::try_parse(stream, current_scope), |o| {
            return Ok(Self::ExpressionOpcode(o));
        });
        Err(AmlError::InvalidType)
    }
}

#[derive(Debug)]
pub enum TermArg {
    ExpressionOpcode(Box<ExpressionOpcode>),
    DataObject(DataObject),
    ArgObj(u8),
    LocalObj(u8),
}

impl TermArg {
    pub fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        let op = stream.peek_byte()?;
        if opcode::LOCAL0_OP <= op && op <= opcode::LOCAL7_OP {
            stream.seek(1)?;
            return Ok(Self::LocalObj(op - opcode::LOCAL0_OP));
        } else if opcode::ARG0_OP <= op && op <= opcode::ARG6_OP {
            stream.seek(1)?;
            return Ok(Self::ArgObj(op - opcode::ARG0_OP));
        }
        ignore_invalid_type_error!(DataObject::try_parse(stream, current_scope), |d| {
            return Ok(Self::DataObject(d));
        });
        ignore_invalid_type_error!(ExpressionOpcode::try_parse(stream, current_scope), |o| {
            return Ok(Self::ExpressionOpcode(Box::new(o)));
        });
        return Err(AmlError::InvalidType);
    }

    pub fn parse_integer(
        stream: &mut AmlStream,
        current_scope: &NameString,
    ) -> Result<Self, AmlError> {
        let backup = stream.clone();
        let arg = Self::parse(stream, current_scope)?;
        if matches!(arg, Self::LocalObj(_))
            || matches!(arg, Self::ArgObj(_))
            || matches!(
                arg,
                Self::DataObject(DataObject::ComputationalData(ComputationalData::ConstData(
                    _
                )))
            )
            || matches!(
                arg,
                Self::DataObject(DataObject::ComputationalData(ComputationalData::ConstObj(
                    _
                )))
            )
            || matches!(
                arg,
                Self::DataObject(DataObject::ComputationalData(ComputationalData::Revision))
            )
            || matches!(arg, Self::DataObject(DataObject::DefVarPackage(_)))
            || matches!(arg, Self::DataObject(DataObject::DefPackage(_)))
            || matches!(arg, Self::ExpressionOpcode(_))
        {
            Ok(arg)
        } else {
            stream.roll_back(&backup);
            Err(AmlError::InvalidType)
        }
    }
}

#[derive(Debug)]
pub struct TermArgList {
    list: Vec<TermArg>,
}

impl TermArgList {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        let mut term_arg_list = Self { list: Vec::new() };
        while !stream.is_end_of_stream() {
            term_arg_list
                .list
                .push(TermArg::parse(stream, current_scope)?)
        }
        Ok(term_arg_list)
    }
}

#[derive(Debug)]
pub struct MethodInvocation {
    name: NameString,
    term_arg_list: TermArgList,
}

impl MethodInvocation {
    pub fn try_parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        let backup = stream.clone();
        match NameString::parse(stream, Some(current_scope)) {
            Ok(name) => {
                let term_arg_list = TermArgList::parse(stream, current_scope)?;
                Ok(Self {
                    name,
                    term_arg_list,
                })
            }
            Err(_) => {
                stream.roll_back(&backup);
                Err(AmlError::InvalidType)
            }
        }
    }
}
