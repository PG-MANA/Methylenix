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
            if name.len() > 1 {
                let relative_name = name.get_element_as_name_string(name.len() - 1).unwrap();
                let sb_name = relative_name
                    .get_full_name_path(&NameString::from_array(&[[b'_', b'S', b'B', 0]], true));
                if &sb_name != name {
                    pr_info!("Temporary fix: Search {} instead.", sb_name);
                    return self.search_aml_variable(
                        &sb_name,
                        local_variables,
                        argument_variables,
                        current_scope,
                    );
                }
                pr_info!("Temporary fix: Search {} instead.", relative_name);
                return self.search_aml_variable(
                    &relative_name,
                    local_variables,
                    argument_variables,
                    current_scope,
                );
            }
            return Err(AmlError::InvalidOperation);
        }

        match object.unwrap() {
            ContentObject::NamedObject(n_o) => match n_o {
                NamedObject::DefBankField(_) => {
                    unimplemented!()
                }
                NamedObject::DefCreateField(f) => {
                    let source_variable = self.get_aml_variable_reference_from_term_arg(
                        f.get_source_buffer().clone(),
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    return if f.is_bit_field() {
                        let index = self
                            .eval_integer_expression(
                                f.get_index().clone(),
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
                                f.get_source_size_term_arg().as_ref().unwrap().clone(),
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
                                f.get_index().clone(),
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
                            operation_region.get_region_offset().clone(),
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .to_int()?;
                    let length = self
                        .eval_integer_expression(
                            operation_region.get_region_length().clone(),
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
                DataRefObject::DataObject(d) => {
                    let variable = Arc::new(Mutex::new(self.eval_term_arg(
                        TermArg::DataObject(d),
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?));
                    self.variables.push((name.clone(), variable.clone()));
                    Ok(variable)
                }
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

    fn get_aml_variable_reference_from_expression_opcode(
        &mut self,
        e: ExpressionOpcode,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
        current_scope: &NameString,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        let result = self.eval_expression(e, local_variables, argument_variables, current_scope)?;
        if let AmlVariable::Reference((source, None)) = result {
            Ok(source)
        } else if matches!(result, AmlVariable::Reference(_)) {
            Ok(Arc::new(Mutex::new(result)))
        } else {
            pr_info!("Expected a reference, but found {:?}.", result);
            Ok(Arc::new(Mutex::new(result)))
        }
    }

    fn get_aml_variable_reference_from_super_name(
        &mut self,
        super_name: &SuperName,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
        current_scope: &NameString,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        match super_name {
            SuperName::SimpleName(simple_name) => match simple_name {
                SimpleName::NameString(name) => {
                    self.get_aml_variable(&name, local_variables, argument_variables, current_scope)
                }
                SimpleName::ArgObj(c) => {
                    if *c as usize > Self::NUMBER_OF_ARGUMENT_VARIABLES {
                        pr_err!("Arg{} is out of index.", c);
                        Err(AmlError::InvalidOperation)
                    } else {
                        Ok(argument_variables[*c as usize].clone())
                    }
                }
                SimpleName::LocalObj(c) => {
                    if *c as usize > Self::NUMBER_OF_LOCAL_VARIABLES {
                        pr_err!("Local{} is out of index.", c);
                        Err(AmlError::InvalidOperation)
                    } else {
                        Ok(local_variables[*c as usize].clone())
                    }
                }
            },
            SuperName::DebugObj => {
                pr_info!("Using DebugObj");
                Err(AmlError::UnsupportedType)
            }
            SuperName::ReferenceTypeOpcode(r) => self
                .get_aml_variable_reference_from_expression_opcode(
                    ExpressionOpcode::ReferenceTypeOpcode((**r).clone()),
                    local_variables,
                    argument_variables,
                    current_scope,
                ),
        }
    }

    fn get_aml_variable_reference_from_term_arg(
        &mut self,
        term_arg: TermArg,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
        current_scope: &NameString,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        match term_arg {
            TermArg::ExpressionOpcode(e) => self.get_aml_variable_reference_from_expression_opcode(
                *e,
                local_variables,
                argument_variables,
                current_scope,
            ),
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
                        AmlVariable::Buffer(self.byte_list_to_vec(
                            byte_list,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?),
                    ))),
                },
                DataObject::DefPackage(p) => Ok(Arc::new(Mutex::new(AmlVariable::Package(
                    self.eval_package(p, local_variables, argument_variables, current_scope)?,
                )))),
                DataObject::DefVarPackage(p) => Ok(Arc::new(Mutex::new(AmlVariable::Package(
                    self.eval_var_package(p, local_variables, argument_variables, current_scope)?,
                )))),
            },
            TermArg::ArgObj(c) => Ok(argument_variables[c as usize].clone()),
            TermArg::LocalObj(c) => Ok(local_variables[c as usize].clone()),
        }
    }

    fn eval_package_list(
        &mut self,
        mut p: Package,
        num: usize,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
        current_scope: &NameString,
    ) -> Result<Vec<AmlPackage>, AmlError> {
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
                                    v.push(AmlPackage::Buffer(self.byte_list_to_vec(
                                        byte_list,
                                        local_variables,
                                        argument_variables,
                                        current_scope,
                                    )?));
                                }
                            },
                            DataObject::DefPackage(package) => {
                                v.push(AmlPackage::Package(self.eval_package(
                                    package,
                                    local_variables,
                                    argument_variables,
                                    current_scope,
                                )?));
                            }
                            DataObject::DefVarPackage(var_package) => {
                                v.push(AmlPackage::Package(self.eval_var_package(
                                    var_package,
                                    local_variables,
                                    argument_variables,
                                    current_scope,
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

    fn eval_package(
        &mut self,
        p: Package,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
        current_scope: &NameString,
    ) -> Result<Vec<AmlPackage>, AmlError> {
        let num = p.get_number_of_remaining_elements();
        self.eval_package_list(p, num, local_variables, argument_variables, current_scope)
    }

    fn eval_var_package(
        &mut self,
        mut p: VarPackage,
        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
        current_scope: &NameString,
    ) -> Result<Vec<AmlPackage>, AmlError> {
        let number_of_elements_term =
            p.get_number_of_elements(&mut self.parse_helper, current_scope)?;
        let number_of_elements = self
            .eval_integer_expression(
                number_of_elements_term,
                local_variables,
                argument_variables,
                current_scope,
            )?
            .to_int()?;
        self.eval_package_list(
            p.convert_to_package(number_of_elements),
            number_of_elements,
            local_variables,
            argument_variables,
            current_scope,
        )
    }

    fn byte_list_to_vec(
        &mut self,
        mut byte_list: ByteList,

        local_variables: &mut LocalVariables,
        argument_variables: &mut LocalVariables,
        current_scope: &NameString,
    ) -> Result<Vec<u8>, AmlError> {
        let buffer_size_term_arg = byte_list.get_buffer_size(&mut self.parse_helper)?;
        let buffer_size = self
            .eval_integer_expression(
                buffer_size_term_arg,
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
                        argument_variables[*l as usize]
                            .try_lock()
                            .or(Err(AmlError::MutexError))?
                            .write(data)?;
                    }
                    SimpleName::LocalObj(l) => {
                        if (*l as usize) > Self::NUMBER_OF_LOCAL_VARIABLES {
                            pr_err!("Writing LocalObj({}) is invalid.", l);
                            return Err(AmlError::InvalidOperation);
                        }
                        local_variables[*l as usize]
                            .try_lock()
                            .or(Err(AmlError::MutexError))?
                            .write(data)?;
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
                        self.eval_term_arg(
                            reference.clone(),
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?
                        .write(data)?;
                    }
                    ReferenceTypeOpcode::DefIndex(i) => {
                        let buffer = self.get_aml_variable_reference_from_term_arg(
                            i.get_source().clone(),
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?;
                        let index = self
                            .eval_term_arg(
                                i.get_index().clone(),
                                local_variables,
                                argument_variables,
                                current_scope,
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
        e: TermArg,
        local_variables: &mut LocalVariables,
        argument_variables: &mut ArgumentVariables,
        current_scope: &NameString,
    ) -> Result<bool, AmlError> {
        let data = self.eval_term_arg(e, local_variables, argument_variables, current_scope)?;
        if let Ok(boolean) = data.to_int() {
            Ok(boolean != 0)
        } else {
            pr_err!("Expected Boolean, but found {:?}.", data);
            Err(AmlError::InvalidType)
        }
    }

    fn eval_integer_expression(
        &mut self,
        e: TermArg,
        local_variables: &mut LocalVariables,
        argument_variables: &mut ArgumentVariables,
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        let data = self.eval_term_arg(e, local_variables, argument_variables, current_scope)?;
        if data.to_int().is_ok() {
            Ok(data)
        } else {
            pr_err!("Expected Integer, but found {:?}.", data);
            Err(AmlError::InvalidType)
        }
    }

    fn eval_term_arg(
        &mut self,
        t: TermArg,
        local_variables: &mut LocalVariables,
        argument_variables: &mut ArgumentVariables,
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        match t {
            TermArg::ExpressionOpcode(e) => {
                self.eval_expression(*e, local_variables, argument_variables, current_scope)
            }
            TermArg::DataObject(d) => match d {
                DataObject::ComputationalData(c_d) => match c_d {
                    ComputationalData::ConstData(c) => Ok(AmlVariable::ConstData(c)),
                    ComputationalData::ConstObj(c) => {
                        Ok(AmlVariable::ConstData(ConstData::Byte(c)))
                    }
                    ComputationalData::Revision => Ok(AmlVariable::ConstData(ConstData::Byte(
                        Self::AML_EVALUATOR_REVISION,
                    ))),
                    ComputationalData::StringData(s) => Ok(AmlVariable::String(String::from(s))),
                    ComputationalData::DefBuffer(byte_list) => {
                        Ok(AmlVariable::Buffer(self.byte_list_to_vec(
                            byte_list,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?))
                    }
                },
                DataObject::DefPackage(p) => Ok(AmlVariable::Package(self.eval_package(
                    p,
                    local_variables,
                    argument_variables,
                    current_scope,
                )?)),
                DataObject::DefVarPackage(v_p) => Ok(AmlVariable::Package(self.eval_var_package(
                    v_p,
                    local_variables,
                    argument_variables,
                    current_scope,
                )?)),
            },
            TermArg::ArgObj(c) => {
                if c as usize > Self::NUMBER_OF_ARGUMENT_VARIABLES {
                    pr_err!("Arg{} is out of index.", c);
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok((*argument_variables[c as usize])
                        .try_lock()
                        .or(Err(AmlError::MutexError))?
                        .clone())
                }
            }
            TermArg::LocalObj(c) => {
                if c as usize > Self::NUMBER_OF_LOCAL_VARIABLES {
                    pr_err!("Local{} is out of index.", c);
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok((*local_variables[c as usize])
                        .try_lock()
                        .or(Err(AmlError::MutexError))?
                        .clone())
                }
            }
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
                Ok(AmlVariable::Buffer(self.byte_list_to_vec(
                    byte_list,
                    local_variables,
                    argument_variables,
                    current_scope,
                )?))
            }
            ExpressionOpcode::DefPackage(p) => Ok(AmlVariable::Package(self.eval_package(
                p,
                local_variables,
                argument_variables,
                current_scope,
            )?)),
            ExpressionOpcode::DefVarPackage(var_package) => {
                Ok(AmlVariable::Package(self.eval_var_package(
                    var_package,
                    local_variables,
                    argument_variables,
                    current_scope,
                )?))
            }
            ExpressionOpcode::DefProcessor => {
                pr_err!("DefProcessor was deleted from ACPI 6.4.");
                Err(AmlError::InvalidOperation)
            }
            ExpressionOpcode::DefConcat(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefConcatRes(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefCopyObject(_, _) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::BinaryOperation(b_o) => {
                let left = self.eval_integer_expression(
                    b_o.get_left_operand().clone(),
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                let right = self.eval_integer_expression(
                    b_o.get_right_operand().clone(),
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
            ExpressionOpcode::DefDecrement(decrement) => {
                let obj = self.get_aml_variable_reference_from_super_name(
                    &decrement,
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                let mut locked_obj = obj.try_lock().or(Err(AmlError::MutexError))?;
                if locked_obj.is_constant_data() {
                    if let AmlVariable::ConstData(c) = *locked_obj {
                        let result = AmlVariable::ConstData(ConstData::from_usize(
                            c.to_int().overflowing_sub(1).0,
                            c.get_byte_size(),
                        )?);
                        locked_obj.write(result.clone())?;
                        Ok(result)
                    } else {
                        pr_err!("Expected Integer, but found {:?}", obj);
                        Err(AmlError::InvalidOperation)
                    }
                } else {
                    let constant_data = locked_obj.get_constant_data()?;
                    if let AmlVariable::ConstData(c) = constant_data {
                        let result = AmlVariable::ConstData(ConstData::from_usize(
                            c.to_int().overflowing_sub(1).0,
                            c.get_byte_size(),
                        )?);
                        locked_obj.write(result.clone())?;
                        Ok(result)
                    } else {
                        pr_err!("Expected Integer, but found {:?}", constant_data);
                        Err(AmlError::InvalidOperation)
                    }
                }
            }

            ExpressionOpcode::DefIncrement(increment) => {
                let obj = self.get_aml_variable_reference_from_super_name(
                    &increment,
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                let mut locked_obj = obj.try_lock().or(Err(AmlError::MutexError))?;
                if locked_obj.is_constant_data() {
                    if let AmlVariable::ConstData(c) = *locked_obj {
                        let result = AmlVariable::ConstData(ConstData::from_usize(
                            c.to_int().overflowing_add(1).0,
                            c.get_byte_size(),
                        )?);
                        locked_obj.write(result.clone())?;
                        Ok(result)
                    } else {
                        pr_err!("Expected Integer, but found {:?}", obj);
                        Err(AmlError::InvalidOperation)
                    }
                } else {
                    let constant_data = locked_obj.get_constant_data()?;
                    if let AmlVariable::ConstData(c) = constant_data {
                        let result = AmlVariable::ConstData(ConstData::from_usize(
                            c.to_int().overflowing_add(1).0,
                            c.get_byte_size(),
                        )?);
                        locked_obj.write(result.clone())?;
                        Ok(result)
                    } else {
                        pr_err!("Expected Integer, but found {:?}", constant_data);
                        Err(AmlError::InvalidOperation)
                    }
                }
            }
            ExpressionOpcode::DefDivide(divide) => {
                let dividend = self.eval_integer_expression(
                    divide.get_dividend().clone(),
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                let divisor = self.eval_integer_expression(
                    divide.get_divisor().clone(),
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                let dividend_data = dividend.to_int()?;
                let divisor_data = divisor.to_int()?;
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
                let operand_data:usize /* To detect error when changed the return type of to_int() */
                    = self.eval_integer_expression(operand,local_variables,argument_variables,current_scope)?.to_int()?;
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
                let operand_data:usize /* To detect error when changed the return type of to_int() */
                    = self.eval_integer_expression(operand,local_variables,argument_variables,current_scope)?.to_int()?;
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
            ExpressionOpcode::DefFromBCD(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefMatch(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefNot((operand, target)) => {
                let op = self.eval_integer_expression(
                    operand,
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
            ExpressionOpcode::DefObjectType(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefSizeOf(obj_name) => {
                let obj = self.get_aml_variable_reference_from_super_name(
                    &obj_name,
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
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
                    data,
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                self.write_data_into_target(
                    data.clone(),
                    &Target::SuperName(destination.clone()),
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                Ok(data)
            }
            ExpressionOpcode::DefToBCD(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefToInteger((operand, target)) => {
                let obj = self.eval_term_arg(
                    operand,
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                let constant_data = if obj.is_constant_data() {
                    obj
                } else {
                    obj.get_constant_data()?
                };
                let result = match constant_data {
                    AmlVariable::Uninitialized => Err(AmlError::InvalidOperation)?,
                    AmlVariable::ConstData(c) => c,
                    AmlVariable::String(s) => {
                        ConstData::QWord(parse_integer_from_buffer(s.as_bytes())? as _)
                    }
                    AmlVariable::Buffer(b) => {
                        if b.len() < 8 {
                            pr_err!("Invalid Buffer Size: {:#X}.", b.len());
                            Err(AmlError::InvalidOperation)?
                        } else {
                            let mut result = 0u64;
                            for index in 0..8 {
                                result |= (b[index] as u64) << index;
                            }
                            ConstData::QWord(result)
                        }
                    }
                    _ => Err(AmlError::UnsupportedType)?,
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
            ExpressionOpcode::DefTimer => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefCondRefOf((source, destination)) => {
                let result = self.get_aml_variable_reference_from_super_name(
                    &source,
                    local_variables,
                    argument_variables,
                    current_scope,
                );
                if matches!(result, Err(AmlError::InvalidMethodName(_))) {
                    Ok(AmlVariable::ConstData(ConstData::Byte(0)))
                } else if let Ok(obj) = result {
                    if !destination.is_null() {
                        self.write_data_into_target(
                            AmlVariable::Reference((obj, None)),
                            &destination,
                            local_variables,
                            argument_variables,
                            current_scope,
                        )?;
                    }
                    Ok(AmlVariable::ConstData(ConstData::Byte(1)))
                } else {
                    Err(result.unwrap_err())
                }
            }
            ExpressionOpcode::DefLAnd((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self.eval_bool_expression(
                        left,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )? && self.eval_bool_expression(
                        right,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?) as u8,
                )))
            }
            ExpressionOpcode::DefLEqual((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
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
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLGreater((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
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
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLGreaterEqual((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
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
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLLess((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
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
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLLessEqual((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
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
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLNot(source) => Ok(AmlVariable::ConstData(ConstData::Byte(
                (self
                    .eval_integer_expression(
                        source,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?
                    .to_int()?
                    == 0) as u8,
            ))),
            ExpressionOpcode::DefLNotEqual((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
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
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLoad(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefLoadTable(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefLOr((left, right)) => Ok(AmlVariable::ConstData(ConstData::Byte(
                (self.eval_bool_expression(
                    left,
                    local_variables,
                    argument_variables,
                    current_scope,
                )? || self.eval_bool_expression(
                    right,
                    local_variables,
                    argument_variables,
                    current_scope,
                )?) as u8,
            ))),
            ExpressionOpcode::DefWait(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::ReferenceTypeOpcode(r_e) => match r_e {
                ReferenceTypeOpcode::DefRefOf(super_name) => Ok(AmlVariable::Reference((
                    self.get_aml_variable_reference_from_super_name(
                        &super_name,
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?,
                    None,
                ))),
                ReferenceTypeOpcode::DefDerefOf(reference) => self.eval_term_arg(
                    reference.clone(),
                    local_variables,
                    argument_variables,
                    current_scope,
                ),
                ReferenceTypeOpcode::DefIndex(i) => {
                    let buffer = self.get_aml_variable_reference_from_term_arg(
                        i.get_source().clone(),
                        local_variables,
                        argument_variables,
                        current_scope,
                    )?;
                    let index = self
                        .eval_term_arg(
                            i.get_index().clone(),
                            local_variables,
                            argument_variables,
                            current_scope,
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
            ExpressionOpcode::DefMid(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefToBuffer(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefToDecimalString(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefToHexString(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::DefToString(_) => Err(AmlError::UnsupportedType),
            ExpressionOpcode::MethodInvocation(method_invocation) => {
                let obj = self.get_aml_variable(
                    method_invocation.get_name(),
                    local_variables,
                    argument_variables,
                    current_scope,
                )?;
                let locked_obj = &*obj.try_lock().or(Err(AmlError::MutexError))?;
                match locked_obj {
                    AmlVariable::Method(method) => Ok(self.eval_method_with_method_invocation(
                        &method_invocation,
                        method,
                        &mut Some(local_variables),
                        &mut Some(argument_variables),
                        current_scope,
                    )?),

                    _ => Ok(AmlVariable::Reference((obj, None))),
                }
            }
        }
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
            predicate.clone(),
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
            if !self.eval_bool_expression(
                predicate.clone(),
                local_variables,
                argument_variables,
                current_scope,
            )? {
                self.parse_helper.move_out_from_current_term_list()?;
                return Ok(None);
            }

            match self._eval_term_list(
                term_list.clone(),
                local_variables,
                argument_variables,
                current_scope,
            ) {
                Ok(None) | Ok(Some(StatementOpcode::DefContinue)) => { /* Continue */ }
                Ok(Some(StatementOpcode::DefBreak)) => {
                    self.parse_helper.move_out_from_current_term_list()?;
                    return Ok(None);
                }
                d => {
                    self.parse_helper.move_out_from_current_term_list()?;
                    return d;
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

    pub fn eval_method(
        &mut self,
        method: &Method,
        arguments: &[AmlVariable],
    ) -> Result<AmlVariable, AmlError> {
        let (mut local_variables, mut argument_variables) =
            Self::init_local_variables_and_argument_variables();
        let current_scope = method.get_name();
        if method.get_argument_count() != arguments.len() {
            pr_err!(
                "Expected {} arguments, but found {}.",
                method.get_argument_count(),
                arguments.len()
            );
            return Err(AmlError::InvalidOperation);
        }
        for (index, arg) in arguments.iter().enumerate() {
            argument_variables[index] = Arc::new(Mutex::new(arg.clone()));
        }
        self.parse_helper
            .move_into_term_list(method.get_term_list().clone())?;
        let result = self._eval_term_list(
            method.get_term_list().clone(),
            &mut local_variables,
            &mut argument_variables,
            current_scope,
        );

        self.parse_helper.move_out_from_current_term_list()?;
        if let Err(e) = result {
            Err(e)
        } else if let Ok(Some(v)) = result {
            let return_value = match v {
                StatementOpcode::DefFatal(_) => Err(AmlError::InvalidOperation),
                StatementOpcode::DefReturn(return_value) => {
                    let val = self.eval_term_arg(
                        return_value,
                        &mut local_variables,
                        &mut argument_variables,
                        current_scope,
                    )?;
                    if val.is_constant_data() {
                        Ok(val)
                    } else {
                        Ok(val.get_constant_data()?)
                    }
                }
                _ => Err(AmlError::InvalidOperation),
            };
            return_value
        } else {
            Ok(AmlVariable::Uninitialized)
        }
    }

    fn eval_method_with_method_invocation(
        &mut self,
        method_invocation: &MethodInvocation,
        method: &Method,
        original_local_variables: &mut Option<&mut LocalVariables>,
        original_argument_variables: &mut Option<&mut ArgumentVariables>,
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        /* TODO: adjust parse_helper's term list */
        let (mut local_variables, mut argument_variables) =
            Self::init_local_variables_and_argument_variables();
        let mut default_argument_variables = if original_argument_variables.is_none() {
            Some(argument_variables.clone())
        } else {
            None
        };
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
                self.eval_term_arg(
                    arg.clone(),
                    original_local_variables
                        .as_deref_mut()
                        .unwrap_or(&mut local_variables),
                    original_argument_variables
                        .as_deref_mut()
                        .unwrap_or_else(|| default_argument_variables.as_mut().unwrap()),
                    current_scope,
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
                StatementOpcode::DefReturn(return_value) => Ok(self.eval_term_arg(
                    return_value,
                    &mut local_variables,
                    &mut argument_variables,
                    method.get_name(),
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
