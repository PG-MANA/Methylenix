//!
//! ACPI Machine Language
//!
//! This is the parser for AML.

pub(super) mod aml_variable;
mod data_object;
pub(super) mod evaluator;
mod expression_opcode;
mod name_object;
pub(super) mod named_object;
mod namespace_modifier_object;
mod opcode;
mod parser;
mod statement_opcode;
mod term_object;

pub use self::aml_variable::AmlPciConfig;
use self::aml_variable::AmlVariable;
pub use self::data_object::{eisa_id_to_dword, ConstData, DataRefObject};
use self::evaluator::Evaluator;
pub use self::name_object::NameString;
use self::named_object::{Device, NamedObject};
use self::parser::{ContentObject, ParseHelper};
use self::term_object::TermList;

use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

type AcpiInt = usize;

#[derive(Clone)]
pub struct AmlInterpreter {
    parse_helper: ParseHelper,
    evaluator: Evaluator,
}

#[derive(Clone)]
pub struct AmlStream {
    pointer: VAddress,
    limit: VAddress,
}

#[derive(Debug)]
pub enum AmlError {
    AccessOutOfRange,
    InvalidSizeChange,
    InvalidType,
    InvalidMethodName(NameString),
    InvalidScope(NameString),
    InvalidOperation,
    MutexError,
    ObjectTreeError,
    NestedSearch,
    UnsupportedType,
}

pub struct IntIter {
    stream: AmlStream,
    remaining_elements: usize,
}

impl IntIter {
    pub fn new(stream: AmlStream, num_of_elements: usize) -> Self {
        Self {
            stream,
            remaining_elements: num_of_elements,
        }
    }

    fn get_next(&mut self) -> Result<Option<AcpiInt>, AmlError> {
        if self.remaining_elements == 0 {
            Ok(None)
        } else {
            let d = DataRefObject::parse(&mut self.stream, &NameString::current())?;
            self.remaining_elements -= 1;
            Ok(d.get_const_data())
        }
    }
}

impl Iterator for IntIter {
    type Item = AcpiInt;
    fn next(&mut self) -> Option<Self::Item> {
        match self.get_next() {
            Ok(o) => o,
            Err(e) => {
                pr_err!("{:?}", e);
                None
            }
        }
    }
}

#[macro_export]
macro_rules! ignore_invalid_type_error {
    ($f:expr, $ok_stmt:expr) => {
        match $f {
            Ok(t) => return $ok_stmt(t),
            Err(AmlError::InvalidType) => { /* Ignore */ }
            Err(e) => return Err(e),
        };
    };
}

impl AmlInterpreter {
    pub fn setup(
        dsdt_term_list_address: (VAddress, MSize),
        ssdt_term_list_address_list: &[(VAddress, MSize)],
    ) -> Option<Self> {
        use alloc::vec::Vec;

        let dsdt = TermList::new(
            AmlStream::new(dsdt_term_list_address.0, dsdt_term_list_address.1),
            NameString::root(),
        );
        let mut ssdt_list = Vec::with_capacity(ssdt_term_list_address_list.len());
        for s in ssdt_term_list_address_list {
            ssdt_list.push(TermList::new(AmlStream::new(s.0, s.1), NameString::root()));
        }
        let mut parse_helper = ParseHelper::new(dsdt, ssdt_list, &NameString::root());
        if let Err(e) = parse_helper.init() {
            pr_err!("Cannot initialize ParseHelper: {:?}", e);
            return None;
        }
        let evaluator = Evaluator::new(parse_helper.clone());
        Some(Self {
            parse_helper,
            evaluator,
        })
    }

    /* DataRefObject => AmlVariable */
    pub fn get_data_object(&mut self, name: &NameString) -> Option<DataRefObject> {
        match self.parse_helper.search_object(name) {
            Ok(Some(d)) => match d {
                ContentObject::NamedObject(n) => {
                    pr_err!("Expected DataRefObject, but found {:?}", n);
                    None
                }
                ContentObject::DataRefObject(d) => Some(d),
                ContentObject::Scope(s) => {
                    pr_err!("Expected DataRefObject, but found Scope({})", s);
                    None
                }
            },
            Ok(None) => None,
            Err(e) => {
                pr_err!("Cannot parse AML: {:?}", e);
                None
            }
        }
    }

    pub fn move_into_device(&mut self, hid: &[u8; 7]) -> Result<Option<Device>, ()> {
        match self.parse_helper.move_into_device(hid) {
            Ok(d) => Ok(d),
            Err(e) => {
                pr_err!("Parsing AML was failed: {:?}", e);
                Err(())
            }
        }
    }

    pub fn evaluate_method(
        &mut self,
        method_name: &NameString,
        arguments: &[AmlVariable],
    ) -> Result<Option<AmlVariable>, ()> {
        if method_name.is_null_name() {
            pr_warn!("method_name is NullName.");
            return Ok(None);
        }
        match self.parse_helper.move_into_object(method_name, None, None) {
            Ok(m) => match m {
                ContentObject::NamedObject(n) => match n {
                    NamedObject::DefMethod(method) => {
                        self.evaluator.set_parse_helper(self.parse_helper.clone());
                        match self.evaluator.eval_method(&method, arguments) {
                            Ok(v) => Ok(Some(v)),
                            Err(e) => {
                                pr_err!("AML Evaluator Error: {:?}", e);
                                Err(())
                            }
                        }
                    }
                    NamedObject::DefExternal(_) => {
                        unimplemented!()
                    }
                    _ => {
                        pr_err!("Expected a method, but found {:?}", n);
                        Err(())
                    }
                },
                ContentObject::DataRefObject(_) => {
                    unimplemented!()
                }
                ContentObject::Scope(s) => {
                    pr_err!("Unexpected Scope({})", s);
                    Err(())
                }
            },
            Err(e) => {
                pr_err!("Parsing AML was failed: {:?}", e);
                Err(())
            }
        }
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
            Err(AmlError::AccessOutOfRange)
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
        self.check_pointer(0)?;
        Ok(unsafe { *(self.pointer.to_usize() as *const u8) })
    }

    fn peek_byte_with_pos(&self, pos_forward_from_current: usize) -> Result<u8, AmlError> {
        self.check_pointer(pos_forward_from_current)?;
        Ok(unsafe {
            *((self.pointer + MSize::new(pos_forward_from_current)).to_usize() as *const u8)
        })
    }

    fn seek(&mut self, bytes_to_forward: usize) -> Result<(), AmlError> {
        self.pointer += MSize::new(bytes_to_forward);
        if self.pointer > self.limit {
            Err(AmlError::AccessOutOfRange)
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
