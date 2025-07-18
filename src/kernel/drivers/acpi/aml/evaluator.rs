//!
//! AML Evaluator
//!

use super::aml_variable::{
    AmlBitFiled, AmlByteFiled, AmlFunction, AmlIndexField, AmlPackage, AmlPciConfig, AmlVariable,
};
use super::data_object::{
    ComputationalData, ConstData, DataObject, PackageElement, parse_integer_from_buffer,
};
use super::expression_opcode::{
    ByteList, ExpressionOpcode, Package, ReferenceTypeOpcode, VarPackage,
};
use super::name_object::{NameString, SimpleName, SuperName, Target};
use super::named_object::{Device, Field, FieldElement, Method, NamedObject, OperationRegionType};
use super::namespace_modifier_object::NamespaceModifierObject;
use super::statement_opcode::{Fatal, IfElse, Notify, StatementOpcode, While};
use super::term_object::{MethodInvocation, TermArg, TermList, TermObj};
use super::variable_tree::AmlVariableTree;
use super::{ACPI_INT_ONES, AcpiInt, AmlError, DataRefObject, eisa_id_to_dword};

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::sync::spin_lock::Mutex;

use core::sync::atomic::{AtomicU8, Ordering};

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

type LocalVariables = [Arc<Mutex<AmlVariable>>; Evaluator::NUMBER_OF_LOCAL_VARIABLES];
type ArgumentVariables = [Arc<Mutex<AmlVariable>>; Evaluator::NUMBER_OF_ARGUMENT_VARIABLES];

#[derive(Clone)]
pub struct Evaluator {
    current_root_term_list: TermList,
    root_term_list: Arc<Vec<TermList>>,
    /* For SSDT */
    variable_tree: AmlVariableTree,
    original_searching_name: Option<NameString>,
    term_list_hierarchy: Vec<TermList>,
    current_local_variables: LocalVariables,
    current_argument_variables: ArgumentVariables,
}

impl Evaluator {
    const NUMBER_OF_LOCAL_VARIABLES: usize = 7;
    const NUMBER_OF_ARGUMENT_VARIABLES: usize = 7;
    const AML_EVALUATOR_REVISION: u8 = 0;

    pub fn new(current_root_term_list: TermList, root_term_list: Vec<TermList>) -> Self {
        assert_eq!(current_root_term_list.get_scope_name(), &NameString::root());
        let (local, arguments) = Self::init_local_variables_and_argument_variables();
        Self {
            current_root_term_list,
            root_term_list: Arc::new(root_term_list),
            variable_tree: AmlVariableTree::create_tree(),
            original_searching_name: None,
            term_list_hierarchy: Vec::new(),
            current_local_variables: local,
            current_argument_variables: arguments,
        }
    }

    pub fn init(&mut self, osi_function: AmlFunction) -> Result<(), AmlError> {
        if !self.variable_tree.get_current_scope_name().is_root() {
            self.variable_tree.move_to_root()?;
        }
        /* Add builtin objects */
        const GL_NAME: NameString = NameString::from_array_const(&[*b"_GL\0"], true);
        let gl = AmlVariable::Mutex(Arc::new((AtomicU8::new(0), 0)));
        self.variable_tree.add_data(GL_NAME, gl, true)?;

        const OSI_NAME: NameString = NameString::from_array_const(&[*b"_OSI"], true);
        let osi = AmlVariable::BuiltInMethod((osi_function, 1));
        self.variable_tree.add_data(OSI_NAME, osi, true)?;

        const OS_NAME: NameString = NameString::from_array_const(&[*b"_OS\0"], true);
        let os = AmlVariable::String(String::from(crate::OS_NAME));
        self.variable_tree.add_data(OS_NAME, os, true)?;

        const REV_NAME: NameString = NameString::from_array_const(&[*b"_REV"], true);
        let rev = AmlVariable::ConstData(ConstData::Byte(2 /* ACPI 2.0 */));
        self.variable_tree.add_data(REV_NAME, rev, true)?;

        const DLM_NAME: NameString = NameString::from_array_const(&[*b"_DLM"], true);
        let dlm = AmlVariable::ConstData(ConstData::Byte(0 /* Temporary fix */));
        self.variable_tree.add_data(DLM_NAME, dlm, true)?;

        Ok(())
    }

    fn evaluate_sta_and_ini_in_device(&mut self, device: Device) -> Result<(), AmlError> {
        const STA_PRESENT_BIT: AcpiInt = 1;
        const STA_FUNCTIONAL_BIT: AcpiInt = 1 << 3;
        const STA_BASE_NAME: NameString = NameString::from_array_const(&[*b"_STA"], false);
        const INI_BASE_NAME: NameString = NameString::from_array_const(&[*b"_INI"], false);

        let sta = STA_BASE_NAME.get_full_name_path(device.get_name(), true);
        let status = match self.search_aml_variable(&sta, None, false) {
            Ok(v) => {
                let locked_sta_object = v.lock().unwrap();
                match &*locked_sta_object {
                    AmlVariable::ConstData(c) => {
                        let r = c.to_int();
                        drop(locked_sta_object);
                        r
                    }
                    AmlVariable::Method(m) => {
                        let cloned_method = m.clone();
                        drop(locked_sta_object);
                        match self.eval_method_in_current_status(&cloned_method, &[]) {
                            Ok(v) => match v.to_int() {
                                Ok(s) => s,
                                Err(e) => {
                                    pr_err!("Expected an integer, but found {:?}({:?})", v, e);
                                    STA_FUNCTIONAL_BIT
                                }
                            },
                            Err(e) => {
                                pr_err!(
                                    "Failed to evaluate {}: {:?}, skip this device and children.",
                                    cloned_method.get_name(),
                                    e
                                );
                                0
                            }
                        }
                    }
                    _ => {
                        pr_err!("Expected a method, but found {:?}", &*locked_sta_object);
                        locked_sta_object.to_int().unwrap_or_else(|e| {
                            pr_err!(
                                "Failed to convert to an integer: {:?}, skip this device.",
                                e
                            );
                            STA_FUNCTIONAL_BIT
                        })
                    }
                }
            }
            Err(AmlError::InvalidName(n)) => {
                if n == sta {
                    0b1111 /* Assume enabled */
                } else {
                    pr_err!("Failed to search {}: {:?}", sta, AmlError::InvalidName(n));
                    0
                }
            }
            Err(e) => {
                pr_err!("Failed to search {}: {:?}", sta, e);
                0
            }
        };

        let present_bit = (status & STA_PRESENT_BIT) != 0;
        let functional_bit = (status & STA_FUNCTIONAL_BIT) != 0;

        if !present_bit && !functional_bit {
            /* Skip this device and children. */
            return Ok(());
        }
        if present_bit {
            let ini = INI_BASE_NAME.get_full_name_path(device.get_name(), true);
            match self.search_aml_variable(&ini, None, false) {
                Ok(v) => {
                    let locked_ini_object = v.lock().unwrap();
                    match &*locked_ini_object {
                        AmlVariable::Method(m) => {
                            let cloned_method = m.clone();
                            drop(locked_ini_object);
                            if let Err(e) = self.eval_method_in_current_status(&cloned_method, &[])
                            {
                                pr_err!("Failed to evaluate {}: {:?}", cloned_method.get_name(), e);
                            }
                        }
                        _ => {
                            pr_err!("Expected a method, but found {:?}", &*locked_ini_object);
                        }
                    }
                }
                Err(AmlError::InvalidName(n)) => {
                    if n != ini {
                        pr_err!("Failed to search {}.", n);
                    }
                }
                Err(e) => {
                    pr_err!("Failed to search {}: {:?}", ini, e);
                }
            };
        }
        self.walk_all_devices(device.get_term_list().clone())
    }

    fn walk_all_devices(&mut self, mut term_list: TermList) -> Result<(), AmlError> {
        while let Some(obj) = term_list.next(self)? {
            match obj {
                TermObj::NamespaceModifierObj(n_m) => {
                    match n_m {
                        NamespaceModifierObject::DefScope(s) => {
                            self.term_list_hierarchy.push(s.get_term_list().clone());
                            let tree_backup = self.variable_tree.backup_current_scope();
                            self.variable_tree.move_current_scope(s.get_name())?;
                            self.walk_all_devices(s.get_term_list().clone())?;
                            self.variable_tree.restore_current_scope(tree_backup);
                            self.term_list_hierarchy.pop();
                        }
                        _ => { /* Ignore */ }
                    }
                }
                TermObj::NamedObj(n_o) => match n_o {
                    NamedObject::DefDevice(d) => {
                        self.term_list_hierarchy.push(d.get_term_list().clone());
                        let tree_backup = self.variable_tree.backup_current_scope();
                        self.variable_tree
                            .move_current_scope(d.get_term_list().get_scope_name())?;
                        self.evaluate_sta_and_ini_in_device(d)?;
                        self.variable_tree.restore_current_scope(tree_backup);
                        self.term_list_hierarchy.pop();
                    }
                    o if matches!(o, NamedObject::DefDataRegion(_))
                        || matches!(o, NamedObject::DefPowerRes(_))
                        || matches!(o, NamedObject::DefThermalZone(_)) =>
                    {
                        let t = o.get_term_list().unwrap();
                        self.term_list_hierarchy.push(t.clone());
                        let tree_backup = self.variable_tree.backup_current_scope();
                        self.variable_tree.move_current_scope(t.get_scope_name())?;
                        self.walk_all_devices(t)?;
                        self.term_list_hierarchy.pop();
                        self.variable_tree.restore_current_scope(tree_backup);
                    }
                    _ => { /* Ignore */ }
                },
                TermObj::StatementOpcode(s_o) => {
                    if let StatementOpcode::DefIfElse(_i_e) = s_o { /* Currently ignore it */ }
                }
                TermObj::ExpressionOpcode(_) => { /* Ignore */ }
            }
        }
        Ok(())
    }

    /// Initialize all devices by evaluating all _STA and _INI methods.
    pub fn initialize_all_devices(&mut self) -> Result<(), AmlError> {
        self.walk_all_devices(self.current_root_term_list.clone())?;

        let backup = self.current_root_term_list.clone();
        for r in self.root_term_list.clone().iter() {
            if r == &backup {
                continue;
            }
            self.current_root_term_list = r.clone();
            self.walk_all_devices(self.current_root_term_list.clone())?;
        }
        self.current_root_term_list = backup;
        Ok(())
    }

