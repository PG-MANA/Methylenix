//!
//! ACPI Machine Language Statement Opcodes
//!
//!
#![allow(dead_code)]
use super::data_object::{NameString, PkgLength, SuperName};
use super::opcode;
use super::parser::ParseHelper;
use super::term_object::{TermArg, TermList};
use super::{AmlError, AmlStream};

#[derive(Debug)]
pub struct Fatal {
    fatal_type: u8,
    fatal_code: u32,
    fatal_arg: TermArg,
}

impl Fatal {
    pub fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* FatalOp was read */
        let fatal_type = stream.read_byte()?;
        let fatal_code = stream.read_dword()?;
        let fatal_arg = TermArg::parse_integer(stream, current_scope, parse_helper)?;
        Ok(Self {
            fatal_type,
            fatal_code,
            fatal_arg,
        })
    }
}

#[derive(Debug)]
pub struct IfElse {
    predicate: TermArg,
    term_list: TermList,
    else_term_list: Option<TermList>,
}

impl IfElse {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* IfScope was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut if_scope_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        if_scope_stream.change_size(pkg_length.actual_length)?;
        let predicate = TermArg::parse_integer(&mut if_scope_stream, current_scope, parse_helper)?;
        let term_list = TermList::new(if_scope_stream, current_scope.clone(), parse_helper)?;
        let op = stream.peek_byte()?;
        if op != opcode::ELSE_OP {
            Ok(Self {
                predicate,
                term_list,
                else_term_list: None,
            })
        } else {
            stream.seek(1)?;
            let pkg_length = PkgLength::parse(stream)?;
            let mut else_scope_stream = stream.clone();
            stream.seek(pkg_length.actual_length)?;
            drop(stream); /* Avoid using this */
            else_scope_stream.change_size(pkg_length.actual_length)?;
            let else_term_list =
                TermList::new(else_scope_stream, current_scope.clone(), parse_helper)?;
            Ok(Self {
                predicate,
                term_list,
                else_term_list: Some(else_term_list),
            })
        }
    }
}

#[derive(Debug)]
pub struct Notify {
    notify_object: SuperName,
    notify_value: TermArg,
}

impl Notify {
    pub fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* NotifyOp was read */
        let notify_object = SuperName::try_parse(stream, current_scope, parse_helper)?;
        let notify_value = TermArg::parse_integer(stream, current_scope, parse_helper)?;
        Ok(Self {
            notify_object,
            notify_value,
        })
    }
}

#[derive(Debug)]
pub struct While {
    predicate: TermArg,
    term_list: TermList,
}

impl While {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        /* WhileOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut while_scope_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        drop(stream); /* Avoid using this */
        while_scope_stream.change_size(pkg_length.actual_length)?;
        let predicate =
            TermArg::parse_integer(&mut while_scope_stream, current_scope, parse_helper)?;
        let term_list = TermList::new(while_scope_stream, current_scope.clone(), parse_helper)?;
        Ok(Self {
            predicate,
            term_list,
        })
    }
}

#[derive(Debug)]
pub enum StatementOpcode {
    DefBreak,
    DefBreakPoint,
    DefContinue,
    DefFatal(Fatal),
    DefIfElse(IfElse),
    DefNoop,
    DefNotify(Notify),
    DefRelease(SuperName),
    DefReset(SuperName),
    DefReturn(TermArg),
    DefSignal(SuperName),
    DefSleep(TermArg),
    DefStall(TermArg),
    DefWhile(While),
}

impl StatementOpcode {
    pub fn try_parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        let back_up = stream.clone();
        match stream.read_byte()? {
            opcode::EXT_OP_PREFIX => match stream.read_byte()? {
                opcode::SIGNAL_OP => Ok(Self::DefSignal(SuperName::try_parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?)),
                opcode::RESET_OP => Ok(Self::DefReset(SuperName::try_parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?)),
                opcode::RELEASE_OP => Ok(Self::DefRelease(SuperName::try_parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?)),
                opcode::FATAL_OP => Ok(Self::DefFatal(Fatal::parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?)),
                opcode::SLEEP_OP => Ok(Self::DefSleep(TermArg::parse_integer(
                    stream,
                    current_scope,
                    parse_helper,
                )?)),
                opcode::STALL_OP => Ok(Self::DefStall(TermArg::parse_integer(
                    stream,
                    current_scope,
                    parse_helper,
                )?)),
                _ => {
                    stream.roll_back(&back_up);
                    Err(AmlError::InvalidType)
                }
            },
            opcode::BREAK_OP => Ok(Self::DefBreak),
            opcode::BREAK_POINT_OP => Ok(Self::DefBreakPoint),
            opcode::CONTINUE_OP => Ok(Self::DefContinue),
            opcode::IF_OP => Ok(Self::DefIfElse(IfElse::parse(
                stream,
                current_scope,
                parse_helper,
            )?)),
            opcode::NOOP_OP => Ok(Self::DefNoop),
            opcode::NOTIFY_OP => Ok(Self::DefNotify(Notify::parse(
                stream,
                current_scope,
                parse_helper,
            )?)),
            opcode::RETURN_OP => Ok(Self::DefReturn(TermArg::try_parse(
                stream,
                current_scope,
                parse_helper,
            )?)),
            opcode::WHILE_OP => Ok(Self::DefWhile(While::parse(
                stream,
                current_scope,
                parse_helper,
            )?)),
            _ => {
                stream.roll_back(&back_up);
                Err(AmlError::InvalidType)
            }
        }
    }
}
