//!
//! ACPI Machine Language
//!
//! This is the parser for AML.

use alloc::vec::Vec;

use crate::arch::target_arch::device::acpi::osi;
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};

pub use self::aml_variable::AmlVariable;
pub use self::data_object::{eisa_id_to_dword, ConstData, DataRefObject};
use self::evaluator::Evaluator;
pub use self::name_object::NameString;
use self::term_object::TermList;

pub(super) mod aml_variable;
mod data_object;
pub(super) mod evaluator;
mod expression_opcode;
mod name_object;
pub(super) mod named_object;
mod namespace_modifier_object;
pub(super) mod notify;
mod opcode;
mod statement_opcode;
mod term_object;
mod variable_tree;

type AcpiInt = usize;

const ACPI_INT_ONES: AcpiInt = opcode::ONES_OP as _;

#[derive(Clone)]
pub struct AmlInterpreter {
    evaluator: Evaluator,
}

#[derive(Clone, PartialEq)]
pub struct AmlStream {
    pointer: VAddress,
    limit: VAddress,
}

#[derive(Debug)]
pub enum AmlError {
    AccessOutOfRange,
    InvalidType,
    InvalidName(NameString),
    InvalidOperation,
    MutexError,
    ObjectTreeError,
    NestedSearch,
    UnsupportedType,
}

#[derive(Clone, Debug)]
pub enum ResourceData {
    Irq(u8),
    Interrupt(usize),
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
        let dsdt = TermList::new(
            AmlStream::new(dsdt_term_list_address.0, dsdt_term_list_address.1),
            NameString::root(),
        );
        let mut root_term_list = Vec::with_capacity(ssdt_term_list_address_list.len() + 1);
        root_term_list.push(dsdt.clone());
        for s in ssdt_term_list_address_list {
            root_term_list.push(TermList::new(AmlStream::new(s.0, s.1), NameString::root()));
        }

        let mut evaluator = Evaluator::new(dsdt, root_term_list);
        if let Err(e) = evaluator.init(osi) {
            pr_err!("Failed to initialize Evaluator: {:?}", e);
            None
        } else {
            Some(Self { evaluator })
        }
    }

    pub fn initialize_all_devices(&mut self) -> Result<(), ()> {
        if let Err(e) = self.evaluator.initialize_all_devices() {
            pr_err!("Failed to Evaluate _INI/_STA: {:?}", e);
            Err(())
        } else {
            Ok(())
        }
    }

    pub fn get_aml_variable(&mut self, name: &NameString) -> Option<AmlVariable> {
        let mut evaluator = self.evaluator.clone();

        match evaluator.search_aml_variable(name, None, false) {
            Ok(v) => {
                let cloned_v = v.lock().unwrap().clone();
                drop(v);
                if let AmlVariable::Method(m) = cloned_v {
                    evaluator = self.evaluator.clone();
                    match evaluator.eval_method(&m, &[], None) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            pr_err!("Evaluating {} was failed: {:?}", m.get_name(), e);
                            None
                        }
                    }
                } else if cloned_v.is_constant_data() {
                    Some(cloned_v)
                } else {
                    match cloned_v.get_constant_data() {
                        Ok(constant_data) => Some(constant_data),
                        Err(e) => {
                            pr_err!("Failed to get the constant data({}): {:?}", name, e);
                            None
                        }
                    }
                }
            }
            Err(e) => {
                pr_err!("Failed to parse AML: {:?}", e);
                None
            }
        }
    }

    pub fn move_into_device(&self, hid: &[u8; 7]) -> Result<Option<Self>, ()> {
        let mut new_interpreter = self.clone();
        match new_interpreter.evaluator.move_into_device(hid) {
            Ok(true) => Ok(Some(new_interpreter)),
            Ok(false) => Ok(None),
            Err(e) => {
                pr_err!("Parsing AML was failed: {:?}", e);
                Err(())
            }
        }
    }

    pub fn get_current_scope(&self) -> &NameString {
        self.evaluator.get_current_scope()
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

        let method = match self.evaluator.search_aml_variable(method_name, None, false) {
            Ok(v) => {
                if let AmlVariable::Method(m) = &*v.lock().unwrap() {
                    m.clone()
                } else {
                    pr_err!("Expected a method, but found {:?}", &*v.lock().unwrap());
                    return Err(());
                }
            }
            Err(AmlError::InvalidName(n)) => {
                return if &n == method_name {
                    pr_warn!("{} is not found.", method_name);
                    Ok(None)
                } else {
                    pr_err!("{} is not found.", n);
                    Err(())
                };
            }
            Err(e) => {
                pr_err!("Parsing AML was failed: {:?}", e);
                return Err(());
            }
        };
        match self.evaluator.eval_method(&method, arguments, None) {
            Ok(v) => match v {
                AmlVariable::Uninitialized => Ok(None),
                _ => Ok(Some(v)),
            },
            Err(e) => {
                pr_err!("Failed to evaluate AML: {:?}", e);
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
        let d = unsafe { *(self.pointer.to_usize() as *const T) };
        self.pointer += MSize::new(core::mem::size_of::<T>());
        Ok(d)
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
            Err(AmlError::AccessOutOfRange)
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