    pub(super) fn init_local_variables_and_argument_variables()
    -> (LocalVariables, ArgumentVariables) {
        /* TODO: use core::array::repeat when it was stabilized */
        use core::array::from_fn;
        let uninitialized_data = Arc::new(Mutex::new(AmlVariable::Uninitialized));
        (
            from_fn(|_| uninitialized_data.clone()),
            from_fn(|_| uninitialized_data.clone()),
        )
    }

    fn search_aml_variable_by_parsing_term_list(
        &mut self,
        name: &NameString,
        name_single_relative: Option<&NameString>,
        mut term_list: TermList,
        search_scope: Option<&NameString>, /* To search the variable like _SB.PCI0.^^_FOO */
        should_keep_term_list_hierarchy_when_found: bool,
    ) -> Result<Option<Arc<Mutex<AmlVariable>>>, AmlError> {
        if !term_list.get_scope_name().is_child(name)
            && search_scope
                .map(|s| !term_list.get_scope_name().is_child(s))
                .unwrap_or(true)
        {
            return Ok(None);
        }
        self.variable_tree
            .move_current_scope(term_list.get_scope_name())?;

        let is_in_search_scope = search_scope
            .map(|s| term_list.get_scope_name() == s || term_list.get_scope_name().is_child(s))
            .unwrap_or(true);
        let get_next_term_obj =
            |t: &mut TermList, p: &mut Self| -> Result<Option<TermObj>, AmlError> {
                match t.next(p) {
                    Ok(Some(o)) => Ok(Some(o)),
                    Ok(None) | Err(AmlError::NestedSearch) | Err(AmlError::AccessOutOfRange) => {
                        Ok(None)
                    }
                    Err(e) => Err(e),
                }
            };
        let compare_by_search_rules = |object_name: &NameString| -> bool {
            if name.is_absolute_path() {
                object_name == name
            } else if let Some(single_name_segment_path) = name_single_relative {
                /* Single Name Segments */
                is_in_search_scope && object_name.suffix_search(single_name_segment_path)
            } else {
                /* Multi Name Segments */
                /* Maybe wrong... */
                is_in_search_scope && object_name.suffix_search(name)
            }
        };

        while let Some(term_obj) = get_next_term_obj(&mut term_list, self)? {
            match term_obj {
                TermObj::NamespaceModifierObj(name_modifier_object) => {
                    match name_modifier_object {
                        NamespaceModifierObject::DefAlias(a) => {
                            if compare_by_search_rules(a.get_name()) {
                                pr_err!(
                                    "Alias is not supported yet. {} => {}",
                                    name,
                                    a.get_source()
                                );
                                return Err(AmlError::UnsupportedType);
                            }
                        }
                        NamespaceModifierObject::DefName(n) => {
                            if compare_by_search_rules(n.get_name()) {
                                return match n.get_data_ref_object() {
                                    DataRefObject::DataObject(d) => {
                                        let variable = self.eval_term_arg(
                                            TermArg::DataObject(d.clone()),
                                            term_list.get_scope_name(),
                                        )?;
                                        let variable = self.variable_tree.add_data(
                                            n.get_name().clone(),
                                            variable,
                                            false,
                                        )?;

                                        Ok(Some(variable))
                                    }
                                    DataRefObject::ObjectReference(d_r) => {
                                        pr_err!("Unsupported Type: DataReference({})", d_r);
                                        Err(AmlError::UnsupportedType)
                                    }
                                };
                            }
                        }
                        NamespaceModifierObject::DefScope(s) => {
                            if s.get_name() == name
                                || s.get_name().suffix_search(name)
                                || s.get_name().is_child(name)
                                || search_scope
                                    .map(|scope| {
                                        scope == s.get_name() || s.get_name().is_child(scope)
                                    })
                                    .unwrap_or(false)
                            {
                                let backup = self.variable_tree.backup_current_scope();
                                self.term_list_hierarchy.push(s.get_term_list().clone());

                                let result = self.search_aml_variable_by_parsing_term_list(
                                    name,
                                    name_single_relative,
                                    s.get_term_list().clone(),
                                    search_scope,
                                    should_keep_term_list_hierarchy_when_found,
                                );

                                match &result {
                                    Ok(Some(_)) => {
                                        if !should_keep_term_list_hierarchy_when_found {
                                            self.term_list_hierarchy.pop();
                                            self.variable_tree.restore_current_scope(backup);
                                        }
                                        return result;
                                    }
                                    Ok(None) | Err(AmlError::NestedSearch) => {
                                        self.term_list_hierarchy.pop();
                                        self.variable_tree.restore_current_scope(backup);
                                        /* Continue */
                                    }
                                    Err(_) => {
                                        self.term_list_hierarchy.pop();
                                        self.variable_tree.restore_current_scope(backup);
                                        return result;
                                    }
                                };
                            }
                        }
                    }
                }
                TermObj::NamedObj(named_object) => {
                    let tree_backup = self.variable_tree.backup_current_scope();
                    match self.search_aml_variable_by_parsing_named_object(
                        name,
                        name_single_relative,
                        term_list.get_scope_name(),
                        named_object,
                        search_scope,
                        should_keep_term_list_hierarchy_when_found,
                    ) {
                        Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
                        Ok(Some(v)) => return Ok(Some(v)),
                        o => {
                            self.variable_tree.restore_current_scope(tree_backup);
                            return o;
                        }
                    }
                    self.variable_tree.restore_current_scope(tree_backup);
                }
                TermObj::StatementOpcode(s) => {
                    if let StatementOpcode::DefIfElse(_i_e) = s {
                        /* Currently ignore it*/
                    } else {
                        /* Ignore */
                    }
                }
                TermObj::ExpressionOpcode(_) => { /* Ignore */ }
            }
        }
        Ok(None)
    }

    fn search_aml_variable_by_parsing_named_object(
        &mut self,
        name: &NameString,
        name_single_relative: Option<&NameString>,
        current_scope: &NameString,
        named_object: NamedObject,
        search_scope: Option<&NameString>, /* To search the variable like _SB.PCI0.^^_FOO */
        should_keep_term_list_hierarchy_when_found: bool,
    ) -> Result<Option<Arc<Mutex<AmlVariable>>>, AmlError> {
        let is_in_current_scope = current_scope.is_child(name);

        if let Some(named_object_name) = named_object.get_name() {
            if name == named_object_name
                || name_single_relative
                    .map(|n| is_in_current_scope && named_object_name.suffix_search(n))
                    .unwrap_or(false)
            {
                let v = self.eval_named_object(name, named_object, current_scope)?;
                return Ok(Some(self.variable_tree.add_data(name.clone(), v, false)?));
            }
        }
        if !name.is_single_relative_path_name()
            && !named_object
                .get_name()
                .unwrap_or(current_scope)
                .is_child(name)
        {
            return Ok(None);
        }

        if let Some(mut field_list) = named_object.get_field_list() {
            while let Some(e) = field_list.next()? {
                if let FieldElement::NameField((n, _)) = &e {
                    if n == name {
                        let v = self.eval_named_object(name, named_object, current_scope)?;
                        return Ok(Some(self.variable_tree.add_data(name.clone(), v, false)?));
                    } else if name_single_relative
                        .map(|relative_name| is_in_current_scope && n.suffix_search(relative_name))
                        .unwrap_or(false)
                    {
                        let v = self.eval_named_object(name, named_object, current_scope)?;
                        return Ok(Some(self.variable_tree.add_data(name.clone(), v, false)?));
                    }
                }
            }
            Ok(None)
        } else if let Some(term_list) = named_object.get_term_list() {
            let tree_backup = self.variable_tree.backup_current_scope();
            self.term_list_hierarchy.push(term_list.clone());
            self.variable_tree
                .move_current_scope(term_list.get_scope_name())?;
            let result = self.search_aml_variable_by_parsing_term_list(
                name,
                name_single_relative,
                term_list,
                search_scope,
                should_keep_term_list_hierarchy_when_found,
            );
            if !(matches!(result, Ok(Some(_))) && should_keep_term_list_hierarchy_when_found) {
                self.variable_tree.restore_current_scope(tree_backup);
                self.term_list_hierarchy.pop();
            }
            result
        } else {
            Ok(None)
        }
    }

    fn search_aml_variable_by_absolute_path(
        &mut self,
        name: &NameString,
    ) -> Result<Option<Arc<Mutex<AmlVariable>>>, AmlError> {
        if let Some(d) = self.variable_tree.find_data_from_root(name)? {
            return Ok(Some(d));
        }
        let tree_backup = self.variable_tree.backup_current_scope();
        let mut term_list_hierarchy_backup = Vec::new();
        core::mem::swap(
            &mut term_list_hierarchy_backup,
            &mut self.term_list_hierarchy,
        );
        self.variable_tree.move_to_root()?;
        let absolute_search = if name.is_absolute_path() {
            None
        } else {
            Some(name.to_be_absolute_path())
        };

        let result = self.search_aml_variable_by_parsing_term_list(
            absolute_search.as_ref().unwrap_or(name),
            None,
            self.current_root_term_list.clone(),
            None,
            false,
        );

        if matches!(&result, Ok(Some(_))) || result.is_err() {
            self.variable_tree.restore_current_scope(tree_backup);
            core::mem::swap(
                &mut term_list_hierarchy_backup,
                &mut self.term_list_hierarchy,
            );
            return result;
        }
        drop(result);
        self.term_list_hierarchy.clear();

        let current_term_list_backup = self.current_root_term_list.clone();
        for term_list in self.root_term_list.clone().iter() {
            self.current_root_term_list = term_list.clone();
            let result = self.search_aml_variable_by_parsing_term_list(
                name,
                None,
                self.current_root_term_list.clone(),
                None,
                false,
            );

            if matches!(&result, Ok(Some(_))) || result.is_err() {
                self.variable_tree.restore_current_scope(tree_backup);
                core::mem::swap(
                    &mut term_list_hierarchy_backup,
                    &mut self.term_list_hierarchy,
                );
                self.current_root_term_list = current_term_list_backup;
                return result;
            }
            self.term_list_hierarchy.clear();
        }
        self.variable_tree.restore_current_scope(tree_backup);
        self.current_root_term_list = current_term_list_backup;
        core::mem::swap(
            &mut term_list_hierarchy_backup,
            &mut self.term_list_hierarchy,
        );
        Ok(None)
    }

