//!
//! ACPI Machine Language Term Objects
//!
#![allow(dead_code)]
use super::data_object::{
    try_parse_argument_object, try_parse_local_object, ComputationalData, DataObject, NameString,
};
use super::expression_opcode::ExpressionOpcode;
use super::named_object::NamedObject;
use super::namespace_modifier_object::NamespaceModifierObject;
use super::parser::ParseHelper;
use super::statement_opcode::StatementOpcode;
use super::{AcpiInt, AmlError, AmlStream};

use crate::ignore_invalid_type_error;

use alloc::boxed::Box;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct TermList {
    stream: AmlStream,
    current_scope: NameString,
    parse_helper: ParseHelper, /* should not hold this */
}

impl TermList {
    pub fn new(
        stream: AmlStream,
        current_scope: NameString,
        parse_helper: &ParseHelper,
    ) -> Result<Self, AmlError> {
        let mut parse_helper = parse_helper.clone();
        parse_helper.move_into_scope(&current_scope)?;
        Ok(Self {
            stream,
            current_scope,
            parse_helper,
        })
    }

    pub fn is_end_of_stream(&self) -> bool {
        self.stream.is_end_of_stream()
    }

    pub fn get_scope_name(&self) -> &NameString {
        &self.current_scope
    }

impl Iterator for TermList {
    type Item = Result<TermObj, AmlError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.stream.is_end_of_stream() {
            None
        } else {
            match TermObj::parse(
                &mut self.stream,
                &self.current_scope,
                &mut self.parse_helper,
            ) {
                Ok(o) => Some(Ok(o)),
                Err(AmlError::InvalidType) => {
                    if self.stream.is_end_of_stream() {
                        None
                    } else {
                        pr_err!("Stream does not end: {:?}", self.stream);
                        Some(Err(AmlError::InvalidType))
                    }
                }
                Err(e) => {
                    pr_err!("Parsing TermObj was failed: {:?}", e);
                    Some(Err(e))
                }
            }
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
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* println!("TermObj:{:#X}(Stream:{:?}", stream.peek_byte()?, stream); */
        ignore_invalid_type_error!(
            NamespaceModifierObject::try_parse(stream, current_scope, parse_helper),
            |o| {
                return Ok(Self::NamespaceModifierObj(o));
            }
        );
        ignore_invalid_type_error!(
            NamedObject::try_parse(stream, current_scope, parse_helper),
            |o: NamedObject| {
                if let Some(name) = o.get_name() {
                    parse_helper.add_named_object(name, &o)?;
                } else {
                    parse_helper.add_named_object(current_scope, &o)?;
                }
                return Ok(Self::NamedObj(o));
            }
        );
        ignore_invalid_type_error!(
            StatementOpcode::try_parse(stream, current_scope, parse_helper),
            |o| {
                return Ok(Self::StatementOpcode(o));
            }
        );
        ignore_invalid_type_error!(
            ExpressionOpcode::try_parse(stream, current_scope, parse_helper),
            |o| {
                return Ok(Self::ExpressionOpcode(o));
            }
        );
        Err(AmlError::InvalidType)
    }
}

#[derive(Debug, Clone)]
pub enum TermArg {
    ExpressionOpcode(Box<ExpressionOpcode>),
    DataObject(DataObject),
    ArgObj(u8),
    LocalObj(u8),
}

impl TermArg {
    pub fn try_parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* println!("TermArg: {:#X}", stream.peek_byte()?); */
        ignore_invalid_type_error!(try_parse_local_object(stream), |n| {
            return Ok(Self::LocalObj(n));
        });
        ignore_invalid_type_error!(try_parse_argument_object(stream), |n| {
            return Ok(Self::ArgObj(n));
        });
        ignore_invalid_type_error!(DataObject::try_parse(stream, current_scope), |d| {
            return Ok(Self::DataObject(d));
        });
        ignore_invalid_type_error!(
            ExpressionOpcode::try_parse(stream, current_scope, parse_helper),
            |o| {
                return Ok(Self::ExpressionOpcode(Box::new(o)));
            }
        );
        return Err(AmlError::InvalidType);
    }

    pub fn parse_integer(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        let backup = stream.clone();
        let arg = Self::try_parse(stream, current_scope, parse_helper)?;
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

#[derive(Debug, Clone)]
pub struct TermArgList {
    list: Vec<TermArg>,
}

impl TermArgList {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        argument_count: AcpiInt,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        let mut term_arg_list = Self {
            list: Vec::with_capacity(argument_count as usize),
        };
        for _ in 0..argument_count {
            match TermArg::try_parse(stream, current_scope, parse_helper) {
                Ok(o) => term_arg_list.list.push(o),
                Err(AmlError::InvalidType) => return Ok(term_arg_list),
                Err(e) => return Err(e),
            }
        }
        return Ok(term_arg_list);
    }
}

#[derive(Debug, Clone)]
pub struct MethodInvocation {
    name: NameString,
    term_arg_list: TermArgList,
}

impl MethodInvocation {
    pub fn try_parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        let backup = stream.clone();
        match NameString::parse(stream, Some(current_scope)) {
            Ok(name) => {
                let arg_cnt = parse_helper
                    .find_method_argument_count(&name)?
                    .ok_or_else(|| AmlError::InvalidMethodName(name.clone()))?;
                let term_arg_list =
                    TermArgList::parse(stream, current_scope, arg_cnt, parse_helper)?;
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
