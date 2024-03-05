//!
//! ACPI Machine Language Term Objects
//!
#![allow(dead_code)]

use super::data_object::{
    try_parse_argument_object, try_parse_local_object, ComputationalData, DataObject,
};
use super::expression_opcode::ExpressionOpcode;
use super::name_object::NameString;
use super::named_object::NamedObject;
use super::namespace_modifier_object::NamespaceModifierObject;
use super::statement_opcode::StatementOpcode;
use super::{AcpiInt, AmlError, AmlStream, Evaluator};

use crate::ignore_invalid_type_error;

use alloc::boxed::Box;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub struct TermList {
    stream: AmlStream,
    current_scope: NameString,
}

impl TermList {
    pub fn new(stream: AmlStream, current_scope: NameString) -> Self {
        Self {
            stream,
            current_scope,
        }
    }

    pub fn is_end_of_stream(&self) -> bool {
        self.stream.is_end_of_stream()
    }

    pub fn get_scope_name(&self) -> &NameString {
        &self.current_scope
    }

    pub fn next(&mut self, evaluator: &mut Evaluator) -> Result<Option<TermObj>, AmlError> {
        if self.is_end_of_stream() {
            Ok(None)
        } else {
            TermObj::parse(&mut self.stream, &self.current_scope, evaluator).map(Some)
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
        evaluator: &mut Evaluator,
    ) -> Result<Self, AmlError> {
        ignore_invalid_type_error!(
            NamespaceModifierObject::try_parse(stream, current_scope),
            |o| { Ok(Self::NamespaceModifierObj(o)) }
        );
        ignore_invalid_type_error!(
            NamedObject::try_parse(stream, current_scope, evaluator),
            |o: NamedObject| { Ok(Self::NamedObj(o)) }
        );
        ignore_invalid_type_error!(
            StatementOpcode::try_parse(stream, current_scope, evaluator),
            |o| { Ok(Self::StatementOpcode(o)) }
        );
        ignore_invalid_type_error!(
            ExpressionOpcode::try_parse(stream, current_scope, evaluator),
            |o| { Ok(Self::ExpressionOpcode(o)) }
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
        evaluator: &mut Evaluator,
    ) -> Result<Self, AmlError> {
        /* println!("TermArg: {:#X}", stream.peek_byte()?); */
        ignore_invalid_type_error!(try_parse_local_object(stream), |n| {
            Ok(Self::LocalObj(n))
        });
        ignore_invalid_type_error!(try_parse_argument_object(stream), |n| {
            Ok(Self::ArgObj(n))
        });
        ignore_invalid_type_error!(DataObject::try_parse(stream, current_scope), |d| {
            Ok(Self::DataObject(d))
        });
        ignore_invalid_type_error!(
            ExpressionOpcode::try_parse(stream, current_scope, evaluator),
            |o| { Ok(Self::ExpressionOpcode(Box::new(o))) }
        );
        Err(AmlError::InvalidType)
    }

    pub fn parse_integer(
        stream: &mut AmlStream,
        current_scope: &NameString,
        evaluator: &mut Evaluator,
    ) -> Result<Self, AmlError> {
        let backup = stream.clone();
        let arg = Self::try_parse(stream, current_scope, evaluator)?;
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
                Self::DataObject(DataObject::ComputationalData(ComputationalData::DefBuffer(
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
            pr_warn!("Unmatched TermArg:{:?}", arg);
            stream.roll_back(&backup);
            Err(AmlError::InvalidType)
        }
    }
}

#[derive(Debug, Clone)]
pub struct TermArgList {
    pub list: Vec<TermArg>,
}

impl TermArgList {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        argument_count: AcpiInt,
        evaluator: &mut Evaluator,
    ) -> Result<Self, AmlError> {
        let mut term_arg_list = Self {
            list: Vec::with_capacity(argument_count),
        };
        for _ in 0..argument_count {
            term_arg_list
                .list
                .push(TermArg::try_parse(stream, current_scope, evaluator)?);
        }
        Ok(term_arg_list)
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
        evaluator: &mut Evaluator,
    ) -> Result<Self, AmlError> {
        let backup = stream.clone();
        match NameString::parse(stream, Some(current_scope)) {
            Ok(name) => {
                let arg_cnt = evaluator.find_method_argument_count(&name)?;
                let term_arg_list = TermArgList::parse(stream, current_scope, arg_cnt, evaluator)?;
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

    pub fn get_name(&self) -> &NameString {
        &self.name
    }

    pub fn get_ter_arg_list(&self) -> &TermArgList {
        &self.term_arg_list
    }
}