    /// Find Element with parsing Field and return the object including it.
    /// This function is the entrance of searching object.
    pub fn search_aml_variable(
        &mut self,
        name: &NameString,
        preferred_search_scope: Option<&NameString>,
        _allow_search_from_cache: bool,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        if name.is_null_name() {
            return Err(AmlError::InvalidName(name.clone()));
        }
        let back_up_of_original_name_searching =
            if let Some(searching) = self.original_searching_name.replace(name.clone()) {
                if name == &searching {
                    self.original_searching_name = Some(searching);
                    return Err(AmlError::NestedSearch);
                }
                Some(searching)
            } else {
                None
            };

        let tree_backup = self.variable_tree.backup_current_scope();

        let restore_status = |s: &mut Self| -> Result<(), AmlError> {
            s.original_searching_name = back_up_of_original_name_searching;
            s.variable_tree.restore_current_scope(tree_backup);
            Ok(())
        };

        /* Search from the Variable Tree */
        if let Some(relative_name) =
            name.get_relative_name(self.variable_tree.get_current_scope_name())
        {
            if let Some(v) = self
                .variable_tree
                .find_data_from_current_scope(&relative_name)?
            {
                restore_status(self)?;
                return Ok(v);
            }
        }
        if name.len() > 1
            && self
                .term_list_hierarchy
                .last()
                .map(|s| !s.get_scope_name().is_child(name))
                .unwrap_or(false)
            && preferred_search_scope.is_none()
        {
            if let Some(v) = self.search_aml_variable_by_absolute_path(name)? {
                restore_status(self)?;
                return Ok(v);
            }
        }
        let single_name = name.get_single_name_path();
        if let Some(s_n) = single_name.as_ref() {
            if let Some(v) = self.variable_tree.find_data_from_current_scope(s_n)? {
                restore_status(self)?;
                return Ok(v);
            }
        }

        /* Search from the current TermList */
        if let Some(current_term_list) = self.term_list_hierarchy.last().cloned() {
            if let Some(v) = self.search_aml_variable_by_parsing_term_list(
                name,
                single_name.as_ref(),
                current_term_list,
                None,
                false,
            )? {
                restore_status(self)?;
                return Ok(v);
            }
        }

        let search_scope = preferred_search_scope
            .unwrap_or_else(|| self.variable_tree.get_current_scope_name())
            .clone();
        /* Backup current status */
        let mut term_list_hierarchy_back_up: Vec<TermList> =
            Vec::with_capacity(self.term_list_hierarchy.len());
        let mut term_list_hierarchy_len = self.term_list_hierarchy.len(); /* For debug */
        let restore_status = |s: &mut Self,
                              mut term_list_hierarchy_back_up: Vec<TermList>|
         -> Result<(), AmlError> {
            restore_status(s)?;
            while let Some(t) = term_list_hierarchy_back_up.pop() {
                s.term_list_hierarchy.push(t);
            }
            Ok(())
        };

        if let Some(t) = self.term_list_hierarchy.pop() {
            term_list_hierarchy_len -= 1;
            term_list_hierarchy_back_up.push(t);
        }

        for index in (0..self.term_list_hierarchy.len()).rev() {
            let term_list = self.term_list_hierarchy.get(index).unwrap().clone();
            self.variable_tree
                .move_current_scope(term_list.get_scope_name())?;

            if !name.is_absolute_path() {
                if let Some(s_n) = single_name.as_ref() {
                    if let Some(v) = self.variable_tree.find_data_from_current_scope(s_n)? {
                        restore_status(self, term_list_hierarchy_back_up)?;
                        return Ok(v);
                    }
                } else if let Some(r_n) = name.get_relative_name(term_list.get_scope_name()) {
                    if let Some(v) = self.variable_tree.find_data_from_current_scope(&r_n)? {
                        restore_status(self, term_list_hierarchy_back_up)?;
                        return Ok(v);
                    }
                }
            }

            let search_target_name = single_name
                .as_ref()
                .map(|n| n.get_full_name_path(term_list.get_scope_name(), false))
                .unwrap_or_else(|| name.clone());

            match self.search_aml_variable_by_parsing_term_list(
                &search_target_name,
                single_name.as_ref(),
                term_list.clone(),
                Some(&search_scope),
                false,
            ) {
                Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
                Ok(Some(v)) => {
                    restore_status(self, term_list_hierarchy_back_up)?;
                    return Ok(v);
                }
                Err(e) => {
                    restore_status(self, term_list_hierarchy_back_up)?;

                    return Err(e);
                }
            }
            if let Some(t) = self.term_list_hierarchy.pop() {
                term_list_hierarchy_back_up.push(t)
            }

            term_list_hierarchy_len -= 1;
            if self.term_list_hierarchy.len() != term_list_hierarchy_len {
                pr_err!(
                    "Expected {} entries in term_list_hierarchy, but found {} entries: {:?}",
                    term_list_hierarchy_len,
                    self.term_list_hierarchy.len(),
                    self.term_list_hierarchy
                );
                return Err(AmlError::ObjectTreeError);
            }
        }

        /* Search from current root */
        assert_eq!(self.term_list_hierarchy.len(), 0);

        match self
            .variable_tree
            .find_data_from_root(single_name.as_ref().unwrap_or(name))
        {
            Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
            Ok(Some(v)) => {
                restore_status(self, term_list_hierarchy_back_up)?;
                return Ok(v);
            }
            Err(e) => {
                restore_status(self, term_list_hierarchy_back_up)?;
                return Err(e);
            }
        }

        /* TODO: check search algorithm */
        match self.search_aml_variable_by_parsing_term_list(
            name,
            single_name.as_ref(),
            self.current_root_term_list.clone(),
            Some(&search_scope),
            false,
        ) {
            Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
            Ok(Some(v)) => {
                restore_status(self, term_list_hierarchy_back_up)?;
                return Ok(v);
            }
            Err(e) => {
                restore_status(self, term_list_hierarchy_back_up)?;
                return Err(e);
            }
        }

        match self.search_aml_variable_by_parsing_term_list(
            single_name.as_ref().unwrap_or(name),
            single_name.as_ref(),
            self.current_root_term_list.clone(),
            Some(&search_scope),
            false,
        ) {
            Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
            Ok(Some(v)) => {
                restore_status(self, term_list_hierarchy_back_up)?;

                return Ok(v);
            }
            Err(e) => {
                restore_status(self, term_list_hierarchy_back_up)?;

                return Err(e);
            }
        }

        let current_term_list_back_up = self.current_root_term_list.clone();
        let restore_status =
            |s: &mut Self, current_term_list_back_up: TermList| -> Result<(), AmlError> {
                restore_status(s, term_list_hierarchy_back_up)?;
                s.current_root_term_list = current_term_list_back_up;
                Ok(())
            };

        /* Search from root_term_list including SSDT */
        for root_term_list in self.root_term_list.clone().iter() {
            if current_term_list_back_up == *root_term_list {
                continue;
            }
            self.current_root_term_list = root_term_list.clone();
            match self.search_aml_variable_by_parsing_term_list(
                name,
                single_name.as_ref(),
                self.current_root_term_list.clone(),
                Some(&search_scope),
                false,
            ) {
                Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
                Ok(Some(v)) => {
                    restore_status(self, current_term_list_back_up)?;
                    return Ok(v);
                }
                Err(e) => {
                    restore_status(self, current_term_list_back_up)?;
                    return Err(e);
                }
            }
        }

        restore_status(self, current_term_list_back_up)?;
        Err(AmlError::InvalidName(name.clone()))
    }

    fn move_into_object(
        &mut self,
        object_name: &NameString,
        search_scope: Option<&NameString>,
    ) -> Result<(), AmlError> {
        /* Search from the current root */
        if !self.term_list_hierarchy.is_empty() {
            pr_err!("TermListHierarchy is not empty, it will be deleted.");
            self.term_list_hierarchy.clear();
        }
        self.variable_tree.move_to_root()?;
        let single_name = object_name.get_single_name_path();

        match self.search_aml_variable_by_parsing_term_list(
            object_name,
            single_name.as_ref(),
            self.current_root_term_list.clone(),
            search_scope,
            true,
        ) {
            Ok(Some(_)) => return Ok(()),
            Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
            Err(e) => return Err(e),
        }

        let current_term_list_back_up = self.current_root_term_list.clone();

        /* Search from root_term_list including SSDT */
        for root_term_list in self.root_term_list.clone().iter() {
            if current_term_list_back_up == *root_term_list {
                continue;
            }
            self.current_root_term_list = root_term_list.clone();
            match self.search_aml_variable_by_parsing_term_list(
                object_name,
                single_name.as_ref(),
                self.current_root_term_list.clone(),
                search_scope,
                true,
            ) {
                Ok(Some(_)) => return Ok(()), /* Keep current_root_term_list */
                Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
                Err(e) => {
                    self.current_root_term_list = current_term_list_back_up;
                    return Err(e);
                }
            }
        }

        self.current_root_term_list = current_term_list_back_up;
        Err(AmlError::InvalidName(object_name.clone()))
    }

