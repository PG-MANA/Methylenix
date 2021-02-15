//!
//! ACPI Machine Language  Namespace Modifier Objects
//!
#![allow(dead_code)]
use super::data_object::{DataRefObject, NameString, PkgLength};
use super::opcode;
use super::term_object::TermList;
use super::{AmlError, AmlStream};

#[derive(Debug)]
pub struct Alias {
    source: NameString,
    alias: NameString,
}

impl Alias {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* AliasOp was read */
        let source = NameString::parse(stream, Some(current_scope))?;
        let alias = NameString::parse(stream, Some(current_scope))?;
        Ok(Self { source, alias })
    }

    pub fn get_name(&self) -> &NameString {
        &self.alias
    }

    pub fn get_source(&self) -> &NameString {
        &self.source
    }
}

#[derive(Debug)]
pub struct Name {
    name: NameString,
    data_ref_object: DataRefObject,
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

    pub fn get_name(&self) -> &NameString {
        &self.name
    }

    pub fn get_data_ref_object(&self) -> &DataRefObject {
        &self.data_ref_object
    }
}

#[derive(Debug)]
pub struct Scope {
    name: NameString,
    term_list: TermList,
}

impl Scope {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* ScopeOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut scope_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        drop(stream); /* Avoid using this */
        scope_stream.change_size(pkg_length.actual_length)?;
        let name = NameString::parse(&mut scope_stream, Some(current_scope))?;
        Ok(Self {
            name: name.clone(),
            term_list: TermList::new(scope_stream, name),
        })
    }

    pub fn get_name(&self) -> &NameString {
        &self.name
    }

    pub fn get_term_list(&self) -> &TermList {
        &self.term_list
    }
}

#[derive(Debug)]
pub enum NamespaceModifierObject {
    DefAlias(Alias),
    DefName(Name),
    DefScope(Scope),
}

impl NamespaceModifierObject {
    pub fn try_parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        let op = stream.peek_byte()?;
        match op {
            opcode::ALIAS_OP => {
                stream.seek(1)?;
                let alias = Alias::parse(stream, current_scope)?;
                Ok(Self::DefAlias(alias))
            }
            opcode::NAME_OP => {
                stream.seek(1)?;
                let name = Name::parse(stream, current_scope)?;
                Ok(Self::DefName(name))
            }
            opcode::SCOPE_OP => {
                stream.seek(1)?;
                Ok(Self::DefScope(Scope::parse(stream, current_scope)?))
            }
            _ => Err(AmlError::InvalidType),
        }
    }

    pub fn get_name(&self) -> &NameString {
        match self {
            Self::DefAlias(a) => a.get_name(),
            Self::DefName(n) => n.get_name(),
            Self::DefScope(s) => s.get_name(),
        }
    }
}
