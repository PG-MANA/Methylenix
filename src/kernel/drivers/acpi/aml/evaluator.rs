//!
//! AML Evaluator
//!

use super::data_object::{
    parse_integer_from_buffer, ComputationalData, ConstData, DataObject, PackageElement,
};
use super::expression_opcode::{
    ByteList, ExpressionOpcode, Package, ReferenceTypeOpcode, VarPackage,
};
use super::name_object::{NameString, SimpleName, SuperName, Target};
use super::named_object::{Field, FieldElement, Method, NamedObject, OperationRegionType};
use super::parser::{ContentObject, ParseHelper};
use super::statement_opcode::{Fatal, IfElse, Notify, StatementOpcode, While};
use super::term_object::{MethodInvocation, TermArg, TermList, TermObj};
use super::{AmlBitFiled, AmlByteFiled, AmlError, AmlPackage, AmlVariable, DataRefObject};

use crate::kernel::sync::spin_lock::Mutex;

use core::mem::MaybeUninit;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

type LocalVariables = [Arc<Mutex<AmlVariable>>; Evaluator::NUMBER_OF_LOCAL_VARIABLES];
type ArgumentVariables = [Arc<Mutex<AmlVariable>>; Evaluator::NUMBER_OF_ARGUMENT_VARIABLES];

pub struct Evaluator {
    parse_helper: ParseHelper,
    variables: Vec<(NameString, Arc<Mutex<AmlVariable>>)>,
}

impl Evaluator {
    const NUMBER_OF_LOCAL_VARIABLES: usize = 7;
    const NUMBER_OF_ARGUMENT_VARIABLES: usize = 7;
    const AML_EVALUATOR_REVISION: u8 = 0;

    pub fn new(parse_helper: ParseHelper) -> Self {
        Self {
            parse_helper,
            variables: Vec::with_capacity(64),
        }
    }

    fn init_local_variables_and_argument_variables() -> (LocalVariables, ArgumentVariables) {
        let mut local_variables: [MaybeUninit<Arc<Mutex<AmlVariable>>>;
            Self::NUMBER_OF_LOCAL_VARIABLES] = MaybeUninit::uninit_array();
        let mut argument_variables: [MaybeUninit<Arc<Mutex<AmlVariable>>>;
            Self::NUMBER_OF_ARGUMENT_VARIABLES] = MaybeUninit::uninit_array();

        let uninitialized_data = Arc::new(Mutex::new(AmlVariable::Uninitialized));

        for e in local_variables.iter_mut() {
            e.write(uninitialized_data.clone());
        }
        for e in argument_variables.iter_mut() {
            e.write(uninitialized_data.clone());
        }
        unsafe {
            (
                MaybeUninit::array_assume_init(local_variables),
                MaybeUninit::array_assume_init(argument_variables),
            )
        }
    }