    fn _move_into_device(
        &mut self,
        hid: u32,
        mut term_list: TermList,
        in_device: bool,
    ) -> Result<bool, AmlError> {
        while let Some(obj) = term_list.next(self)? {
            match obj {
                TermObj::NamespaceModifierObj(n_m) => {
                    match n_m {
                        NamespaceModifierObject::DefScope(s) => {
                            self.term_list_hierarchy.push(s.get_term_list().clone());
                            let tree_backup = self.variable_tree.backup_current_scope();
                            self.variable_tree.move_current_scope(s.get_name())?;
                            if self._move_into_device(hid, s.get_term_list().clone(), in_device)? {
                                return Ok(true);
                            }
                            self.variable_tree.restore_current_scope(tree_backup);
                            self.term_list_hierarchy.pop();
                        }
                        NamespaceModifierObject::DefName(n) => {
                            if in_device {
                                let hid_name = NameString::from_array(&[*b"_HID"], false)
                                    .get_full_name_path(term_list.get_scope_name(), true);
                                if n.get_name() == &hid_name {
                                    if let DataRefObject::DataObject(
                                        DataObject::ComputationalData(
                                            ComputationalData::ConstData(d),
                                        ),
                                    ) = n.get_data_ref_object()
                                    {
                                        if d.to_int() == hid as AcpiInt {
                                            return Ok(true);
                                        }
                                    }
                                }
                            }
                        }
                        _ => { /* Ignore */ }
                    }
                }
                TermObj::NamedObj(n_o) => match n_o {
                    NamedObject::DefDevice(d) => {
                        self.term_list_hierarchy.push(d.get_term_list().clone());
                        let tree_backup = self.variable_tree.backup_current_scope();
                        self.variable_tree.move_current_scope(d.get_name())?;
                        if self._move_into_device(hid, d.get_term_list().clone(), true)? {
                            return Ok(true);
                        }
                        self.variable_tree.restore_current_scope(tree_backup);
                        self.term_list_hierarchy.pop();
                    }
                    NamedObject::DefMethod(_) => { /* Ignore */ }
                    o => {
                        if let Some(term_list) = o.get_term_list() {
                            self.term_list_hierarchy.push(term_list.clone());
                            let tree_backup = self.variable_tree.backup_current_scope();
                            self.variable_tree
                                .move_current_scope(term_list.get_scope_name())?;
                            if self._move_into_device(hid, term_list, in_device)? {
                                return Ok(true);
                            }
                            self.variable_tree.restore_current_scope(tree_backup);
                            self.term_list_hierarchy.pop();
                        }
                    }
                },
                TermObj::StatementOpcode(_s_o) => {
                    /* if let StatementOpcode::DefIfElse(i_e) = s_o {
                        pr_warn!(
                            "Found IfElse Statement out of a method, currently ignore it: {:?}",
                            i_e
                        );
                    } */
                }
                TermObj::ExpressionOpcode(_) => { /* Ignore */ }
            }
        }
        Ok(false)
    }

    pub fn move_into_device(&mut self, hid: &[u8; 7]) -> Result<bool, AmlError> {
        if !self.term_list_hierarchy.is_empty() {
            pr_err!("TermListHierarchy is not empty, it will be deleted.");
            self.term_list_hierarchy.clear();
        }
        self.variable_tree.move_to_root()?;
        let hid_u32 = eisa_id_to_dword(hid);
        if self._move_into_device(hid_u32, self.current_root_term_list.clone(), false)? {
            return Ok(true);
        }

        let backup = self.current_root_term_list.clone();
        for r in self.root_term_list.clone().iter() {
            if r == &backup {
                continue;
            }
            self.current_root_term_list = r.clone();
            if self._move_into_device(hid_u32, r.clone(), false)? {
                self.current_root_term_list = backup;
                return Ok(true);
            }
        }
        self.current_root_term_list = backup;
        Ok(false)
    }

    pub fn find_method_argument_count(
        &mut self,
        method_name: &NameString,
    ) -> Result<AcpiInt, AmlError> {
        if method_name.is_null_name() {
            return Ok(0);
        }
        let v = self.search_aml_variable(method_name, None, true)?;
        Ok(match &*v.lock().unwrap() {
            AmlVariable::Method(m) => m.get_argument_count(),
            AmlVariable::BuiltInMethod((_, c)) => *c as AcpiInt,
            _ => 0,
        })
    }

