//!
//! ACPI Machine Language
//!
//! This is the parser for AML.

mod data_object;
mod expression_opcode;
mod named_object;
mod namespace_modifier_object;
mod opcode;
mod parser;
mod statement_opcode;
mod term_object;

use self::data_object::NameString;
use self::namespace_modifier_object::NamespaceModifierObject;
use self::parser::ParseHelper;
use self::term_object::{TermList, TermObj};

use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

type AcpiInt = usize;
type AcpiData = u64;

pub struct AmlParser {
    base_address: VAddress,
    size: MSize,
}

#[derive(Clone)]
pub struct AmlStream {
    pointer: VAddress,
    limit: VAddress,
}

#[derive(Debug)]
pub enum AmlError {
    ReadOutOfRange,
    SeekOutOfRange,
    PeekOutOfRange,
    InvalidSizeChange,
    InvalidData,
    InvalidType,
    InvalidMethodName(NameString),
    InvalidScope(NameString),
    MutexError,
    UnsupportedType,
}

#[macro_export]
macro_rules! ignore_invalid_type_error {
    ($f:expr, $ok_stmt:expr) => {
        match $f {
            Ok(t) => return $ok_stmt(t),
            Err(AmlError::InvalidType) => {}
            Err(e) => return Err(e),
        };
    };
}

impl AmlParser {
    pub const fn new(address: VAddress, size: MSize) -> Self {
        /* memory area must be accessible. */
        Self {
            base_address: address,
            size,
        }
    }

    pub fn debug(&mut self) {
        println!("AML Size: {:#X}", self.size.to_usize());
        let root_name = NameString::root();
        let mut parse_helper =
            ParseHelper::new(AmlStream::new(self.base_address, self.size), &root_name);
        let root_term_list = TermList::new(
            AmlStream::new(self.base_address, self.size),
            root_name,
            &mut parse_helper,
        )
        .unwrap();
        match Self::debug_term_list(root_term_list) {
            Ok(_) => {
                println!("AML End");
            }
            Err(e) => {
                println!("ParseError: {:?}", e);
            }
        }
    }

    fn debug_term_list(term_list: TermList) -> Result<(), AmlError> {
        for o in term_list {
            match o? {
                TermObj::NamespaceModifierObj(n) => match n {
                    NamespaceModifierObject::DefAlias(d_a) => {
                        println!("DefAlias({} => {})", d_a.name, d_a.destination);
                    }
                    NamespaceModifierObject::DefName(d_n) => {
                        println!("DefName({}) => {:?}", d_n.name, d_n.data_ref_object);
                    }
                    NamespaceModifierObject::DefScope(d_s) => {
                        println!("DefScope({}) => {{", d_s.name);
                        Self::debug_term_list(d_s.term_list.clone())?;
                        println!("}}");
                    }
                },
                d => {
                    println!("{:?}", d);
                }
            }
        }
        Ok(())
    }
}

impl AmlStream {
    pub const fn new(address: VAddress, size: MSize) -> Self {
        Self {
            pointer: address,
            limit: address + size,
        }
    }

    fn check_pointer(&self, read_size: usize) -> Result<(), AmlError> {
        if self.pointer + MSize::new(read_size) > self.limit {
            println!(
                "AmlError: ({},{})",
                (self.pointer + MSize::new(read_size) - self.limit).to_usize(),
                read_size
            );
            Err(AmlError::ReadOutOfRange)
        } else {
            Ok(())
        }
    }

    pub fn is_end_of_stream(&self) -> bool {
        self.pointer == self.limit
    }

    fn get_available_size(&self) -> usize {
        (self.limit - self.pointer).to_usize()
    }

    fn read<T: ?Sized + Copy>(&mut self) -> Result<T, AmlError> {
        self.check_pointer(core::mem::size_of::<T>())?;
        let d = unsafe { *((self.pointer).to_usize() as *const T) };
        self.pointer += MSize::new(core::mem::size_of::<T>());
        return Ok(d);
    }

    fn read_byte(&mut self) -> Result<u8, AmlError> {
        self.read::<u8>()
    }

    fn read_word(&mut self) -> Result<u16, AmlError> {
        Ok(u16::from_le(self.read::<_>()?))
    }

    fn read_dword(&mut self) -> Result<u32, AmlError> {
        Ok(u32::from_le(self.read::<_>()?))
    }

    fn read_qword(&mut self) -> Result<u64, AmlError> {
        Ok(u64::from_le(self.read::<_>()?))
    }

    fn peek_byte(&self) -> Result<u8, AmlError> {
        self.check_pointer(1).or(Err(AmlError::PeekOutOfRange))?;
        Ok(unsafe { *(self.pointer.to_usize() as *const u8) })
    }

    fn peek_byte_with_pos(&self, pos_forward_from_current: usize) -> Result<u8, AmlError> {
        self.check_pointer(pos_forward_from_current)
            .or(Err(AmlError::PeekOutOfRange))?;
        Ok(unsafe {
            *((self.pointer + MSize::new(pos_forward_from_current)).to_usize() as *const u8)
        })
    }

    fn seek(&mut self, bytes_to_forward: usize) -> Result<(), AmlError> {
        self.pointer += MSize::new(bytes_to_forward);
        if self.pointer > self.limit {
            Err(AmlError::SeekOutOfRange)
        } else {
            Ok(())
        }
    }

    fn change_size(&mut self, new_size_from_current_point: usize) -> Result<(), AmlError> {
        let new_limit = self.pointer + MSize::new(new_size_from_current_point);
        if new_limit > self.limit {
            Err(AmlError::InvalidSizeChange)
        } else {
            self.limit = new_limit;
            Ok(())
        }
    }

    fn get_pointer(&self) -> VAddress {
        self.pointer
    }

    fn roll_back(&mut self, backup: &Self) {
        self.pointer = backup.pointer;
        self.limit = backup.limit;
    }
}

impl core::fmt::Debug for AmlStream {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "AmlStream(Base: {:#X}, Size: {:#X})",
            self.get_pointer().to_usize(),
            self.get_available_size()
        ))
    }
}