    fn search_aml_variable(
        &mut self,
        name: &NameString,
        local_variables: &mut LocalVariables,
        argument_variables: &mut ArgumentVariables,
        current_scope: &NameString,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        let object = self
            .parse_helper
            .search_object_from_list_with_parsing_term_list(name)?;
        if object.is_none() {
            pr_err!("Cannot find {}.", name);
            return Err(AmlError::InvalidOperation);
        }

        match object.unwrap() {
            ContentObject::NamedObject(n_o) => match n_o {
                NamedObject::DefBankField(_) => {
                    unimplemented!()
                }
                NamedObject::DefCreateField(f) => {
                    let source = f.get_source_buffer();
                    let source_variable = self.eval_buffer_expression(source)?;
                    return if f.is_bit_field() {
                        let index = self
                            .eval_integer_expression(
                                f.get_index(),
                                local_variables,
                                argument_variables,
                                current_scope,
                            )?
                            .to_int()?;
                        let field_size = if let Some(field_size) = f.get_source_size() {
                            assert_eq!(field_size, 1);
                            field_size
                        } else {
                            self.eval_integer_expression(
                                f.get_source_size_term_arg().as_ref().unwrap(),
                                local_variables,
                                argument_variables,
                                current_scope,
                            )?
                            .to_int()?
                        };
                        let bit_field = Arc::new(Mutex::new(AmlVariable::BitField(AmlBitFiled {
                            source: source_variable,
                            bit_index: index,
                            num_of_bits: field_size,
                            access_align: 1,
                            should_lock_global_lock: false,
                        })));
                        self.variables.push((name.clone(), bit_field.clone()));
                        Ok(bit_field)
                    } else {
                        let index = self
                            .eval_integer_expression(
                                f.get_index(),
                                local_variables,
                                argument_variables,
                                current_scope,
                            )?
                            .to_int()?;
                        let field_size = f.get_source_size().unwrap();
                        let byte_field =
                            Arc::new(Mutex::new(AmlVariable::ByteField(AmlByteFiled {
                                source: source_variable,
                                byte_index: index,
                                num_of_bytes: field_size,
                                should_lock_global_lock: false,
                            })));
                        self.variables.push((name.clone(), byte_field.clone()));
                        Ok(byte_field)
                    };
                }
                NamedObject::DefDataRegion(_) => {
                    unimplemented!();
                }
                NamedObject::DefDevice(_) => {
                    unimplemented!();
                }
                NamedObject::DefField(f) => {
                    let mut access_size = f.get_access_size();
                    let should_lock_global_lock = f.should_lock();
                    let source = self.get_aml_variable(
                        f.get_source_region_name(),
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    let mut index = 0;
                    let mut field_list = f.get_field_list().clone();
                    let relative_name = name
                        .get_relative_name(current_scope)
                        .unwrap_or_else(|| name.clone());

                    while let Some(e) = field_list.next()? {
                        match e {
                            FieldElement::ReservedField(size) => {
                                index += size.length;
                            }
                            FieldElement::AccessField((access_type, access_attribute)) => {
                                access_size = Field::convert_to_access_size(access_type);
                                if access_attribute != 0 {
                                    pr_warn!("Unsupported Attribute: {}", access_attribute);
                                }
                            }
                            FieldElement::ExtendedAccessField(e) => {
                                pr_warn!("Unsupported ExtendedAccessField: {:?}", e);
                                index += e[2] as usize;
                            }
                            FieldElement::ConnectField(c) => {
                                pr_warn!("Unsupported ConnectField: {}", c);
                            }
                            FieldElement::NameField((entry_name, pkg_length)) => {
                                if relative_name.suffix_search(&entry_name) {
                                    pr_info!("Found: {}, index: {}", entry_name, index);
                                    let bit_field =
                                        Arc::new(Mutex::new(AmlVariable::BitField(AmlBitFiled {
                                            source,
                                            bit_index: index,
                                            num_of_bits: pkg_length.length,
                                            access_align: access_size,
                                            should_lock_global_lock,
                                        })));
                                    self.variables.push((name.clone(), bit_field.clone()));
                                    return Ok(bit_field);
                                } else {
                                    index += pkg_length.length;
                                }
                            }
                        }
                    }
                    Err(AmlError::AccessOutOfRange)
                }
                NamedObject::DefEvent(_) => {
                    unimplemented!()
                }
                NamedObject::DefIndexField(_) => {
                    unimplemented!()
                }
                NamedObject::DefMethod(m) => {
                    let variable = Arc::new(Mutex::new(AmlVariable::Method(m)));
                    self.variables.push((name.clone(), variable.clone()));
                    Ok(variable)
                }
                NamedObject::DefMutex(_) => {
                    unimplemented!()
                }
                NamedObject::DefExternal(_) => {
                    unimplemented!()
                }
                NamedObject::DefOpRegion(operation_region) => {
                    let region_type = operation_region.get_operation_type()?;
                    let offset = self
                        .eval_integer_expression(
                            operation_region.get_region_offset(),
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?;
                    let length = self
                        .eval_integer_expression(
                            operation_region.get_region_length(),
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?;
                    let variable = Arc::new(Mutex::new(match region_type {
                        OperationRegionType::SystemMemory => (AmlVariable::MMIo((offset, length))),
                        OperationRegionType::SystemIO => (AmlVariable::MMIo((offset, length))),
                        _ => {
                            pr_err!("Unsupported Type: {:?}", region_type);
                            return Err(AmlError::UnsupportedType);
                        }
                    }));
                    self.variables.push((name.clone(), variable.clone()));
                    Ok(variable)
                }
                NamedObject::DefPowerRes(_) => {
                    unimplemented!()
                }
                NamedObject::DefThermalZone(_) => {
                    unimplemented!()
                }
            },
            ContentObject::DataRefObject(d_o) => match d_o {
                DataRefObject::DataObject(d) => match d {
                    DataObject::ComputationalData(c_d) => {
                        let variable = Arc::new(Mutex::new(match c_d {
                            ComputationalData::ConstData(c) => AmlVariable::ConstData(c),
                            ComputationalData::StringData(s) => {
                                AmlVariable::String(String::from(s))
                            }
                            ComputationalData::ConstObj(c) => {
                                AmlVariable::ConstData(ConstData::Byte(c))
                            }
                            ComputationalData::Revision => AmlVariable::ConstData(ConstData::Byte(
                                Self::AML_EVALUATOR_REVISION,
                            )),
                            ComputationalData::DefBuffer(byte_list) => {
                                AmlVariable::Buffer(self.eval_byte_list(
                                    byte_list,
                                    current_scope,
                                    local_variables,
                                    argument_variables,
                                )?)
                            }
                        }));
                        self.variables.push((name.clone(), variable.clone()));
                        Ok(variable)
                    }
                    DataObject::DefPackage(p) => {
                        let variable = Arc::new(Mutex::new(AmlVariable::Package(
                            self.eval_package(p, name, local_variables, argument_variables)?,
                        )));
                        self.variables.push((name.clone(), variable.clone()));
                        Ok(variable)
                    }
                    DataObject::DefVarPackage(v_p) => {
                        let variable = Arc::new(Mutex::new(AmlVariable::Package(
                            self.eval_var_package(v_p, name, local_variables, argument_variables)?,
                        )));
                        self.variables.push((name.clone(), variable.clone()));
                        Ok(variable)
                    }
                },
                DataRefObject::ObjectReference(d_r) => {
                    pr_err!("Unsupported Type: DataReference({})", d_r);
                    Err(AmlError::UnsupportedType)
                }
            },
            ContentObject::Scope(s) => {
                pr_err!("Invalid Object: {:?}.", s);
                Err(AmlError::InvalidOperation)
            }
        }
    }

    fn get_aml_variable(
        &mut self,
        name: &NameString,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
        current_scope: &NameString,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        if let Some(v) = self.variables.iter().find(|e| &e.0 == name) {
            Ok(v.1.clone())
        } else {
            self.search_aml_variable(name, local_variables, argument_variables, current_scope)
        }
    }

    fn get_aml_variable_from_term_arg(
        &mut self,
        term_arg: TermArg,
        current_scope: &NameString,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
    ) -> Result<AmlVariable, AmlError> {
        match term_arg {
            TermArg::ExpressionOpcode(e) => {
                self.eval_expression(*e, local_variables, argument_variables, current_scope)
            }
            TermArg::DataObject(data_object) => match data_object {
                DataObject::ComputationalData(computational_data) => match computational_data {
                    ComputationalData::ConstData(const_data) => {
                        Ok(AmlVariable::ConstData(const_data))
                    }
                    ComputationalData::StringData(s) => Ok(AmlVariable::String(String::from(s))),
                    ComputationalData::ConstObj(c) => {
                        Ok(AmlVariable::ConstData(ConstData::Byte(c)))
                    }
                    ComputationalData::Revision => Ok(AmlVariable::ConstData(ConstData::Byte(
                        Self::AML_EVALUATOR_REVISION,
                    ))),
                    ComputationalData::DefBuffer(byte_list) => {
                        Ok(AmlVariable::Buffer(self.eval_byte_list(
                            byte_list,
                            current_scope,
                            local_variables,
                            argument_variables,
                        )?))
                    }
                },
                DataObject::DefPackage(p) => Ok(AmlVariable::Package(self.eval_package(
                    p,
                    current_scope,
                    local_variables,
                    argument_variables,
                )?)),
                DataObject::DefVarPackage(p) => Ok(AmlVariable::Package(self.eval_var_package(
                    p,
                    current_scope,
                    local_variables,
                    argument_variables,
                )?)),
            },
            TermArg::ArgObj(c) => Ok(argument_variables[c as usize]
                .try_lock()
                .or(Err(AmlError::MutexError))?
                .clone()),
            TermArg::LocalObj(c) => Ok(local_variables[c as usize]
                .try_lock()
                .or(Err(AmlError::MutexError))?
                .clone()),
        }
    }

    fn get_aml_variable_reference_from_term_arg(
        &mut self,
        term_arg: TermArg,
        current_scope: &NameString,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        match term_arg {
            TermArg::ExpressionOpcode(e) => Ok(Arc::new(Mutex::new(self.eval_expression(
                *e,
                local_variables,
                argument_variables,
                current_scope,
            )?))),
            TermArg::DataObject(data_object) => match data_object {
                DataObject::ComputationalData(computational_data) => match computational_data {
                    ComputationalData::ConstData(const_data) => {
                        Ok(Arc::new(Mutex::new(AmlVariable::ConstData(const_data))))
                    }
                    ComputationalData::StringData(s) => {
                        Ok(Arc::new(Mutex::new(AmlVariable::String(String::from(s)))))
                    }
                    ComputationalData::ConstObj(c) => Ok(Arc::new(Mutex::new(
                        AmlVariable::ConstData(ConstData::Byte(c)),
                    ))),
                    ComputationalData::Revision => Ok(Arc::new(Mutex::new(
                        AmlVariable::ConstData(ConstData::Byte(Self::AML_EVALUATOR_REVISION)),
                    ))),
                    ComputationalData::DefBuffer(byte_list) => Ok(Arc::new(Mutex::new(
                        AmlVariable::Buffer(self.eval_byte_list(
                            byte_list,
                            current_scope,
                            local_variables,
                            argument_variables,
                        )?),
                    ))),
                },
                DataObject::DefPackage(p) => Ok(Arc::new(Mutex::new(AmlVariable::Package(
                    self.eval_package(p, current_scope, local_variables, argument_variables)?,
                )))),
                DataObject::DefVarPackage(p) => Ok(Arc::new(Mutex::new(AmlVariable::Package(
                    self.eval_var_package(p, current_scope, local_variables, argument_variables)?,
                )))),
            },
            TermArg::ArgObj(c) => Ok(argument_variables[c as usize].clone()),
            TermArg::LocalObj(c) => Ok(local_variables[c as usize].clone()),
        }
    }

    fn eval_package(
        &mut self,
        mut p: Package,
        current_scope: &NameString,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
    ) -> Result<Vec<AmlPackage>, AmlError> {
        let num = p.get_number_of_remaining_elements();
        let mut v = Vec::<AmlPackage>::with_capacity(num);

        for i in 0..num {
            match p.get_next_element(current_scope) {
                Ok(Some(element)) => match element {
                    PackageElement::DataRefObject(d) => match d {
                        DataRefObject::DataObject(o) => match o {
                            DataObject::ComputationalData(c_d) => match c_d {
                                ComputationalData::ConstData(const_data) => {
                                    v.push(AmlPackage::ConstData(const_data));
                                }
                                ComputationalData::StringData(s) => {
                                    v.push(AmlPackage::String(String::from(s)));
                                }
                                ComputationalData::ConstObj(c) => {
                                    v.push(AmlPackage::ConstData(ConstData::Byte(c)));
                                }
                                ComputationalData::Revision => {
                                    v.push(AmlPackage::ConstData(ConstData::Byte(
                                        Self::AML_EVALUATOR_REVISION,
                                    )));
                                }
                                ComputationalData::DefBuffer(byte_list) => {
                                    v.push(AmlPackage::Buffer(self.eval_byte_list(
                                        byte_list,
                                        current_scope,
                                        local_variables,
                                        argument_variables,
                                    )?));
                                }
                            },
                            DataObject::DefPackage(package) => {
                                v.push(AmlPackage::Package(self.eval_package(
                                    package,
                                    current_scope,
                                    local_variables,
                                    argument_variables,
                                )?));
                            }
                            DataObject::DefVarPackage(var_package) => {
                                v.push(AmlPackage::Package(self.eval_var_package(
                                    var_package,
                                    current_scope,
                                    local_variables,
                                    argument_variables,
                                )?));
                            }
                        },
                        DataRefObject::ObjectReference(o_r) => {
                            pr_err!("Unsupported ObjectReference: {:#X}", o_r);
                            return Err(AmlError::UnsupportedType);
                        }
                    },
                    PackageElement::NameString(n) => {
                        v.push(AmlPackage::NameString(n));
                    }
                },
                Ok(None) | Err(AmlError::AccessOutOfRange) => {
                    for _ in i..num {
                        v.push(AmlPackage::ConstData(ConstData::Byte(0)))
                    }
                    break;
                }
                Err(e) => Err(e)?,
            }
        }
        return Ok(v);
    }

    fn eval_var_package(
        &mut self,
        mut _p: VarPackage,
        _current_scope: &NameString,
        _local_variables: &mut LocalVariables,
        _argument_variables: &mut LocalVariables,
    ) -> Result<Vec<AmlPackage>, AmlError> {
        unimplemented!()
    }

    fn eval_byte_list(
        &mut self,
        mut byte_list: ByteList,
        current_scope: &NameString,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
    ) -> Result<Vec<u8>, AmlError> {
        let buffer_size_term_arg = byte_list.get_buffer_size(&mut self.parse_helper)?;
        let buffer_size = self
            .eval_integer_expression(
                &buffer_size_term_arg,
                local_variables,
                argument_variables,
                current_scope,
            )?
            .to_int()?;
        let mut buffer = Vec::<u8>::with_capacity(buffer_size);
        for i in 0..buffer_size {
            match byte_list.read_next() {
                Ok(d) => buffer.push(d),
                Err(AmlError::AccessOutOfRange) => {
                    for _ in i..buffer_size {
                        buffer.push(0)
                    }
                    break;
                }
                Err(e) => Err(e)?,
            }
        }
        Ok(buffer)
    }

    fn write_data_into_target(
        &mut self,
        data: AmlVariable,
        target: &Target,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
        current_scope: &NameString,
    ) -> Result<(), AmlError> {
        match target {
            Target::Null => {
                return Err(AmlError::InvalidOperation);
            }
            Target::SuperName(s) => match s {
                SuperName::SimpleName(s_n) => match s_n {
                    SimpleName::NameString(n) => {
                        self.get_aml_variable(
                            n,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .try_lock()
                        .or(Err(AmlError::MutexError))?
                        .write(data)?;
                    }
                    SimpleName::ArgObj(l) => {
                        if argument_variables.len() <= *l as usize {
                            pr_err!("Writing ArgObj({}) is invalid.", l);
                            return Err(AmlError::InvalidOperation);
                        }
                        argument_variables[*l as usize] = Arc::new(Mutex::new(data));
                    }
                    SimpleName::LocalObj(l) => {
                        if (*l as usize) > Self::NUMBER_OF_LOCAL_VARIABLES {
                            pr_err!("Writing LocalObj({}) is invalid.", l);
                            return Err(AmlError::InvalidOperation);
                        }
                        local_variables[*l as usize] = Arc::new(Mutex::new(data));
                    }
                },
                SuperName::DebugObj => {
                    pr_info!("Writing {:?} into Debug Object.", data);
                }
                SuperName::ReferenceTypeOpcode(r) => match &**r {
                    ReferenceTypeOpcode::DefRefOf(d) => {
                        pr_info!("Writing {:?} into DefRefOf({:?}) is invalid.", data, d);
                        return Err(AmlError::InvalidOperation);
                    }
                    ReferenceTypeOpcode::DefDerefOf(reference) => {
                        self.get_aml_variable_from_term_arg(
                            reference.clone(),
                            current_scope,
                            local_variables,
                            argument_variables,
                        )?
                        .write(data)?;
                    }
                    ReferenceTypeOpcode::DefIndex(i) => {
                        let buffer = self.get_aml_variable_reference_from_term_arg(
                            i.get_source().clone(),
                            current_scope,
                            local_variables,
                            argument_variables,
                        )?;
                        let index = self
                            .get_aml_variable_from_term_arg(
                                i.get_index().clone(),
                                current_scope,
                                local_variables,
                                argument_variables,
                            )?
                            .to_int()?;
                        let mut aml_variable = AmlVariable::Reference((buffer, Some(index)));
                        aml_variable.write(data)?;
                        if !i.get_destination().is_null() {
                            self.write_data_into_target(
                                aml_variable,
                                i.get_destination(),
                                local_variables,
                                argument_variables,
                                current_scope,
                            )?;
                        }
                    }
                    ReferenceTypeOpcode::UserTermObj => {
                        pr_err!("UserTermObj is not supported.");
                        return Err(AmlError::InvalidType);
                    }
                },
            },
        }
        return Ok(());
    }

    fn eval_bool_expression(
        &mut self,
        e: &TermArg,
        local_variables: &mut LocalVariables,
        argument_variables: &mut ArgumentVariables,
        current_scope: &NameString,
    ) -> Result<bool, AmlError> {
        match e {
            TermArg::ExpressionOpcode(e) => match &**e {
                ExpressionOpcode::DefCondRefOf((source, result)) => match source {
                    SuperName::SimpleName(simple_name) => match simple_name {
                        SimpleName::NameString(name) => {
                            if let Some(_) = self
                                .parse_helper
                                .search_object_from_list_with_parsing_term_list(name)?
                            {
                                if !result.is_null() {
                                    let _aml_variable = self.get_aml_variable(
                                        name,
                                        local_variables,
                                        argument_variables,
                                        current_scope,
                                    )?;
                                    unimplemented!()
                                } else {
                                    Ok(true)
                                }
                            } else {
                                Ok(false)
                            }
                        }
                        SimpleName::ArgObj(c) => {
                            if let AmlVariable::Uninitialized = *argument_variables[*c as usize]
                                .try_lock()
                                .or(Err(AmlError::MutexError))?
                            {
                                Ok(false)
                            } else {
                                Ok(true)
                            }
                        }
                        SimpleName::LocalObj(c) => {
                            if let AmlVariable::Uninitialized = *local_variables[*c as usize]
                                .try_lock()
                                .or(Err(AmlError::MutexError))?
                            {
                                Ok(false)
                            } else {
                                Ok(true)
                            }
                        }
                    },
                    SuperName::DebugObj => {
                        pr_info!("CondRef DebugObj");
                        Ok(false)
                    }
                    SuperName::ReferenceTypeOpcode(r) => self.eval_bool_expression(
                        &TermArg::ExpressionOpcode(Box::new(
                            ExpressionOpcode::ReferenceTypeOpcode((**r).clone()),
                        )),
                        local_variables,
                        argument_variables,
                        current_scope,
                    ),
                },
                ExpressionOpcode::DefLAnd((left, right)) => Ok(self
                    .eval_integer_expression(
                        left,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?
                    .to_int()?
                    != 0
                    && self
                        .eval_integer_expression(
                            right,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?
                        != 0),
                ExpressionOpcode::DefLEqual((left, right)) => Ok(self
                    .eval_integer_expression(
                        left,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?
                    .to_int()?
                    == self
                        .eval_integer_expression(
                            right,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?),
                ExpressionOpcode::DefLGreater((left, right)) => Ok(self
                    .eval_integer_expression(
                        left,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?
                    .to_int()?
                    > self
                        .eval_integer_expression(
                            right,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?),
                ExpressionOpcode::DefLGreaterEqual((left, right)) => Ok(self
                    .eval_integer_expression(
                        left,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?
                    .to_int()?
                    >= self
                        .eval_integer_expression(
                            right,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?),
                ExpressionOpcode::DefLLess((left, right)) => Ok(self
                    .eval_integer_expression(
                        left,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?
                    .to_int()?
                    < self
                        .eval_integer_expression(
                            right,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?),
                ExpressionOpcode::DefLLessEqual((left, right)) => Ok(self
                    .eval_integer_expression(
                        left,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?
                    .to_int()?
                    <= self
                        .eval_integer_expression(
                            right,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?),
                ExpressionOpcode::DefLNot(source) => Ok(self
                    .eval_integer_expression(
                        source,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?
                    .to_int()?
                    == 0),
                ExpressionOpcode::DefLNotEqual((left, right)) => Ok(self
                    .eval_integer_expression(
                        left,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?
                    .to_int()?
                    != self
                        .eval_integer_expression(
                            right,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?),
                ExpressionOpcode::DefLoad(_) => unimplemented!(),
                ExpressionOpcode::DefLoadTable(_) => unimplemented!(),
                ExpressionOpcode::DefLOr((left, right)) => Ok(self
                    .eval_integer_expression(
                        left,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?
                    .to_int()?
                    != 0
                    || self
                        .eval_integer_expression(
                            right,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?
                        != 0),
                ExpressionOpcode::DefWait(_) => {
                    unimplemented!()
                }
                ExpressionOpcode::ReferenceTypeOpcode(r_e) => match r_e {
                    ReferenceTypeOpcode::DefDerefOf(reference) => Ok(self
                        .get_aml_variable_from_term_arg(
                            reference.clone(),
                            current_scope,
                            local_variables,
                            argument_variables,
                        )?
                        .to_int()?
                        != 0),
                    ReferenceTypeOpcode::UserTermObj => {
                        pr_err!("UserTermObj is not supported.");
                        return Err(AmlError::InvalidType);
                    }
                    _ => {
                        pr_warn!("Expected Boolean, but found {:?}", e);
                        Err(AmlError::InvalidType)
                    }
                },
                ExpressionOpcode::MethodInvocation(method_invocation) => {
                    let obj = self.get_aml_variable(
                        method_invocation.get_name(),
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    let locked_obj = &*obj.try_lock().or(Err(AmlError::MutexError))?;
                    match locked_obj {
                        AmlVariable::ConstData(c) => Ok(c.to_int() != 0),
                        AmlVariable::String(s) => {
                            pr_err!("Expected Boolean, but found {:?}", s);
                            Err(AmlError::InvalidType)
                        }
                        AmlVariable::Buffer(b) => {
                            pr_err!("Expected Boolean, but found {:?}", b);
                            Err(AmlError::InvalidType)
                        }
                        AmlVariable::Package(_)
                        | AmlVariable::ByteField(_)
                        | AmlVariable::BitField(_)
                        | AmlVariable::MMIo(_)
                        | AmlVariable::Reference(_)
                        | AmlVariable::Io(_) => {
                            let const_obj = locked_obj.get_constant_data()?;
                            if let Ok(i) = const_obj.to_int() {
                                Ok(i != 0)
                            } else {
                                pr_err!("Expected Integer, but found {:?}", const_obj);
                                Err(AmlError::InvalidType)
                            }
                        }
                        AmlVariable::Method(method) => {
                            let value = self.eval_method_with_method_invocation(
                                method_invocation,
                                method,
                                &mut Some(local_variables),
                                &mut Some(argument_variables),
                            )?;
                            if let Ok(i) = value.to_int() {
                                Ok(i != 0)
                            } else {
                                pr_err!("Expected Integer, but found {:?}", value);
                                Err(AmlError::InvalidType)
                            }
                        }
                        AmlVariable::Uninitialized => Err(AmlError::InvalidType),
                    }
                }
                _ => {
                    pr_warn!("Expected Boolean, but found {:?}", e);
                    Err(AmlError::InvalidType)
                }
            },
            TermArg::DataObject(d) => {
                if let DataObject::ComputationalData(ComputationalData::ConstData(c_d)) = d {
                    Ok(c_d.to_int() != 0)
                } else {
                    Err(AmlError::InvalidType)
                }
            }
            TermArg::ArgObj(a) => {
                if let AmlVariable::ConstData(c) = &*argument_variables[*a as usize]
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                {
                    Ok(c.to_int() != 0)
                } else {
                    Err(AmlError::InvalidType)
                }
            }
            TermArg::LocalObj(l) => {
                if let AmlVariable::ConstData(c) = &*local_variables[*l as usize]
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                {
                    Ok(c.to_int() != 0)
                } else {
                    Err(AmlError::InvalidType)
                }
            }
        }
    }

    fn eval_integer_expression(
        &mut self,
        e: &TermArg,
        local_variables: &mut LocalVariables,
        argument_variables: &mut ArgumentVariables,
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        match e {
            TermArg::DataObject(d) => match d {
                DataObject::ComputationalData(c_d) => match c_d {
                    ComputationalData::ConstData(c) => Ok(AmlVariable::ConstData(c.clone())),
                    ComputationalData::ConstObj(c) => {
                        Ok(AmlVariable::ConstData(ConstData::Byte(*c)))
                    }
                    ComputationalData::Revision => Ok(AmlVariable::ConstData(ConstData::Byte(
                        Self::AML_EVALUATOR_REVISION,
                    ))),
                    _ => {
                        pr_err!("Expected Integer, but found {:?}", c_d);
                        Err(AmlError::InvalidType)
                    }
                },
                p => {
                    pr_err!("Expected Integer, but found {:?}", p);
                    Err(AmlError::InvalidType)
                }
            },
            TermArg::ArgObj(c) => {
                if *c as usize > Self::NUMBER_OF_ARGUMENT_VARIABLES {
                    pr_err!("Arg{} is out of index.", c);
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok((*argument_variables[*c as usize])
                        .try_lock()
                        .or(Err(AmlError::MutexError))?
                        .clone())
                }
            }
            TermArg::LocalObj(c) => {
                if *c as usize > Self::NUMBER_OF_LOCAL_VARIABLES {
                    pr_err!("Local{} is out of index.", c);
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok((*local_variables[*c as usize])
                        .try_lock()
                        .or(Err(AmlError::MutexError))?
                        .clone())
                }
            }
            TermArg::ExpressionOpcode(expression) => match &**expression {
                ExpressionOpcode::BinaryOperation(b_o) => {
                    let left = self.eval_integer_expression(
                        b_o.get_left_operand(),
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    let right = self.eval_integer_expression(
                        b_o.get_right_operand(),
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    use super::opcode;
                    let result = match b_o.get_opcode() {
                        opcode::ADD_OP => left.to_int().unwrap() + right.to_int().unwrap(),
                        opcode::AND_OP => left.to_int().unwrap() & right.to_int().unwrap(),
                        opcode::MULTIPLY_OP => left.to_int().unwrap() * right.to_int().unwrap(),
                        opcode::NAND_OP => !left.to_int().unwrap() | !right.to_int().unwrap(),
                        opcode::MOD_OP => left.to_int().unwrap() % right.to_int().unwrap(),
                        opcode::NOR_OP => !left.to_int().unwrap() & !right.to_int().unwrap(),
                        opcode::OR_OP => left.to_int().unwrap() | right.to_int().unwrap(),
                        opcode::SHIFT_LEFT_OP => left.to_int().unwrap() << right.to_int().unwrap(),
                        opcode::SHIFT_RIGHT_OP => left.to_int().unwrap() >> right.to_int().unwrap(),
                        opcode::SUBTRACT_OP => left.to_int().unwrap() - right.to_int().unwrap(),
                        opcode::XOR_OP => left.to_int().unwrap() ^ right.to_int().unwrap(),
                        _ => {
                            unreachable!()
                        }
                    };
                    let result_aml_variable = AmlVariable::ConstData(ConstData::from_usize(
                        result,
                        left.get_byte_size()?.max(right.get_byte_size()?),
                    )?);
                    if let Target::SuperName(_) = b_o.get_target() {
                        self.write_data_into_target(
                            result_aml_variable.clone(),
                            b_o.get_target(),
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?;
                    }
                    Ok(result_aml_variable)
                }
                ExpressionOpcode::DefCopyObject(_, _) => Err(AmlError::UnsupportedType),
                ExpressionOpcode::DefDecrement(decrement) => match decrement {
                    SuperName::SimpleName(simple_name) => match simple_name {
                        SimpleName::NameString(name) => {
                            let obj = self.get_aml_variable(
                                &name,
                                local_variables,
                                argument_variables,
                                current_scope,
                            )?;
                            if let AmlVariable::ConstData(c) =
                                &mut *obj.try_lock().or(Err(AmlError::MutexError))?
                            {
                                *c = ConstData::from_usize(
                                    c.to_int().overflowing_sub(1).0,
                                    c.get_byte_size(),
                                )?;
                                Ok(AmlVariable::ConstData(c.clone()))
                            } else {
                                pr_err!("Expected Integer, but found {:?}", obj);
                                Err(AmlError::InvalidOperation)
                            }
                        }
                        SimpleName::ArgObj(arg) => {
                            if *arg as usize > Self::NUMBER_OF_ARGUMENT_VARIABLES {
                                pr_err!("Arg{} is out of index.", arg);
                                Err(AmlError::InvalidOperation)
                            } else {
                                if let AmlVariable::ConstData(c) = &mut *argument_variables
                                    [*arg as usize]
                                    .try_lock()
                                    .or(Err(AmlError::MutexError))?
                                {
                                    *c = ConstData::from_usize(
                                        c.to_int().overflowing_sub(1).0,
                                        c.get_byte_size(),
                                    )?;
                                    Ok(AmlVariable::ConstData(c.clone()))
                                } else {
                                    pr_err!(
                                        "Expected Integer, but found {:?}",
                                        argument_variables[*arg as usize]
                                    );
                                    Err(AmlError::InvalidOperation)
                                }
                            }
                        }
                        SimpleName::LocalObj(l) => {
                            if *l as usize > Self::NUMBER_OF_LOCAL_VARIABLES {
                                pr_err!("LocalObj{} is out of index.", l);
                                Err(AmlError::InvalidOperation)
                            } else {
                                if let AmlVariable::ConstData(c) = &mut *local_variables
                                    [*l as usize]
                                    .try_lock()
                                    .or(Err(AmlError::MutexError))?
                                {
                                    *c = ConstData::from_usize(
                                        c.to_int().overflowing_sub(1).0,
                                        c.get_byte_size(),
                                    )?;
                                    Ok(AmlVariable::ConstData(c.clone()))
                                } else {
                                    pr_err!(
                                        "Expected Integer, but found {:?}",
                                        local_variables[*l as usize]
                                    );
                                    Err(AmlError::InvalidOperation)
                                }
                            }
                        }
                    },
                    SuperName::DebugObj => {
                        pr_info!("DebugObj--");
                        Err(AmlError::UnsupportedType)
                    }
                    SuperName::ReferenceTypeOpcode(r_o) => self.eval_integer_expression(
                        &TermArg::ExpressionOpcode(Box::new(
                            ExpressionOpcode::ReferenceTypeOpcode((**r_o).clone()),
                        )),
                        local_variables,
                        argument_variables,
                        current_scope,
                    ),
                },

                ExpressionOpcode::DefIncrement(increment) => match increment {
                    SuperName::SimpleName(simple_name) => match simple_name {
                        SimpleName::NameString(name) => {
                            let obj = self.get_aml_variable(
                                &name,
                                local_variables,
                                argument_variables,
                                current_scope,
                            )?;
                            if let AmlVariable::ConstData(c) =
                                &mut *obj.try_lock().or(Err(AmlError::MutexError))?
                            {
                                *c = ConstData::from_usize(
                                    c.to_int().overflowing_add(1).0,
                                    c.get_byte_size(),
                                )?;
                                Ok(AmlVariable::ConstData(c.clone()))
                            } else {
                                pr_err!("Expected Integer, but found {:?}", obj);
                                Err(AmlError::InvalidOperation)
                            }
                        }
                        SimpleName::ArgObj(arg) => {
                            if *arg as usize > Self::NUMBER_OF_ARGUMENT_VARIABLES {
                                pr_err!("Arg{} is out of index.", arg);
                                Err(AmlError::InvalidOperation)
                            } else {
                                if let AmlVariable::ConstData(c) = &mut *argument_variables
                                    [*arg as usize]
                                    .try_lock()
                                    .or(Err(AmlError::MutexError))?
                                {
                                    *c = ConstData::from_usize(
                                        c.to_int().overflowing_add(1).0,
                                        c.get_byte_size(),
                                    )?;
                                    Ok(AmlVariable::ConstData(c.clone()))
                                } else {
                                    pr_err!(
                                        "Expected Integer, but found {:?}",
                                        argument_variables[*arg as usize]
                                    );
                                    Err(AmlError::InvalidOperation)
                                }
                            }
                        }
                        SimpleName::LocalObj(l) => {
                            if *l as usize > Self::NUMBER_OF_LOCAL_VARIABLES {
                                pr_err!("LocalObj{} is out of index.", l);
                                Err(AmlError::InvalidOperation)
                            } else {
                                if let AmlVariable::ConstData(c) = &mut *local_variables
                                    [*l as usize]
                                    .try_lock()
                                    .or(Err(AmlError::MutexError))?
                                {
                                    *c = ConstData::from_usize(
                                        c.to_int().overflowing_add(1).0,
                                        c.get_byte_size(),
                                    )?;
                                    Ok(AmlVariable::ConstData(c.clone()))
                                } else {
                                    pr_err!(
                                        "Expected Integer, but found {:?}",
                                        local_variables[*l as usize]
                                    );
                                    Err(AmlError::InvalidOperation)
                                }
                            }
                        }
                    },
                    SuperName::DebugObj => {
                        pr_info!("DebugObj++");
                        Err(AmlError::UnsupportedType)
                    }
                    SuperName::ReferenceTypeOpcode(_) => Err(AmlError::UnsupportedType),
                },
                ExpressionOpcode::DefDivide(divide) => {
                    let dividend = self.eval_integer_expression(
                        divide.get_dividend(),
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    let divisor = self.eval_integer_expression(
                        divide.get_divisor(),
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    let dividend_data = dividend.to_int().or(Err(AmlError::InvalidOperation))?;
                    let divisor_data = divisor.to_int().or(Err(AmlError::InvalidOperation))?;
                    let result_size = dividend.get_byte_size()?.max(divisor.get_byte_size()?);
                    let result_data = dividend_data / divisor_data;
                    let result_aml_variable =
                        AmlVariable::ConstData(ConstData::from_usize(result_data, result_size)?);
                    if !divide.get_quotient().is_null() {
                        self.write_data_into_target(
                            result_aml_variable.clone(),
                            divide.get_quotient(),
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?;
                    }
                    if !divide.get_remainder().is_null() {
                        let remainder = AmlVariable::ConstData(ConstData::from_usize(
                            dividend_data - result_data * divisor_data,
                            result_size,
                        )?);
                        self.write_data_into_target(
                            remainder,
                            divide.get_remainder(),
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?;
                    }
                    Ok(result_aml_variable)
                }
                ExpressionOpcode::DefFindSetLeftBit((operand, target)) => {
                    let operand_data:usize /* To detect error when changed the return type of to_int()*/
                        = self.eval_integer_expression(&operand,local_variables,argument_variables,current_scope)?.to_int()?;
                    let result = AmlVariable::ConstData(ConstData::Byte(
                        (usize::BITS - operand_data.leading_zeros()) as u8,
                    ));
                    if !target.is_null() {
                        self.write_data_into_target(
                            result.clone(),
                            &target,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?;
                    }
                    Ok(result)
                }
                ExpressionOpcode::DefFindSetRightBit((operand, target)) => {
                    let operand_data:usize /* To detect error when changed the return type of to_int()*/
                        = self.eval_integer_expression(&operand,local_variables,argument_variables,current_scope)?.to_int()?;
                    let result = AmlVariable::ConstData(ConstData::Byte(if operand_data == 0 {
                        0
                    } else {
                        (operand_data.trailing_zeros() + 1) as u8
                    }));
                    if !target.is_null() {
                        self.write_data_into_target(
                            result.clone(),
                            &target,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?;
                    }
                    Ok(result)
                }
                ExpressionOpcode::DefFromBCD(_) => {
                    unimplemented!()
                }
                ExpressionOpcode::DefMatch(_) => {
                    unimplemented!()
                }
                ExpressionOpcode::DefNot((operand, target)) => {
                    let op = self.eval_integer_expression(
                        &operand,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    if let AmlVariable::ConstData(c) = op {
                        let result_aml_variables = AmlVariable::ConstData(match c {
                            ConstData::Byte(data) => ConstData::Byte(!data),
                            ConstData::Word(data) => ConstData::Word(!data),
                            ConstData::DWord(data) => ConstData::DWord(!data),
                            ConstData::QWord(data) => ConstData::QWord(!data),
                        });
                        if !target.is_null() {
                            self.write_data_into_target(
                                result_aml_variables.clone(),
                                &target,
                                local_variables,
                                argument_variables,
                                current_scope,
                            )?;
                        }
                        Ok(result_aml_variables)
                    } else {
                        pr_err!("Expected Integer, but found {:?}", op);
                        Err(AmlError::InvalidOperation)
                    }
                }
                ExpressionOpcode::DefObjectType(_) => {
                    unimplemented!()
                }
                ExpressionOpcode::DefSizeOf(obj_name) => {
                    let obj = match obj_name {
                        SuperName::SimpleName(simple_name) => match simple_name {
                            SimpleName::NameString(name_string) => self.get_aml_variable(
                                &name_string,
                                local_variables,
                                argument_variables,
                                current_scope,
                            )?,
                            SimpleName::ArgObj(c) => {
                                if *c as usize > Self::NUMBER_OF_ARGUMENT_VARIABLES {
                                    pr_err!("ArgObj{} is out of index.", c);
                                    Err(AmlError::InvalidOperation)?
                                } else {
                                    argument_variables[*c as usize].clone()
                                }
                            }
                            SimpleName::LocalObj(c) => {
                                if *c as usize > Self::NUMBER_OF_LOCAL_VARIABLES {
                                    pr_err!("LocalObj{} is out of index.", c);
                                    Err(AmlError::InvalidOperation)?
                                } else {
                                    local_variables[*c as usize].clone()
                                }
                            }
                        },
                        SuperName::DebugObj => Err(AmlError::UnsupportedType)?,
                        SuperName::ReferenceTypeOpcode(_) => {
                            unimplemented!()
                        }
                    };
                    let byte_size = match &*obj.try_lock().or(Err(AmlError::MutexError))? {
                        AmlVariable::ConstData(c) => c.get_byte_size(),
                        AmlVariable::String(s) => s.len(),
                        AmlVariable::Buffer(b) => b.len(),
                        AmlVariable::Io(_) => Err(AmlError::InvalidOperation)?,
                        AmlVariable::MMIo(_) => Err(AmlError::InvalidOperation)?,
                        AmlVariable::BitField(b) => b.access_align.max(b.num_of_bits >> 3),
                        AmlVariable::ByteField(b) => b.num_of_bytes,
                        AmlVariable::Package(p) => p.len(), /* OK? */
                        AmlVariable::Method(_) => Err(AmlError::InvalidOperation)?,
                        AmlVariable::Uninitialized => Err(AmlError::InvalidOperation)?,
                        AmlVariable::Reference((s, _)) => s
                            .try_lock()
                            .or(Err(AmlError::MutexError))?
                            .get_byte_size()?,
                    };
                    Ok(AmlVariable::ConstData(ConstData::QWord(byte_size as _)))
                }
                ExpressionOpcode::DefStore((data, destination)) => {
                    let data = self.eval_integer_expression(
                        &data,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    if data.to_int().is_err() {
                        pr_err!("Expected Integer, but found {:?}", data);
                        Err(AmlError::InvalidType)
                    } else {
                        self.write_data_into_target(
                            data.clone(),
                            &Target::SuperName(destination.clone()),
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?;
                        Ok(data)
                    }
                }
                ExpressionOpcode::DefToBCD(_) => unimplemented!(),
                ExpressionOpcode::DefToInteger((operand, target)) => {
                    let result = match operand {
                        TermArg::ExpressionOpcode(_) => {
                            unimplemented!()
                        }
                        TermArg::DataObject(data) => match data {
                            DataObject::ComputationalData(c_d) => match c_d {
                                ComputationalData::ConstData(d) => d.clone(),
                                ComputationalData::StringData(s) => {
                                    ConstData::QWord(parse_integer_from_buffer(s.as_bytes())? as _)
                                }
                                ComputationalData::ConstObj(c) => ConstData::Byte(*c),
                                ComputationalData::Revision => {
                                    ConstData::Byte(Self::AML_EVALUATOR_REVISION)
                                }
                                ComputationalData::DefBuffer(b) => {
                                    let mut b = b.clone();
                                    let size = b.get_buffer_size(&mut self.parse_helper)?;
                                    let buffer_size = self
                                        .eval_integer_expression(
                                            &size,
                                            local_variables,
                                            argument_variables,
                                            current_scope,
                                        )?
                                        .to_int()?;
                                    if buffer_size < 8 {
                                        pr_err!("Invalid Buffer Size: {:#X}.", buffer_size);
                                        Err(AmlError::InvalidOperation)?
                                    } else {
                                        let mut result = 0u64;
                                        for index in 0..8 {
                                            result |= (b.read_next()? as u64) << index;
                                        }
                                        ConstData::QWord(result)
                                    }
                                }
                            },
                            d => {
                                pr_err!("Expected Data, but found: {:?}", d);
                                Err(AmlError::InvalidOperation)?
                            }
                        },
                        TermArg::ArgObj(_) => {
                            unimplemented!()
                        }
                        TermArg::LocalObj(_) => {
                            unimplemented!()
                        }
                    };
                    if !target.is_null() {
                        self.write_data_into_target(
                            AmlVariable::ConstData(result),
                            &target,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?;
                    }
                    Ok(AmlVariable::ConstData(result))
                }
                ExpressionOpcode::DefTimer => {
                    unimplemented!()
                }
                ExpressionOpcode::ReferenceTypeOpcode(r_o) => match r_o {
                    ReferenceTypeOpcode::DefDerefOf(reference) => self
                        .get_aml_variable_from_term_arg(
                            reference.clone(),
                            current_scope,
                            local_variables,
                            argument_variables,
                        ),
                    ReferenceTypeOpcode::UserTermObj => {
                        pr_err!("UserTermObj is not supported.");
                        return Err(AmlError::InvalidType);
                    }
                    _ => {
                        pr_warn!("Expected Boolean, but found {:?}", e);
                        Err(AmlError::InvalidType)
                    }
                },
                ExpressionOpcode::MethodInvocation(method_invocation) => {
                    let obj = self.get_aml_variable(
                        method_invocation.get_name(),
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    let locked_obj = &*obj.try_lock().or(Err(AmlError::MutexError))?;
                    match locked_obj {
                        AmlVariable::ConstData(c) => Ok(AmlVariable::ConstData(c.clone())),
                        AmlVariable::String(s) => {
                            pr_err!("Expected Integer, but found {:?}", s);
                            Err(AmlError::InvalidType)
                        }
                        AmlVariable::Buffer(b) => {
                            pr_err!("Expected Integer, but found {:?}", b);
                            Err(AmlError::InvalidType)
                        }
                        AmlVariable::Package(_)
                        | AmlVariable::ByteField(_)
                        | AmlVariable::BitField(_)
                        | AmlVariable::MMIo(_)
                        | AmlVariable::Reference(_)
                        | AmlVariable::Io(_) => {
                            let const_obj = locked_obj.get_constant_data()?;
                            if const_obj.to_int().is_err() {
                                pr_err!("Expected Integer, but found {:?}", const_obj);
                                Err(AmlError::InvalidType)
                            } else {
                                Ok(const_obj)
                            }
                        }
                        AmlVariable::Method(method) => {
                            let value = self.eval_method_with_method_invocation(
                                method_invocation,
                                method,
                                &mut Some(local_variables),
                                &mut Some(argument_variables),
                            )?;
                            if value.to_int().is_ok() {
                                Ok(value)
                            } else {
                                pr_err!("Expected Integer, but found {:?}", e);
                                Err(AmlError::InvalidType)
                            }
                        }
                        AmlVariable::Uninitialized => Err(AmlError::InvalidType),
                    }
                }
                other => {
                    let result = self.eval_bool_expression(
                        e,
                        local_variables,
                        argument_variables,
                        current_scope,
                    );
                    if let Ok(b) = result {
                        pr_info!("Cast boolean({}) to integer.", b);
                        Ok(AmlVariable::ConstData(ConstData::Byte(b as _)))
                    } else {
                        pr_err!("Expected Integer, but found {:?}", other);
                        Err(result.unwrap_err())
                    }
                }
            },
        }
    }

    fn eval_expression(
        &mut self,
        e: ExpressionOpcode,
        local_variables: &mut LocalVariables,
        argument_variables: &mut ArgumentVariables,
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        match e {
            ExpressionOpcode::DefAcquire(_) => {
                unimplemented!()
            }
            ExpressionOpcode::DefBuffer(byte_list) => {
                Ok(AmlVariable::Buffer(self.eval_byte_list(
                    byte_list,
                    current_scope,
                    local_variables,
                    argument_variables,
                )?))
            }
            ExpressionOpcode::DefPackage(p) => Ok(AmlVariable::Package(self.eval_package(
                p,
                current_scope,
                local_variables,
                argument_variables,
            )?)),
            ExpressionOpcode::DefVarPackage(var_package) => {
                Ok(AmlVariable::Package(self.eval_var_package(
                    var_package,
                    current_scope,
                    local_variables,
                    argument_variables,
                )?))
            }
            ExpressionOpcode::DefProcessor => {
                pr_err!("DefProcessor was deleted from ACPI 6.4.");
                Err(AmlError::InvalidOperation)
            }
            ExpressionOpcode::DefConcat(_) => {
                unimplemented!()
            }
            ExpressionOpcode::DefConcatRes(_) => {
                unimplemented!()
            }
            ExpressionOpcode::DefCopyObject(_, _) => {
                unimplemented!()
            }
            ExpressionOpcode::BinaryOperation(_)
            | ExpressionOpcode::DefCondRefOf(_)
            | ExpressionOpcode::DefDecrement(_)
            | ExpressionOpcode::DefDivide(_)
            | ExpressionOpcode::DefFindSetLeftBit(_)
            | ExpressionOpcode::DefFindSetRightBit(_)
            | ExpressionOpcode::DefFromBCD(_)
            | ExpressionOpcode::DefIncrement(_)
            | ExpressionOpcode::DefMatch(_)
            | ExpressionOpcode::DefMid(_)
            | ExpressionOpcode::DefObjectType(_)
            | ExpressionOpcode::DefSizeOf(_)
            | ExpressionOpcode::DefToBCD(_)
            | ExpressionOpcode::DefTimer
            | ExpressionOpcode::DefNot(_) => self.eval_integer_expression(
                &TermArg::ExpressionOpcode(Box::new(e)),
                local_variables,
                argument_variables,
                current_scope,
            ),
            ExpressionOpcode::DefLAnd(_)
            | ExpressionOpcode::DefLEqual(_)
            | ExpressionOpcode::DefLGreater(_)
            | ExpressionOpcode::DefLGreaterEqual(_)
            | ExpressionOpcode::DefLLess(_)
            | ExpressionOpcode::DefLLessEqual(_)
            | ExpressionOpcode::DefLNot(_)
            | ExpressionOpcode::DefLOr(_)
            | ExpressionOpcode::DefToInteger(_)
            | ExpressionOpcode::DefLNotEqual(_)
            | ExpressionOpcode::DefLoad(_)
            | ExpressionOpcode::DefLoadTable(_)
            | ExpressionOpcode::DefWait(_) => Ok(AmlVariable::ConstData(ConstData::Byte(
                self.eval_bool_expression(
                    &TermArg::ExpressionOpcode(Box::new(e)),
                    local_variables,
                    argument_variables,
                    current_scope,
                )? as _,
            ))),

            ExpressionOpcode::DefStore((source, destination)) => {
                let source_aml_variable = self
                    .get_aml_variable_from_term_arg(
                        source,
                        current_scope,
                        local_variables,
                        argument_variables,
                    )?
                    .get_constant_data()?;

                self.write_data_into_target(
                    source_aml_variable.clone(),
                    &Target::SuperName(destination),
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                Ok(source_aml_variable)
            }
            ExpressionOpcode::DefToBuffer(_) => {
                unimplemented!()
            }
            ExpressionOpcode::DefToDecimalString(_) => {
                unimplemented!()
            }
            ExpressionOpcode::DefToHexString(_) => {
                unimplemented!()
            }
            ExpressionOpcode::DefToString(_) => {
                unimplemented!()
            }
            ExpressionOpcode::ReferenceTypeOpcode(r_o) => match r_o {
                ReferenceTypeOpcode::DefRefOf(_) => {
                    unimplemented!()
                }
                ReferenceTypeOpcode::DefDerefOf(reference) => self.get_aml_variable_from_term_arg(
                    reference.clone(),
                    current_scope,
                    local_variables,
                    argument_variables,
                ),
                ReferenceTypeOpcode::DefIndex(i) => {
                    let buffer = self.get_aml_variable_reference_from_term_arg(
                        i.get_source().clone(),
                        current_scope,
                        local_variables,
                        argument_variables,
                    )?;
                    let index = self
                        .get_aml_variable_from_term_arg(
                            i.get_index().clone(),
                            current_scope,
                            local_variables,
                            argument_variables,
                        )?
                        .to_int()?;
                    let aml_variable = AmlVariable::Reference((buffer, Some(index)));
                    if !i.get_destination().is_null() {
                        self.write_data_into_target(
                            aml_variable.clone(),
                            i.get_destination(),
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?;
                    }
                    Ok(aml_variable)
                }
                ReferenceTypeOpcode::UserTermObj => {
                    pr_err!("UserTermObj is not supported.");
                    return Err(AmlError::InvalidType);
                }
            },
            ExpressionOpcode::MethodInvocation(method_invocation) => {
                let obj = self.get_aml_variable(
                    method_invocation.get_name(),
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                match &*obj.try_lock().or(Err(AmlError::MutexError))? {
                    AmlVariable::Method(method) => self.eval_method_with_method_invocation(
                        &method_invocation,
                        method,
                        &mut Some(local_variables),
                        &mut Some(argument_variables),
                    ),
                    o => Ok(o.clone()),
                }
            }
        }
    }

    fn eval_buffer_expression(
        &mut self,
        _term_arg: &TermArg,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        unimplemented!()
    }

    fn eval_notify(&mut self, _notify: Notify) -> Result<(), AmlError> {
        unimplemented!()
    }

    fn release_mutex(&mut self, _mutex_object: &SuperName) -> Result<(), AmlError> {
        unimplemented!()
    }

    fn reset_event(&mut self, _event: &SuperName) -> Result<(), AmlError> {
        unimplemented!()
    }

    fn eval_break_point(&self, term_list: &TermList) {
        pr_info!("AML BreakPoint: {:?}", term_list);
    }

    fn eval_fatal(&self, fatal: &Fatal, term_list: &TermList) -> Result<(), AmlError> {
        pr_info!("AML Fatal: {:?} ({:?})", fatal, term_list);
        return Ok(());
    }

    fn eval_signal(&self, _signal: &SuperName) -> Result<(), AmlError> {
        unimplemented!()
    }

    fn eval_sleep(&self, _milli_seconds: &TermArg) -> Result<(), AmlError> {
        unimplemented!()
    }

    fn eval_stall(&self, _micro_seconds: &TermArg) -> Result<(), AmlError> {
        unimplemented!()
    }

    fn eval_if_else(
        &mut self,
        i_e: IfElse,
        local_variables: &mut LocalVariables,
        argument_variables: &mut ArgumentVariables,
        current_scope: &NameString,
    ) -> Result<Option<StatementOpcode>, AmlError> {
        let predicate = i_e.get_predicate();
        if self.eval_bool_expression(
            predicate,
            local_variables,
            argument_variables,
            current_scope,
        )? {
            let true_statement = i_e.get_if_true_term_list();
            self.parse_helper
                .move_into_term_list(true_statement.clone())?;
            let result = self._eval_term_list(
                true_statement.clone(),
                local_variables,
                argument_variables,
                current_scope,
            );
            self.parse_helper.move_out_from_current_term_list()?;
            result
        } else if let Some(false_statement) = i_e.get_if_false_term_list() {
            self.parse_helper
                .move_into_term_list(false_statement.clone())?;
            let result = self._eval_term_list(
                false_statement.clone(),
                local_variables,
                argument_variables,
                current_scope,
            );
            self.parse_helper.move_out_from_current_term_list()?;
            result
        } else {
            Ok(None)
        }
    }

    fn eval_while(
        &mut self,
        w: While,
        local_variables: &mut LocalVariables,
        argument_variables: &mut ArgumentVariables,
        current_scope: &NameString,
    ) -> Result<Option<StatementOpcode>, AmlError> {
        let predicate = w.get_predicate();
        let term_list = w.get_term_list();
        self.parse_helper.move_into_term_list(term_list.clone())?;
        loop {
            if self.eval_bool_expression(
                predicate,
                local_variables,
                argument_variables,
                current_scope,
            )? == false
            {
                return Ok(None);
            }
            let mut t = term_list.clone();

            while let Some(term_obj) = t.next(&mut self.parse_helper)? {
                match term_obj {
                    TermObj::NamespaceModifierObj(_) => { /* Ignore */ }
                    TermObj::NamedObj(_) => { /* Ignore */ }
                    TermObj::StatementOpcode(s_o) => match s_o {
                        StatementOpcode::DefBreak => {
                            self.parse_helper.move_out_from_current_term_list()?;
                            return Ok(None);
                        }
                        StatementOpcode::DefBreakPoint => {
                            self.eval_break_point(&t);
                        }
                        StatementOpcode::DefContinue => {
                            break;
                        }
                        StatementOpcode::DefFatal(f) => {
                            let result = self.eval_fatal(&f, &t);
                            self.parse_helper.move_out_from_current_term_list()?;
                            return result.and(Ok(Some(StatementOpcode::DefFatal(f))));
                        }
                        StatementOpcode::DefIfElse(i_e) => {
                            let result = self.eval_if_else(
                                i_e,
                                local_variables,
                                argument_variables,
                                current_scope,
                            );
                            if result.is_err() {
                                self.parse_helper.move_out_from_current_term_list()?;
                                return result;
                            } else if matches!(result, Ok(Some(StatementOpcode::DefBreak)))
                                || matches!(result, Ok(None))
                            {
                                /* Catch */
                            } else {
                                self.parse_helper.move_out_from_current_term_list()?;
                                return result;
                            }
                        }
                        StatementOpcode::DefNoop => { /* Do Nothing */ }
                        StatementOpcode::DefNotify(n) => {
                            if let Err(e) = self.eval_notify(n) {
                                self.parse_helper.move_out_from_current_term_list()?;
                                return Err(e);
                            }
                        }
                        StatementOpcode::DefRelease(m) => {
                            if let Err(e) = self.release_mutex(&m) {
                                self.parse_helper.move_out_from_current_term_list()?;
                                return Err(e);
                            }
                        }
                        StatementOpcode::DefReset(event) => {
                            if let Err(e) = self.reset_event(&event) {
                                self.parse_helper.move_out_from_current_term_list()?;
                                return Err(e);
                            }
                        }
                        StatementOpcode::DefReturn(value) => {
                            self.parse_helper.move_out_from_current_term_list()?;
                            return Ok(Some(StatementOpcode::DefReturn(value)));
                        }
                        StatementOpcode::DefSignal(signal) => {
                            if let Err(e) = self.eval_signal(&signal) {
                                self.parse_helper.move_out_from_current_term_list()?;
                                return Err(e);
                            }
                        }
                        StatementOpcode::DefSleep(sleep) => {
                            if let Err(e) = self.eval_sleep(&sleep) {
                                self.parse_helper.move_out_from_current_term_list()?;
                                return Err(e);
                            }
                        }
                        StatementOpcode::DefStall(sleep) => {
                            if let Err(e) = self.eval_stall(&sleep) {
                                self.parse_helper.move_out_from_current_term_list()?;
                                return Err(e);
                            }
                        }
                        StatementOpcode::DefWhile(w) => {
                            let result = self.eval_while(
                                w,
                                local_variables,
                                argument_variables,
                                current_scope,
                            );
                            if result.is_err() {
                                self.parse_helper.move_out_from_current_term_list()?;
                                return result;
                            } else if matches!(result, Ok(None)) {
                                /* Continue */
                            } else {
                                self.parse_helper.move_out_from_current_term_list()?;
                                return result;
                            }
                        }
                    },
                    TermObj::ExpressionOpcode(e_o) => {
                        if let Err(err) = self.eval_expression(
                            e_o,
                            local_variables,
                            argument_variables,
                            current_scope,
                        ) {
                            self.parse_helper.move_out_from_current_term_list()?;
                            return Err(err);
                        }
                    }
                }
            }
        }
    }

    fn _eval_term_list(
        &mut self,
        mut term_list: TermList,
        local_variables: &mut LocalVariables,
        argument_variables: &mut ArgumentVariables,
        current_scope: &NameString,
    ) -> Result<Option<StatementOpcode>, AmlError> {
        while let Some(term_obj) = term_list.next(&mut self.parse_helper)? {
            match term_obj {
                TermObj::NamespaceModifierObj(_) => { /* Ignore */ }
                TermObj::NamedObj(_) => { /* Ignore */ }
                TermObj::StatementOpcode(s_o) => match s_o {
                    StatementOpcode::DefNoop => { /* Do Nothing */ }
                    StatementOpcode::DefNotify(n) => {
                        self.eval_notify(n)?;
                    }
                    StatementOpcode::DefRelease(m) => {
                        self.release_mutex(&m)?;
                    }
                    StatementOpcode::DefReset(event) => {
                        self.reset_event(&event)?;
                    }
                    StatementOpcode::DefReturn(value) => {
                        return Ok(Some(StatementOpcode::DefReturn(value)));
                    }
                    StatementOpcode::DefSignal(signal) => {
                        self.eval_signal(&signal)?;
                    }
                    StatementOpcode::DefSleep(sleep) => {
                        self.eval_sleep(&sleep)?;
                    }
                    StatementOpcode::DefStall(sleep) => {
                        self.eval_stall(&sleep)?;
                    }
                    StatementOpcode::DefWhile(w) => {
                        let result =
                            self.eval_while(w, local_variables, argument_variables, current_scope);
                        if result.is_err() {
                            return result;
                        } else if matches!(result, Ok(None)) {
                            /* Continue */
                        } else {
                            return result;
                        }
                    }
                    StatementOpcode::DefBreak => {
                        return Ok(Some(StatementOpcode::DefBreak));
                    }
                    StatementOpcode::DefBreakPoint => {
                        self.eval_break_point(&term_list);
                    }
                    StatementOpcode::DefContinue => {
                        return Ok(Some(StatementOpcode::DefContinue));
                    }
                    StatementOpcode::DefFatal(f) => {
                        self.eval_fatal(&f, &term_list)?;
                        return Ok(Some(StatementOpcode::DefFatal(f)));
                    }
                    StatementOpcode::DefIfElse(i_e) => {
                        let result = self.eval_if_else(
                            i_e,
                            local_variables,
                            argument_variables,
                            current_scope,
                        );
                        if result.is_err() {
                            return result;
                        } else if matches!(result, Ok(None)) {
                            /* Continue */
                        } else {
                            return result;
                        }
                    }
                },
                TermObj::ExpressionOpcode(e_o) => {
                    self.eval_expression(e_o, local_variables, argument_variables, current_scope)?;
                }
            }
        }
        return Ok(None);
    }

    pub fn eval_method(&mut self, method: &Method) -> Result<AmlVariable, AmlError> {
        /* TODO: adjust parse_helper's term list */
        let (mut local_variables, mut argument_variables) =
            Self::init_local_variables_and_argument_variables();
        let current_scope = method.get_name();
        if method.get_argument_count() != 0 {
            pr_err!(
                "Expected {} arguments(TODO: give arguments...).",
                method.get_argument_count()
            );
            return Err(AmlError::InvalidOperation);
        }
        self.parse_helper
            .move_into_term_list(method.get_term_list().clone())?;
        let result = self._eval_term_list(
            method.get_term_list().clone(),
            &mut local_variables,
            &mut argument_variables,
            method.get_name(),
        );

        for (index, e) in local_variables.iter().enumerate() {
            pr_info!("Local Variables[{}]: {:?}", index, *e.lock().unwrap());
        }

        if let Err(e) = result {
            self.parse_helper.move_out_from_current_term_list()?;
            Err(e)
        } else if let Ok(Some(v)) = result {
            let return_value = match v {
                StatementOpcode::DefFatal(_) => Err(AmlError::InvalidOperation),
                StatementOpcode::DefReturn(return_value) => Ok(self
                    .get_aml_variable_from_term_arg(
                        return_value,
                        current_scope,
                        &mut local_variables,
                        &mut argument_variables,
                    )?
                    .get_constant_data()?),
                _ => Err(AmlError::InvalidOperation),
            };
            self.parse_helper.move_out_from_current_term_list()?;
            return_value
        } else {
            self.parse_helper.move_out_from_current_term_list()?;
            Ok(AmlVariable::Uninitialized)
        }
    }

    fn eval_method_with_method_invocation(
        &mut self,
        method_invocation: &MethodInvocation,
        method: &Method,
        original_local_variables: &mut Option<&mut LocalVariables>,
        original_argument_variables: &mut Option<&mut ArgumentVariables>,
    ) -> Result<AmlVariable, AmlError> {
        /* TODO: adjust parse_helper's term list */
        let (mut local_variables, mut argument_variables) =
            Self::init_local_variables_and_argument_variables();
        let mut default_argument_variables = if original_argument_variables.is_none() {
            Some(argument_variables.clone())
        } else {
            None
        };

        let current_scope = method.get_name();
        if method_invocation.get_ter_arg_list().list.len() != method.get_argument_count() {
            pr_err!(
                "Expected {} arguments, but found {} arguments.",
                method_invocation.get_ter_arg_list().list.len(),
                method.get_argument_count()
            );
            return Err(AmlError::InvalidOperation);
        } else if method_invocation.get_ter_arg_list().list.len()
            > Self::NUMBER_OF_ARGUMENT_VARIABLES
        {
            pr_err!(
                "Too many arguments: {:?}.",
                method_invocation.get_ter_arg_list().list
            );
            return Err(AmlError::InvalidOperation);
        }
        let mut index = 0;
        for arg in method_invocation.get_ter_arg_list().list.iter() {
            argument_variables[index] = Arc::new(Mutex::new(
                self.get_aml_variable_from_term_arg(
                    arg.clone(),
                    current_scope,
                    original_local_variables
                        .as_deref_mut()
                        .unwrap_or(&mut local_variables),
                    original_argument_variables
                        .as_deref_mut()
                        .unwrap_or_else(|| default_argument_variables.as_mut().unwrap()),
                )?,
            ));
            index += 1;
        }
        self.parse_helper
            .move_into_term_list(method.get_term_list().clone())?;
        let result = self._eval_term_list(
            method.get_term_list().clone(),
            &mut local_variables,
            &mut argument_variables,
            method.get_name(),
        );

        if let Err(e) = result {
            self.parse_helper.move_out_from_current_term_list()?;
            Err(e)
        } else if let Ok(Some(v)) = result {
            let return_value = match v {
                StatementOpcode::DefFatal(_) => Err(AmlError::InvalidOperation),
                StatementOpcode::DefReturn(return_value) => Ok(self
                    .get_aml_variable_from_term_arg(
                        return_value,
                        current_scope,
                        &mut local_variables,
                        &mut argument_variables,
                    )?),
                _ => Err(AmlError::InvalidOperation),
            };
            self.parse_helper.move_out_from_current_term_list()?;
            return_value
        } else {
            self.parse_helper.move_out_from_current_term_list()?;
            Ok(AmlVariable::Uninitialized)
        }
    }
}