    fn eval_named_object(
        &mut self,
        object_name: &NameString,
        named_object: NamedObject,
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        match named_object {
            NamedObject::DefBankField(f) => {
                pr_err!("DefBankField is not implemented: {:?}", f);
                Err(AmlError::UnsupportedType)
            }
            NamedObject::DefCreateField(f) => {
                let source_variable = self.create_aml_variable_reference_from_term_arg(
                    f.get_source_buffer().clone(),
                    current_scope,
                )?;
                if f.is_bit_field() {
                    let index = self
                        .eval_integer_expression(f.get_index().clone(), current_scope)?
                        .to_int()?;
                    let field_size = if let Some(field_size) = f.get_source_size() {
                        assert_eq!(field_size, 1);
                        field_size
                    } else {
                        self.eval_integer_expression(
                            f.get_source_size_term_arg().as_ref().unwrap().clone(),
                            current_scope,
                        )?
                        .to_int()?
                    };
                    Ok(AmlVariable::BitField(AmlBitFiled {
                        source: source_variable,
                        bit_index: index,
                        num_of_bits: field_size,
                        access_align: 1,
                        should_lock_global_lock: false,
                    }))
                } else {
                    let index = self
                        .eval_integer_expression(f.get_index().clone(), current_scope)?
                        .to_int()?;
                    let field_size = f.get_source_size().unwrap();
                    Ok(AmlVariable::ByteField(AmlByteFiled {
                        source: source_variable,
                        byte_index: index,
                        num_of_bytes: field_size,
                        should_lock_global_lock: false,
                    }))
                }
            }
            NamedObject::DefDataRegion(d) => {
                pr_err!("DefDataRegion is not implemented: {:?}", d);
                Err(AmlError::UnsupportedType)
            }
            NamedObject::DefDevice(d) => {
                pr_err!("Converting Device({:?}) to AmlVariable is invalid.", d);
                Err(AmlError::InvalidOperation)
            }
            NamedObject::DefField(f) => {
                let mut access_size = f.get_access_size();
                let should_lock_global_lock = f.should_lock();
                let source = self.search_aml_variable(f.get_source_region_name(), None, false)?;
                let mut index = 0;
                let mut field_list = f.get_field_list().clone();
                let relative_name = object_name
                    .get_relative_name(current_scope)
                    .unwrap_or_else(|| object_name.clone());

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
                            let last_name = entry_name.get_single_name_path().unwrap_or(entry_name);
                            if relative_name.suffix_search(&last_name) {
                                return Ok(AmlVariable::BitField(AmlBitFiled {
                                    source,
                                    bit_index: index,
                                    num_of_bits: pkg_length.length,
                                    access_align: access_size,
                                    should_lock_global_lock,
                                }));
                            } else {
                                index += pkg_length.length;
                            }
                        }
                    }
                }
                pr_err!("{} is not found in FiledList.", relative_name);
                Err(AmlError::AccessOutOfRange)
            }
            NamedObject::DefEvent(_) => {
                pr_err!("DefEvent is not implemented.");
                Err(AmlError::UnsupportedType)
            }
            NamedObject::DefIndexField(field) => {
                let index_register =
                    self.search_aml_variable(field.get_index_register(), None, false)?;
                let data_register =
                    self.search_aml_variable(field.get_data_register(), None, false)?;
                let mut access_size = field.get_access_size();
                let mut index = 0;
                let mut field_list = field.get_field_list().clone();
                let relative_name = object_name
                    .get_relative_name(current_scope)
                    .unwrap_or_else(|| object_name.clone());

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
                            let last_name = entry_name.get_single_name_path().unwrap_or(entry_name);
                            if relative_name.suffix_search(&last_name) {
                                return Ok(AmlVariable::IndexField(AmlIndexField {
                                    index_register,
                                    data_register,
                                    bit_index: index,
                                    num_of_bits: pkg_length.length,
                                    access_align: access_size,
                                    should_lock_global_lock: field.should_lock(),
                                }));
                            } else {
                                index += pkg_length.length;
                            }
                        }
                    }
                }
                pr_err!("{} is not found in FiledList.", relative_name);
                Err(AmlError::AccessOutOfRange)
            }
            NamedObject::DefMethod(m) => Ok(AmlVariable::Method(m)),
            NamedObject::DefMutex(m) => Ok(AmlVariable::Mutex(Arc::new((AtomicU8::new(0), m.1)))),
            NamedObject::DefExternal(e) => {
                pr_err!("Cannot get real object of {}.", e.get_name());
                Err(AmlError::InvalidType)
            }
            NamedObject::DefOpRegion(operation_region) => {
                let region_type = operation_region.get_operation_type()?;
                let offset = self
                    .eval_integer_expression(
                        operation_region.get_region_offset().clone(),
                        current_scope,
                    )?
                    .to_int()?;
                let length = self
                    .eval_integer_expression(
                        operation_region.get_region_length().clone(),
                        current_scope,
                    )?
                    .to_int()?;
                Ok(match region_type {
                    OperationRegionType::SystemMemory => AmlVariable::MMIo((offset, length)),
                    OperationRegionType::SystemIO => AmlVariable::Io((offset, length)),
                    OperationRegionType::EmbeddedControl => AmlVariable::EcIo((offset, length)),
                    OperationRegionType::PciConfig => {
                        let operation_region_scope = operation_region.get_name().get_scope_name();
                        const BBN_BASE_NAME: NameString =
                            NameString::from_array_const(&[*b"_BBN"], false);
                        let bbn_name =
                            BBN_BASE_NAME.get_full_name_path(&operation_region_scope, false);
                        let bus = match self.search_aml_variable(&bbn_name, None, false) {
                            Ok(v) => {
                                let locked_bbn = v.try_lock().or(Err(AmlError::MutexError))?;
                                (match &*locked_bbn {
                                    AmlVariable::ConstData(c) => c.to_int(),
                                    AmlVariable::Method(m) => {
                                        let method = m.clone();
                                        drop(locked_bbn);
                                        let eval_result =
                                            self.eval_method(&method, &[], Some(current_scope))?;
                                        match eval_result.to_int() {
                                            Ok(b) => b,
                                            Err(_) => {
                                                pr_err!(
                                                    "Expected bus number, but found {:?}",
                                                    eval_result
                                                );
                                                Err(AmlError::InvalidType)?
                                            }
                                        }
                                    }
                                    _ => {
                                        pr_err!("Expected bus number, but found {:?}", *locked_bbn);
                                        Err(AmlError::InvalidType)?
                                    }
                                } & 0xFF) as u16
                            }
                            Err(AmlError::InvalidName(m)) => {
                                if m == bbn_name {
                                    pr_info!("{} is not found. Assume the bus number is 0.", m);
                                    0
                                } else {
                                    Err(AmlError::InvalidName(m))?
                                }
                            }
                            Err(e) => Err(e)?,
                        };

                        const ADR_BASE_NAME: NameString =
                            NameString::from_array_const(&[*b"_ADR"], false);
                        let adr_name =
                            ADR_BASE_NAME.get_full_name_path(&operation_region_scope, false);
                        let adr = self.search_aml_variable(&adr_name, None, false)?;
                        let locked_adr = adr.try_lock().or(Err(AmlError::MutexError))?;
                        let addr = match &*locked_adr {
                            AmlVariable::ConstData(c) => c.to_int(),
                            AmlVariable::Method(m) => {
                                let method = m.clone();
                                drop(locked_adr);
                                let eval_result =
                                    self.eval_method(&method, &[], Some(current_scope))?;
                                match eval_result.to_int() {
                                    Ok(b) => b,
                                    Err(_) => {
                                        pr_err!(
                                            "Expected device/function number, but found {:?}",
                                            eval_result
                                        );
                                        Err(AmlError::InvalidType)?
                                    }
                                }
                            }
                            _ => {
                                pr_err!(
                                    "Expected device/function number, but found {:?}",
                                    *locked_adr
                                );
                                Err(AmlError::InvalidType)?
                            }
                        };
                        let device = ((addr >> 16) & 0xFFFF) as u16;
                        let function = (addr & 0xFFFF) as u16;
                        pr_debug!(
                            "{}(bus:{}, device:{}, function:{}, offset:{}, length:{})",
                            operation_region.get_name(),
                            bus,
                            device,
                            function,
                            offset,
                            length
                        );
                        AmlVariable::PciConfig(AmlPciConfig {
                            bus,
                            device,
                            function,
                            offset,
                            length,
                        })
                    }
                    _ => {
                        pr_err!("Unsupported Type: {:?}", region_type);
                        Err(AmlError::UnsupportedType)?
                    }
                })
            }
            NamedObject::DefPowerRes(_) => {
                pr_err!("DefPowerResource is not implemented.");
                Err(AmlError::UnsupportedType)
            }
            NamedObject::DefThermalZone(_) => {
                pr_err!("DefThermalZone is not implemented.");
                Err(AmlError::UnsupportedType)
            }
        }
    }

    fn create_aml_variable_reference_from_expression_opcode(
        &mut self,
        e: ExpressionOpcode,
        current_scope: &NameString,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        let result = self.eval_expression(e, current_scope)?;
        if let AmlVariable::Reference((source, None)) = result {
            Ok(source)
        } else if matches!(result, AmlVariable::Reference(_)) {
            Ok(Arc::new(Mutex::new(result)))
        } else {
            pr_warn!("Expected a reference, but found {:?}.", result);
            Ok(Arc::new(Mutex::new(result)))
        }
    }

    fn create_aml_variable_reference_from_super_name(
        &mut self,
        super_name: &SuperName,
        current_scope: &NameString,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        match super_name {
            SuperName::SimpleName(simple_name) => match simple_name {
                SimpleName::NameString(name) => self.search_aml_variable(name, None, false),
                SimpleName::ArgObj(c) => {
                    if *c as usize >= Self::NUMBER_OF_ARGUMENT_VARIABLES {
                        pr_err!("ArgObj({}) is out of index.", c);
                        Err(AmlError::InvalidOperation)
                    } else {
                        Ok(self.current_argument_variables[*c as usize].clone())
                    }
                }
                SimpleName::LocalObj(c) => {
                    if *c as usize >= Self::NUMBER_OF_LOCAL_VARIABLES {
                        pr_err!("LocalObj({}) is out of index.", c);
                        Err(AmlError::InvalidOperation)
                    } else {
                        Ok(self.current_local_variables[*c as usize].clone())
                    }
                }
            },
            SuperName::DebugObj => {
                pr_info!("Using DebugObj");
                Err(AmlError::UnsupportedType)
            }
            SuperName::ReferenceTypeOpcode(r) => self
                .create_aml_variable_reference_from_expression_opcode(
                    ExpressionOpcode::ReferenceTypeOpcode((**r).clone()),
                    current_scope,
                ),
        }
    }

    fn create_aml_variable_reference_from_term_arg(
        &mut self,
        term_arg: TermArg,
        current_scope: &NameString,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        match term_arg {
            TermArg::ExpressionOpcode(e) => {
                self.create_aml_variable_reference_from_expression_opcode(*e, current_scope)
            }
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
                        AmlVariable::Buffer(self.byte_list_to_vec(byte_list, current_scope)?),
                    ))),
                },
                DataObject::DefPackage(p) => Ok(Arc::new(Mutex::new(AmlVariable::Package(
                    self.eval_package(p, current_scope)?,
                )))),
                DataObject::DefVarPackage(p) => Ok(Arc::new(Mutex::new(AmlVariable::Package(
                    self.eval_var_package(p, current_scope)?,
                )))),
            },
            TermArg::ArgObj(c) => {
                if c as usize >= Self::NUMBER_OF_ARGUMENT_VARIABLES {
                    pr_err!("Reading ArgObj({}) is invalid.", c);
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok(self.current_argument_variables[c as usize].clone())
                }
            }
            TermArg::LocalObj(c) => {
                if c as usize >= Self::NUMBER_OF_LOCAL_VARIABLES {
                    pr_err!("Reading LocalObj({}) is invalid.", c);
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok(self.current_local_variables[c as usize].clone())
                }
            }
        }
    }

    fn search_mutex_object(
        &mut self,
        mutex_name: &SuperName,
        current_scope: &NameString,
    ) -> Result<Arc<(AtomicU8, u8)>, AmlError> {
        let aml_variable =
            &self.create_aml_variable_reference_from_super_name(mutex_name, current_scope)?;
        let locked_aml_variable = aml_variable.try_lock().or(Err(AmlError::MutexError))?;
        let mutex_object = if let AmlVariable::Mutex(m) = &*locked_aml_variable {
            m.clone()
        } else if let AmlVariable::Mutex(m) = locked_aml_variable.get_constant_data()? {
            m.clone()
        } else {
            pr_err!(
                "Invalid Mutex Object Reference: {:?}",
                &*locked_aml_variable
            );
            Err(AmlError::InvalidOperation)?
        };
        Ok(mutex_object)
    }

    fn eval_package_list(
        &mut self,
        mut p: Package,
        num: usize,
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
                                    v.push(AmlPackage::Buffer(
                                        self.byte_list_to_vec(byte_list, current_scope)?,
                                    ));
                                }
                            },
                            DataObject::DefPackage(package) => {
                                v.push(AmlPackage::Package(
                                    self.eval_package(package, current_scope)?,
                                ));
                            }
                            DataObject::DefVarPackage(var_package) => {
                                v.push(AmlPackage::Package(
                                    self.eval_var_package(var_package, current_scope)?,
                                ));
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
        Ok(v)
    }

    fn eval_package(
        &mut self,
        p: Package,
        current_scope: &NameString,
    ) -> Result<Vec<AmlPackage>, AmlError> {
        let num = p.get_number_of_remaining_elements();
        self.eval_package_list(p, num, current_scope)
    }

    fn eval_var_package(
        &mut self,
        mut p: VarPackage,
        current_scope: &NameString,
    ) -> Result<Vec<AmlPackage>, AmlError> {
        let number_of_elements_term = p.get_number_of_elements(self, current_scope)?;
        let number_of_elements = self
            .eval_integer_expression(number_of_elements_term, current_scope)?
            .to_int()?;
        self.eval_package_list(
            p.convert_to_package(number_of_elements),
            number_of_elements,
            current_scope,
        )
    }

    fn byte_list_to_vec(
        &mut self,
        mut byte_list: ByteList,
        current_scope: &NameString,
    ) -> Result<Vec<u8>, AmlError> {
        let buffer_size_term_arg = byte_list.get_buffer_size(self)?;
        let buffer_size = self
            .eval_integer_expression(buffer_size_term_arg, current_scope)?
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
        current_scope: &NameString,
    ) -> Result<(), AmlError> {
        match target {
            Target::Null => {
                return Err(AmlError::InvalidOperation);
            }
            Target::SuperName(s) => match s {
                SuperName::SimpleName(s_n) => match s_n {
                    SimpleName::NameString(n) => {
                        self.search_aml_variable(n, None, false)?
                            .try_lock()
                            .or(Err(AmlError::MutexError))?
                            .write(data)?;
                    }
                    SimpleName::ArgObj(l) => {
                        if *l as usize >= Self::NUMBER_OF_ARGUMENT_VARIABLES {
                            pr_err!("Writing ArgObj({}) is invalid.", l);
                            return Err(AmlError::InvalidOperation);
                        }
                        self.current_argument_variables[*l as usize]
                            .try_lock()
                            .or(Err(AmlError::MutexError))?
                            .write(data)?;
                    }
                    SimpleName::LocalObj(l) => {
                        if *l as usize >= Self::NUMBER_OF_LOCAL_VARIABLES {
                            pr_err!("Writing LocalObj({}) is invalid.", l);
                            return Err(AmlError::InvalidOperation);
                        }
                        self.current_local_variables[*l as usize]
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
                        self.eval_term_arg(reference.clone(), current_scope)?
                            .write(data)?;
                    }
                    ReferenceTypeOpcode::DefIndex(i) => {
                        let buffer = self.create_aml_variable_reference_from_term_arg(
                            i.get_source().clone(),
                            current_scope,
                        )?;
                        let index = self
                            .eval_term_arg(i.get_index().clone(), current_scope)?
                            .to_int()?;
                        let mut aml_variable = AmlVariable::Reference((buffer, Some(index)));
                        aml_variable.write(data)?;
                        if !i.get_destination().is_null() {
                            self.write_data_into_target(
                                aml_variable,
                                i.get_destination(),
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
        Ok(())
    }

    fn eval_bool_expression(
        &mut self,
        e: TermArg,
        current_scope: &NameString,
    ) -> Result<bool, AmlError> {
        let data = self.eval_term_arg(e, current_scope)?;
        match data.to_int() {
            Ok(val) => Ok(val != 0),
            Err(err) => {
                pr_err!("Expected Boolean, but found {:?}({:?}).", data, err);
                Err(AmlError::InvalidType)
            }
        }
    }

    fn eval_integer_expression(
        &mut self,
        t: TermArg,
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        let data = self.eval_term_arg(t, current_scope)?;
        let constant_data = if data.is_constant_data() {
            data
        } else {
            data.get_constant_data()?
        };
        if let Err(e) = constant_data.to_int() {
            pr_err!("Expected Integer, but found {:?}({:?}).", constant_data, e);
            Err(AmlError::InvalidType)
        } else {
            Ok(constant_data)
        }
    }

    pub(super) fn eval_term_arg(
        &mut self,
        t: TermArg,
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        match t {
            TermArg::ExpressionOpcode(e) => self.eval_expression(*e, current_scope),
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
                    ComputationalData::DefBuffer(byte_list) => Ok(AmlVariable::Buffer(
                        self.byte_list_to_vec(byte_list, current_scope)?,
                    )),
                },
                DataObject::DefPackage(p) => {
                    Ok(AmlVariable::Package(self.eval_package(p, current_scope)?))
                }
                DataObject::DefVarPackage(v_p) => Ok(AmlVariable::Package(
                    self.eval_var_package(v_p, current_scope)?,
                )),
            },
            TermArg::ArgObj(c) => {
                if c as usize >= Self::NUMBER_OF_ARGUMENT_VARIABLES {
                    pr_err!("Arg{} is out of index.", c);
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok((*self.current_argument_variables[c as usize])
                        .try_lock()
                        .or(Err(AmlError::MutexError))?
                        .clone())
                }
            }
            TermArg::LocalObj(c) => {
                if c as usize >= Self::NUMBER_OF_LOCAL_VARIABLES {
                    pr_err!("Local{} is out of index.", c);
                    Err(AmlError::InvalidOperation)
                } else {
                    Ok((*self.current_local_variables[c as usize])
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
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        match e {
            ExpressionOpcode::DefAcquire((mutex_name, wait)) => {
                let mutex_object = self.search_mutex_object(&mutex_name, current_scope)?;

                let current_tick = get_kernel_manager_cluster()
                    .global_timer_manager
                    .get_current_tick();
                while mutex_object
                    .0
                    .fetch_update(Ordering::Acquire, Ordering::Relaxed, |current_level| {
                        if current_level <= mutex_object.1 {
                            Some(current_level + 1)
                        } else {
                            None
                        }
                    })
                    .is_err()
                {
                    if wait != 0xFFFF
                        && get_kernel_manager_cluster()
                            .global_timer_manager
                            .get_difference_ms(current_tick)
                            >= wait as u64
                    {
                        pr_warn!("Acquiring Mutex({:?}) was timed out.", mutex_name);
                        return Ok(AmlVariable::ConstData(ConstData::Byte(1)));
                    }
                }
                Ok(AmlVariable::ConstData(ConstData::Byte(0)))
            }
            ExpressionOpcode::DefBuffer(byte_list) => Ok(AmlVariable::Buffer(
                self.byte_list_to_vec(byte_list, current_scope)?,
            )),
            ExpressionOpcode::DefPackage(p) => {
                Ok(AmlVariable::Package(self.eval_package(p, current_scope)?))
            }
            ExpressionOpcode::DefVarPackage(var_package) => Ok(AmlVariable::Package(
                self.eval_var_package(var_package, current_scope)?,
            )),
            ExpressionOpcode::DefProcessor => {
                pr_err!("DefProcessor was deleted from ACPI 6.4.");
                Err(AmlError::InvalidOperation)
            }
            ExpressionOpcode::DefConcat(_) => {
                pr_err!("DefConcat is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::DefConcatRes(_) => {
                pr_err!("DefConcatRes is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::DefCopyObject(_, _) => {
                pr_err!("DefCopyObject is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::BinaryOperation(b_o) => {
                let left =
                    self.eval_integer_expression(b_o.get_left_operand().clone(), current_scope)?;
                let right =
                    self.eval_integer_expression(b_o.get_right_operand().clone(), current_scope)?;
                let left_value = left.to_int()?;
                let right_value = right.to_int()?;
                use super::opcode;
                let result = match b_o.get_opcode() {
                    opcode::ADD_OP => left_value + right_value,
                    opcode::AND_OP => left_value & right_value,
                    opcode::MULTIPLY_OP => left_value * right_value,
                    opcode::NAND_OP => !left_value | !right_value,
                    opcode::MOD_OP => left_value % right_value,
                    opcode::NOR_OP => !left_value & !right_value,
                    opcode::OR_OP => left_value | right_value,
                    opcode::SHIFT_LEFT_OP => left_value << right_value,
                    opcode::SHIFT_RIGHT_OP => left_value >> right_value,
                    opcode::SUBTRACT_OP => left_value - right_value,
                    opcode::XOR_OP => left_value ^ right_value,
                    _ => Err(AmlError::InvalidOperation)?,
                };
                let result_aml_variable = AmlVariable::ConstData(ConstData::from_usize(
                    result,
                    left.get_byte_size()?.max(right.get_byte_size()?),
                )?);
                if !b_o.get_target().is_null() {
                    self.write_data_into_target(
                        result_aml_variable.clone(),
                        b_o.get_target(),
                        current_scope,
                    )?;
                }
                Ok(result_aml_variable)
            }
            ExpressionOpcode::DefDecrement(decrement) => {
                let obj =
                    self.create_aml_variable_reference_from_super_name(&decrement, current_scope)?;
                let mut locked_obj = obj.try_lock().or(Err(AmlError::MutexError))?;
                if locked_obj.is_constant_data() {
                    if let AmlVariable::ConstData(c) = *locked_obj {
                        let result = AmlVariable::ConstData(ConstData::from_usize(
                            c.to_int().wrapping_sub(1),
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
                            c.to_int().wrapping_sub(1),
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
                let obj =
                    self.create_aml_variable_reference_from_super_name(&increment, current_scope)?;
                let mut locked_obj = obj.try_lock().or(Err(AmlError::MutexError))?;
                if locked_obj.is_constant_data() {
                    if let AmlVariable::ConstData(c) = *locked_obj {
                        let result = AmlVariable::ConstData(ConstData::from_usize(
                            c.to_int().wrapping_add(1),
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
                            c.to_int().wrapping_add(1),
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
                let dividend =
                    self.eval_integer_expression(divide.get_dividend().clone(), current_scope)?;
                let divisor =
                    self.eval_integer_expression(divide.get_divisor().clone(), current_scope)?;
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
                        current_scope,
                    )?;
                }
                if !divide.get_remainder().is_null() {
                    let remainder = AmlVariable::ConstData(ConstData::from_usize(
                        dividend_data - result_data * divisor_data,
                        result_size,
                    )?);
                    self.write_data_into_target(remainder, divide.get_remainder(), current_scope)?;
                }
                Ok(result_aml_variable)
            }
            ExpressionOpcode::DefFindSetLeftBit((operand, target)) => {
                let operand_data: usize /* To detect error when changed the return type of to_int() */
                    = self.eval_integer_expression(operand, current_scope)?.to_int()?;
                let result = AmlVariable::ConstData(ConstData::Byte(
                    (usize::BITS - operand_data.leading_zeros()) as u8,
                ));
                if !target.is_null() {
                    self.write_data_into_target(result.clone(), &target, current_scope)?;
                }
                Ok(result)
            }
            ExpressionOpcode::DefFindSetRightBit((operand, target)) => {
                let operand_data: usize /* To detect error when changed the return type of to_int() */
                    = self.eval_integer_expression(operand, current_scope)?.to_int()?;
                let result = AmlVariable::ConstData(ConstData::Byte(if operand_data == 0 {
                    0
                } else {
                    (operand_data.trailing_zeros() + 1) as u8
                }));
                if !target.is_null() {
                    self.write_data_into_target(result.clone(), &target, current_scope)?;
                }
                Ok(result)
            }
            ExpressionOpcode::DefFromBCD(_) => {
                pr_err!("DefFromBCD is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::DefMatch(_) => {
                pr_err!("DefMatch is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::DefNot((operand, target)) => {
                let op = self.eval_integer_expression(operand, current_scope)?;
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
                pr_err!("DefObjectType is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::DefSizeOf(obj_name) => {
                let obj =
                    self.create_aml_variable_reference_from_super_name(&obj_name, current_scope)?;
                let byte_size = match &*obj.try_lock().or(Err(AmlError::MutexError))? {
                    AmlVariable::String(s) => s.len(),
                    AmlVariable::Buffer(b) => b.len(),
                    AmlVariable::Package(p) => p.len(),
                    AmlVariable::Reference((s, _)) => s
                        .try_lock()
                        .or(Err(AmlError::MutexError))?
                        .get_byte_size()?,
                    _ => Err(AmlError::InvalidOperation)?,
                };
                Ok(AmlVariable::ConstData(ConstData::QWord(byte_size as _)))
            }
            ExpressionOpcode::DefStore((data, destination)) => {
                let data = self.eval_term_arg(data, current_scope)?;
                self.write_data_into_target(
                    data.clone(),
                    &Target::SuperName(destination.clone()),
                    current_scope,
                )?;
                Ok(data)
            }
            ExpressionOpcode::DefToBCD(_) => {
                pr_err!("DefToBCD is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::DefToInteger((operand, target)) => {
                let obj = self.eval_term_arg(operand, current_scope)?;
                let constant_data = if obj.is_constant_data() {
                    obj
                } else {
                    obj.get_constant_data()?
                };
                let result = match constant_data {
                    AmlVariable::ConstData(c) => c,
                    AmlVariable::String(s) if !s.is_empty() => {
                        ConstData::QWord(parse_integer_from_buffer(s.as_bytes())? as _)
                    }
                    AmlVariable::Buffer(b) if !b.is_empty() => {
                        let mut result = 0u64;
                        for index in 0..b.len().min(8) {
                            result |= (b[index] as u64) << index;
                        }
                        ConstData::QWord(result)
                    }
                    _ => Err(AmlError::InvalidOperation)?,
                };
                if !target.is_null() {
                    self.write_data_into_target(
                        AmlVariable::ConstData(result),
                        &target,
                        current_scope,
                    )?;
                }
                Ok(AmlVariable::ConstData(result))
            }
            ExpressionOpcode::DefTimer => Ok(AmlVariable::ConstData(ConstData::QWord(
                get_cpu_manager_cluster()
                    .local_timer_manager
                    .get_monotonic_clock_ns(),
            ))),
            ExpressionOpcode::DefCondRefOf((source, destination)) => {
                let result =
                    self.create_aml_variable_reference_from_super_name(&source, current_scope);
                if matches!(result, Err(AmlError::InvalidName(_))) {
                    Ok(AmlVariable::ConstData(ConstData::Byte(0)))
                } else if let Ok(obj) = result {
                    if !destination.is_null() {
                        self.write_data_into_target(
                            AmlVariable::Reference((obj, None)),
                            &destination,
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
                    (self.eval_bool_expression(left, current_scope)?
                        && self.eval_bool_expression(right, current_scope)?)
                        as u8,
                )))
            }
            ExpressionOpcode::DefLEqual((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
                        .eval_integer_expression(left, current_scope)?
                        .to_int()?
                        == self
                            .eval_integer_expression(right, current_scope)?
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLGreater((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
                        .eval_integer_expression(left, current_scope)?
                        .to_int()?
                        > self
                            .eval_integer_expression(right, current_scope)?
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLGreaterEqual((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
                        .eval_integer_expression(left, current_scope)?
                        .to_int()?
                        >= self
                            .eval_integer_expression(right, current_scope)?
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLLess((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
                        .eval_integer_expression(left, current_scope)?
                        .to_int()?
                        < self
                            .eval_integer_expression(right, current_scope)?
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLLessEqual((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
                        .eval_integer_expression(left, current_scope)?
                        .to_int()?
                        <= self
                            .eval_integer_expression(right, current_scope)?
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLNot(source) => Ok(AmlVariable::ConstData(ConstData::Byte(
                (self
                    .eval_integer_expression(source, current_scope)?
                    .to_int()?
                    == 0) as u8,
            ))),
            ExpressionOpcode::DefLNotEqual((left, right)) => {
                Ok(AmlVariable::ConstData(ConstData::Byte(
                    (self
                        .eval_integer_expression(left, current_scope)?
                        .to_int()?
                        != self
                            .eval_integer_expression(right, current_scope)?
                            .to_int()?) as u8,
                )))
            }
            ExpressionOpcode::DefLoad(_) => {
                pr_err!("DefLoad is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::DefLoadTable(_) => {
                pr_err!("DefLoadTable is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::DefLOr((left, right)) => Ok(AmlVariable::ConstData(ConstData::Byte(
                (self.eval_bool_expression(left, current_scope)?
                    || self.eval_bool_expression(right, current_scope)?) as u8,
            ))),
            ExpressionOpcode::DefWait(_) => {
                pr_err!("DefWait is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::ReferenceTypeOpcode(r_e) => match r_e {
                ReferenceTypeOpcode::DefRefOf(super_name) => Ok(AmlVariable::Reference((
                    self.create_aml_variable_reference_from_super_name(&super_name, current_scope)?,
                    None,
                ))),
                ReferenceTypeOpcode::DefDerefOf(reference) => {
                    self.eval_term_arg(reference.clone(), current_scope)
                }
                ReferenceTypeOpcode::DefIndex(i) => {
                    let buffer = self.create_aml_variable_reference_from_term_arg(
                        i.get_source().clone(),
                        current_scope,
                    )?;
                    let index = self
                        .eval_term_arg(i.get_index().clone(), current_scope)?
                        .to_int()?;
                    let aml_variable = AmlVariable::Reference((buffer, Some(index)));
                    if !i.get_destination().is_null() {
                        self.write_data_into_target(
                            aml_variable.clone(),
                            i.get_destination(),
                            current_scope,
                        )?;
                    }
                    Ok(aml_variable)
                }
                ReferenceTypeOpcode::UserTermObj => {
                    pr_err!("UserTermObj is not supported.");
                    Err(AmlError::InvalidType)
                }
            },
            ExpressionOpcode::DefMid(_) => {
                pr_err!("DefMid is not supported currently: {:?}", e);
                Err(AmlError::UnsupportedType)
            }
            ExpressionOpcode::DefToBuffer((operand, target)) => {
                let obj = self.eval_term_arg(operand, current_scope)?;
                let constant_data = if obj.is_constant_data() {
                    obj
                } else {
                    obj.get_constant_data()?
                };
                let result = match constant_data {
                    AmlVariable::ConstData(c) => {
                        let mut result = Vec::new();
                        let mut data = c.to_int();
                        while data != 0 {
                            result.push((data & 0xff) as u8);
                            data >>= 8;
                        }
                        if result.is_empty() {
                            result.push(0);
                        }
                        result
                    }
                    AmlVariable::String(s) if !s.is_empty() => {
                        let mut result = Vec::from(s);
                        result.push(0);
                        result
                    }
                    AmlVariable::String(s) if s.is_empty() => Vec::new(),
                    AmlVariable::Buffer(b) => b,
                    _ => Err(AmlError::InvalidOperation)?,
                };
                if !target.is_null() {
                    self.write_data_into_target(
                        AmlVariable::Buffer(result.clone()),
                        &target,
                        current_scope,
                    )?;
                }
                Ok(AmlVariable::Buffer(result))
            }
            ExpressionOpcode::DefToDecimalString((operand, target)) => {
                let obj = self.eval_term_arg(operand, current_scope)?;
                let constant_data = if obj.is_constant_data() {
                    obj
                } else {
                    obj.get_constant_data()?
                };
                let result = match constant_data {
                    AmlVariable::ConstData(c) => {
                        format!("{}", c.to_int())
                    }
                    AmlVariable::String(s) => s,
                    AmlVariable::Buffer(b) if !b.is_empty() => {
                        let mut result = format!("{}", b[0]);
                        for e in b.iter().skip(1) {
                            result.push_str(format!(",{}", e).as_str());
                        }
                        result
                    }
                    AmlVariable::Buffer(b) if b.is_empty() => String::new(),
                    _ => Err(AmlError::InvalidOperation)?,
                };
                if !target.is_null() {
                    self.write_data_into_target(
                        AmlVariable::String(result.clone()),
                        &target,
                        current_scope,
                    )?;
                }
                Ok(AmlVariable::String(result))
            }
            ExpressionOpcode::DefToHexString((operand, target)) => {
                let obj = self.eval_term_arg(operand, current_scope)?;
                let constant_data = if obj.is_constant_data() {
                    obj
                } else {
                    obj.get_constant_data()?
                };
                let result = match constant_data {
                    AmlVariable::ConstData(c) => {
                        format!("{:X}", c.to_int())
                    }
                    AmlVariable::String(s) => s,
                    AmlVariable::Buffer(b) if !b.is_empty() => {
                        let mut result = format!("{:X}", b[0]);
                        for e in b.iter().skip(1) {
                            result.push_str(format!(",{:X}", e).as_str());
                        }
                        result
                    }
                    AmlVariable::Buffer(b) if b.is_empty() => String::new(),
                    _ => Err(AmlError::InvalidOperation)?,
                };
                if !target.is_null() {
                    self.write_data_into_target(
                        AmlVariable::String(result.clone()),
                        &target,
                        current_scope,
                    )?;
                }
                Ok(AmlVariable::String(result))
            }
            ExpressionOpcode::DefToString(((operand, length), target)) => {
                let data = self.eval_term_arg(operand, current_scope)?;
                let constant_data = if data.is_constant_data() {
                    data
                } else {
                    data.get_constant_data()?
                };
                let len = self
                    .eval_integer_expression(length, current_scope)?
                    .to_int()?;

                let result = match constant_data {
                    AmlVariable::Buffer(mut b) if !b.is_empty() => {
                        if len != 0 && len != ACPI_INT_ONES {
                            b.truncate(len);
                        };
                        String::from_utf8(b).or(Err(AmlError::InvalidOperation))?
                    }
                    AmlVariable::Buffer(b) if b.is_empty() => String::new(),
                    _ => Err(AmlError::InvalidOperation)?,
                };
                if !target.is_null() {
                    self.write_data_into_target(
                        AmlVariable::String(result.clone()),
                        &target,
                        current_scope,
                    )?;
                }
                Ok(AmlVariable::String(result))
            }
            ExpressionOpcode::MethodInvocation(method_invocation) => {
                let obj = self.search_aml_variable(method_invocation.get_name(), None, false)?;

                let locked_obj = obj.try_lock().or(Err(AmlError::MutexError))?;
                match &*locked_obj {
                    AmlVariable::Method(method) => {
                        let cloned_method = method.clone();
                        drop(locked_obj);
                        self.eval_method_invocation(
                            &method_invocation,
                            &cloned_method,
                            current_scope,
                        )
                    }
                    AmlVariable::BuiltInMethod((func, _)) => {
                        let cloned_func = *func;
                        drop(locked_obj);
                        self.eval_builtin_method(&method_invocation, cloned_func, current_scope)
                    }

                    _ => Ok(AmlVariable::Reference((obj, None))),
                }
            }
        }
    }

    fn eval_notify(&mut self, notify: Notify, current_scope: &NameString) -> Result<(), AmlError> {
        let notify_value = self.eval_term_arg(notify.get_notify_value().clone(), current_scope)?;
        let notify_list = get_kernel_manager_cluster()
            .acpi_event_manager
            .get_notify_list();
        if let SuperName::SimpleName(SimpleName::NameString(n)) = notify.get_notify_object_name() {
            if !notify_list.notify(n, notify_value) {
                pr_warn!(
                    "No function to handle the notify {}({:?}) was found.",
                    n,
                    notify.get_notify_value()
                );
            }
        } else {
            pr_err!(
                "Notify {:?}({:?}) is not supported currently.",
                notify.get_notify_object_name(),
                notify_value
            );
        }
        Ok(())
    }

    fn release_mutex(
        &mut self,
        mutex_name: &SuperName,
        current_scope: &NameString,
    ) -> Result<(), AmlError> {
        self.search_mutex_object(mutex_name, current_scope)?
            .0
            .fetch_sub(1, Ordering::Release);
        Ok(())
    }

    fn reset_event(&mut self, event: &SuperName) -> Result<(), AmlError> {
        pr_err!("Resetting {:?} is not supported currently.", event);
        Err(AmlError::UnsupportedType)
    }

    fn eval_break_point(&self, term_list: &TermList) {
        pr_debug!("AML BreakPoint: {:?}", term_list);
    }

    fn eval_fatal(&self, fatal: &Fatal, term_list: &TermList) -> Result<(), AmlError> {
        pr_err!("AML Fatal: {:?} ({:?})", fatal, term_list);
        Ok(())
    }

    fn eval_signal(&self, signal: &SuperName) -> Result<(), AmlError> {
        pr_err!("Signal to {:?} is not supported currently.", signal);
        Err(AmlError::UnsupportedType)
    }

    fn eval_sleep(
        &mut self,
        milli_seconds: TermArg,
        current_scope: &NameString,
    ) -> Result<(), AmlError> {
        let seconds = self
            .eval_integer_expression(milli_seconds, current_scope)?
            .to_int()? as u64;
        if get_kernel_manager_cluster()
            .global_timer_manager
            .busy_wait_ms(seconds)
        {
            Ok(())
        } else {
            pr_err!("Sleeping {}ms was failed.", seconds);
            Err(AmlError::InvalidOperation)
        }
    }

    fn eval_stall(
        &mut self,
        micro_seconds: TermArg,
        current_scope: &NameString,
    ) -> Result<(), AmlError> {
        let seconds = self
            .eval_integer_expression(micro_seconds, current_scope)?
            .to_int()? as u64;

        if get_kernel_manager_cluster()
            .global_timer_manager
            .busy_wait_us(seconds)
        {
            Ok(())
        } else {
            pr_err!("Sleeping {}us was failed.", seconds);
            Err(AmlError::InvalidOperation)
        }
    }

    fn eval_if_else(
        &mut self,
        i_e: IfElse,
        current_scope: &NameString,
    ) -> Result<Option<StatementOpcode>, AmlError> {
        let predicate = i_e.get_predicate();
        if self.eval_bool_expression(predicate.clone(), current_scope)? {
            let true_statement = i_e.get_if_true_term_list();
            self.term_list_hierarchy.push(true_statement.clone());
            let result = self.eval_term_list(true_statement.clone(), current_scope);
            self.term_list_hierarchy.pop();
            result
        } else if let Some(false_statement) = i_e.get_if_false_term_list() {
            self.term_list_hierarchy.push(false_statement.clone());
            let result = self.eval_term_list(false_statement.clone(), current_scope);
            self.term_list_hierarchy.pop();
            result
        } else {
            Ok(None)
        }
    }

    fn eval_while(
        &mut self,
        w: While,
        current_scope: &NameString,
    ) -> Result<Option<StatementOpcode>, AmlError> {
        let predicate = w.get_predicate();
        let term_list = w.get_term_list();
        self.term_list_hierarchy.push(term_list.clone());
        loop {
            if !self.eval_bool_expression(predicate.clone(), current_scope)? {
                self.term_list_hierarchy.pop();
                return Ok(None);
            }

            match self.eval_term_list(term_list.clone(), current_scope) {
                Ok(None) | Ok(Some(StatementOpcode::DefContinue)) => { /* Continue */ }
                Ok(Some(StatementOpcode::DefBreak)) => {
                    self.term_list_hierarchy.pop();
                    return Ok(None);
                }
                d => {
                    self.term_list_hierarchy.pop();
                    return d;
                }
            }
        }
    }

    fn eval_term_list(
        &mut self,
        mut term_list: TermList,
        current_scope: &NameString,
    ) -> Result<Option<StatementOpcode>, AmlError> {
        while let Some(term_obj) = term_list.next(self)? {
            match term_obj {
                TermObj::NamespaceModifierObj(_) => { /* Ignore */ }
                TermObj::NamedObj(_) => { /* Ignore */ }
                TermObj::StatementOpcode(s_o) => match s_o {
                    StatementOpcode::DefNoop => { /* Do Nothing */ }
                    StatementOpcode::DefNotify(n) => {
                        self.eval_notify(n, current_scope)?;
                    }
                    StatementOpcode::DefRelease(m) => {
                        self.release_mutex(&m, current_scope)?;
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
                        self.eval_sleep(sleep, current_scope)?;
                    }
                    StatementOpcode::DefStall(sleep) => {
                        self.eval_stall(sleep, current_scope)?;
                    }
                    StatementOpcode::DefWhile(w) => {
                        let result = self.eval_while(w, current_scope);
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
                        let result = self.eval_if_else(i_e, current_scope);
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
                    self.eval_expression(e_o, current_scope)?;
                }
            }
        }
        Ok(None)
    }

    fn eval_builtin_method(
        &mut self,
        method_invocation: &MethodInvocation,
        func: AmlFunction,
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        let (_, mut new_argument_variables) = Self::init_local_variables_and_argument_variables();
        for (d, arg) in new_argument_variables
            .iter_mut()
            .zip(method_invocation.get_ter_arg_list().list.iter())
        {
            *d = Arc::new(Mutex::new(self.eval_term_arg(arg.clone(), current_scope)?));
        }
        func(&new_argument_variables)
    }

    /// Evaluate method with current variable_tree and term_list_hierarchy
    ///
    /// method::term_list will be pushed in term_list_hierarchy
    fn eval_method_in_current_status(
        &mut self,
        method: &Method,
        arguments: &[AmlVariable],
    ) -> Result<AmlVariable, AmlError> {
        let (mut new_local_variables, mut new_argument_variables) =
            Self::init_local_variables_and_argument_variables();

        if method.get_argument_count() != arguments.len() {
            let mut num_of_valid_arguments = 0;
            for e in arguments {
                if matches!(e, AmlVariable::Uninitialized) {
                    break;
                }
                num_of_valid_arguments += 1;
            }
            if num_of_valid_arguments != method.get_argument_count() {
                pr_err!(
                    "Expected {} arguments, but found {} arguments.",
                    method.get_argument_count(),
                    arguments.len()
                );
                return Err(AmlError::InvalidOperation);
            }
        }

        for (destination, source) in new_argument_variables.iter_mut().zip(arguments.iter()) {
            if matches!(source, AmlVariable::Uninitialized) {
                continue;
            }
            *destination = Arc::new(Mutex::new(source.clone()));
        }

        self.term_list_hierarchy
            .push(method.get_term_list().clone());

        /* Backup current scope and move into method's scope */
        let scope_backup = self.variable_tree.backup_current_scope();
        self.variable_tree
            .move_current_scope(method.get_term_list().get_scope_name())?;
        /* Swap local variables and argument variables */
        core::mem::swap(&mut self.current_local_variables, &mut new_local_variables);
        core::mem::swap(
            &mut self.current_argument_variables,
            &mut new_argument_variables,
        );

        let result = self.eval_term_list(method.get_term_list().clone(), method.get_name());

        let result = match result {
            Err(e) => {
                pr_err!("Evaluating {} was failed: {:?}", method.get_name(), e);
                Err(e)
            }
            Ok(None) => Ok(AmlVariable::Uninitialized),
            Ok(Some(v)) => match v {
                StatementOpcode::DefFatal(_) => Err(AmlError::InvalidOperation),
                StatementOpcode::DefReturn(return_value) => Ok(self
                    .eval_term_arg(return_value, method.get_name())?
                    .get_constant_data()?
                    .clone()),
                _ => {
                    pr_err!("Unexpected StatementCode: {:?}", v);
                    Err(AmlError::InvalidOperation)
                }
            },
        };

        if self
            .term_list_hierarchy
            .pop()
            .map(|t| &t != method.get_term_list())
            .unwrap_or(true)
        {
            pr_err!("TermListHierarchy may be broken.");
        }

        /* Restore status */
        self.variable_tree.restore_current_scope(scope_backup);
        core::mem::swap(&mut self.current_local_variables, &mut new_local_variables);
        core::mem::swap(
            &mut self.current_argument_variables,
            &mut new_argument_variables,
        );

        result
    }

    pub fn eval_method(
        &mut self,
        method: &Method,
        arguments: &[AmlVariable],
        search_scope: Option<&NameString>,
    ) -> Result<AmlVariable, AmlError> {
        /* Backup the current status */
        let mut term_list_hierarchy_backup = Vec::with_capacity(self.term_list_hierarchy.len());
        core::mem::swap(
            &mut self.term_list_hierarchy,
            &mut term_list_hierarchy_backup,
        );
        let tree_backup = self.variable_tree.backup_current_scope();
        let current_term_list_backup = self.current_root_term_list.clone();

        self.variable_tree.move_to_root()?;
        self.move_into_object(method.get_name(), search_scope)?;

        let result = self.eval_method_in_current_status(method, arguments);

        /* Restore the status */
        self.term_list_hierarchy = term_list_hierarchy_backup;
        self.variable_tree.restore_current_scope(tree_backup);
        self.current_root_term_list = current_term_list_backup;

        result
    }

    fn eval_method_invocation(
        &mut self,
        method_invocation: &MethodInvocation,
        method: &Method,
        current_scope: &NameString,
    ) -> Result<AmlVariable, AmlError> {
        if method_invocation.get_ter_arg_list().list.len() != method.get_argument_count() {
            pr_err!(
                "Expected {} arguments, but found {} arguments.",
                method.get_argument_count(),
                method_invocation.get_ter_arg_list().list.len(),
            );
            return Err(AmlError::InvalidOperation);
        }

        let mut arguments: [AmlVariable; Self::NUMBER_OF_ARGUMENT_VARIABLES] =
            [const { AmlVariable::Uninitialized }; Self::NUMBER_OF_ARGUMENT_VARIABLES];

        for (destination, source) in arguments
            .iter_mut()
            .zip(method_invocation.get_ter_arg_list().list.iter())
        {
            *destination = self.eval_term_arg(source.clone(), current_scope)?;
        }

        self.eval_method(method, &arguments, Some(current_scope))
    }

    pub fn get_current_scope(&self) -> &NameString {
        self.variable_tree.get_current_scope_name()
    }
}
