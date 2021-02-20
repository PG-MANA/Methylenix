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
use self::statement_opcode::StatementOpcode;
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
    AccessOutOfRange,
    InvalidSizeChange,
    InvalidType,
    InvalidMethodName(NameString),
    InvalidScope(NameString),
    MutexError,
    ObjectTreeError,
    NestedSearch,
    UnsupportedType,
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
        let root_term_list = TermList::new(
            AmlStream::new(self.base_address, self.size),
            root_name.clone(),
        );
        let mut parse_helper = ParseHelper::new(root_term_list.clone(), &root_name);
        if let Err(e) = parse_helper.init() {
            println!("Cannot Init ParseHelper:{:?}", e);
            return;
        }
        match Self::debug_term_list(root_term_list, &mut parse_helper) {
            Ok(_) => {
                println!("AML End");
            }
            Err(e) => {
                println!("ParseError: {:?}", e);
            }
        }
    }

    fn debug_term_list(
        mut term_list: TermList,
        parse_helper: &mut ParseHelper,
    ) -> Result<(), AmlError> {
        while let Some(term_obj) = term_list.next(parse_helper)? {
            match term_obj {
                TermObj::NamespaceModifierObj(n) => match n {
                    NamespaceModifierObject::DefAlias(d_a) => {
                        println!("DefAlias({} => {})", d_a.get_name(), d_a.get_source());
                    }
                    NamespaceModifierObject::DefName(d_n) => {
                        println!(
                            "DefName({}) => {:?}",
                            d_n.get_name(),
                            d_n.get_data_ref_object()
                        );
                    }
                    NamespaceModifierObject::DefScope(d_s) => {
                        println!("DefScope({}) => {{", d_s.get_name());
                        parse_helper.move_current_scope(d_s.get_name())?;
                        if let Err(e) =
                            Self::debug_term_list(d_s.get_term_list().clone(), parse_helper)
                        {
                            pr_err!(
                                "Cannot parse {} Error: {:?}. Continue...",
                                d_s.get_name(),
                                e
                            );
                            parse_helper.move_current_scope(term_list.get_scope_name());
                        } else {
                            parse_helper.move_parent_scope()?;
                        }
                        //println!("}}");
                    }
                },
                TermObj::NamedObj(n_o) => {
                    println!("{:?}", n_o);
                    if let Some(mut field_list) = n_o.get_field_list() {
                        println!(
                            "FieldList({}) => {{",
                            n_o.get_name().unwrap_or(term_list.get_scope_name())
                        );
                        while let Some(field_element) = field_list.next()? {
                            println!("{:?}", field_element);
                        }
                        println!("}}");
                    } else if let Some(object_term_list) = n_o.get_term_list() {
                        let name = n_o.get_name().unwrap();
                        println!("TermList({}) => {{", name);
                        parse_helper.move_current_scope(name)?;
                        if let Err(e) = Self::debug_term_list(object_term_list, parse_helper) {
                            pr_err!("Cannot parse {} Error: {:?}. Continue...", name, e);
                            parse_helper.move_current_scope(term_list.get_scope_name());
                        } else {
                            parse_helper.move_parent_scope()?;
                        }
                        println!("}}");
                    }
                }
                TermObj::StatementOpcode(s_o) => {
                    match s_o {
                        StatementOpcode::DefBreak => {
                            println!("break;");
                        }
                        StatementOpcode::DefBreakPoint => {
                            println!("(BreakPoint);");
                        }
                        StatementOpcode::DefContinue => {
                            println!("continue;");
                        }
                        StatementOpcode::DefFatal(f) => {
                            println!("{:?}", f);
                        }
                        StatementOpcode::DefIfElse(i_e) => {
                            println!("if({:?}) {{", i_e.get_predicate());
                            if let Err(e) = Self::debug_term_list(
                                i_e.get_if_true_term_list().clone(),
                                parse_helper,
                            ) {
                                pr_err!(
                                    "Cannot parse if statement of {} Error: {:?}. Continue...",
                                    term_list.get_scope_name(),
                                    e
                                );
                                parse_helper.move_current_scope(term_list.get_scope_name());
                            } else {
                                parse_helper.move_parent_scope()?;
                            }
                            if let Some(else_term_list) = i_e.get_if_false_term_list() {
                                println!("}} else {{");
                                if let Err(e) =
                                    Self::debug_term_list(else_term_list.clone(), parse_helper)
                                {
                                    pr_err!("Cannot parse else statement of {} Error: {:?}. Continue...",term_list.get_scope_name(),e);
                                    parse_helper.move_current_scope(term_list.get_scope_name());
                                } else {
                                    parse_helper.move_parent_scope()?;
                                }
                            }
                            println!("}}");
                        }
                        StatementOpcode::DefNoop => {
                            println!("(Noop);")
                        }
                        StatementOpcode::DefNotify(notify) => {
                            println!("{:?}", notify);
                        }
                        StatementOpcode::DefRelease(release) => {
                            println!("Release(Mutex:{:?});", release);
                        }
                        StatementOpcode::DefReset(reset) => {
                            println!("Reset({:?})", reset)
                        }
                        StatementOpcode::DefReturn(return_value) => {
                            println!("return {:?};", return_value);
                        }
                        StatementOpcode::DefSignal(signal) => {
                            println!("Signal({:?})", signal);
                        }
                        StatementOpcode::DefSleep(sleep_time) => {
                            println!("Sleep(microsecond:{:?});", sleep_time);
                        }
                        StatementOpcode::DefStall(u_sec_time) => {
                            println!("Stall(millisecond:{:?})", u_sec_time);
                        }
                        StatementOpcode::DefWhile(w) => {
                            println!("while({:?}) {{", w.get_predicate());
                            if let Err(e) =
                                Self::debug_term_list(w.get_term_list().clone(), parse_helper)
                            {
                                pr_err!(
                                    "Cannot parse while statement of {} Error: {:?}. Continue...",
                                    term_list.get_scope_name(),
                                    e
                                );
                                parse_helper.move_current_scope(term_list.get_scope_name());
                            } else {
                                parse_helper.move_parent_scope()?;
                            }
                            println!("}}");
                        }
                    }
                }
                TermObj::ExpressionOpcode(e_o) => {
                    println!("{:?}", e_o);
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
