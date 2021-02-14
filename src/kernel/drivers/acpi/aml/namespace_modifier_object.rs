//!
//! ACPI Machine Language  Namespace Modifier Objects
//!
#![allow(dead_code)]
use super::data_object::{DataRefObject, NameString, PkgLength};
use super::opcode;
use super::parser::ParseHelper;
use super::term_object::TermList;
use super::{AmlError, AmlStream};

#[derive(Debug)]
pub struct Alias {
    pub name: NameString,
    pub destination: NameString,
}

impl Alias {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* AliasOp was read */
        let name = NameString::parse(stream, Some(current_scope))?;
        let destination = NameString::parse(stream, Some(current_scope))?;
        Ok(Self { name, destination })
    }
}

#[derive(Debug)]
pub struct Name {
    pub name: NameString,
    pub data_ref_object: DataRefObject,
}

impl Name {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* NameOp was read */
        let name = NameString::parse(stream, Some(current_scope))?;
        let data_ref_object = DataRefObject::parse(stream, current_scope)?;
        Ok(Self {
            name,
            data_ref_object,
        })
    }
}

#[derive(Debug)]
pub struct Scope {
    pub name: NameString,
    pub term_list: TermList,
}

impl Scope {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &ParseHelper,
    ) -> Result<Self, AmlError> {
        /* ScopeOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut scope_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        drop(stream); /* Avoid using this */
        scope_stream.change_size(pkg_length.actual_length)?;
        let name = NameString::parse(&mut scope_stream, Some(current_scope))?;
        Ok(Self {
            name: name.clone(),
            term_list: TermList::new(scope_stream, name, parse_helper)?,
        })
    }
}

#[derive(Debug)]
pub enum NamespaceModifierObject {
    DefAlias(Alias),
    DefName(Name),
    DefScope(Scope),
}

impl NamespaceModifierObject {
    pub fn try_parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        parse_helper: &mut ParseHelper,
    ) -> Result<Self, AmlError> {
        let op = stream.peek_byte()?;
        match op {
            opcode::ALIAS_OP => {
                stream.seek(1)?;
                let alias = Alias::parse(stream, current_scope)?;
                parse_helper.add_alias_name(&alias.name, &alias.destination)?;
                Ok(Self::DefAlias(alias))
            }
            opcode::NAME_OP => {
                stream.seek(1)?;
                let name = Name::parse(stream, current_scope)?;
                parse_helper.add_def_name(&name.name, &name.data_ref_object)?;
                Ok(Self::DefName(name))
            }
            opcode::SCOPE_OP => {
                stream.seek(1)?;
                Ok(Self::DefScope(Scope::parse(
                    stream,
                    current_scope,
                    parse_helper,
                )?))
            }
            _ => Err(AmlError::InvalidType),
        }
    }
}
