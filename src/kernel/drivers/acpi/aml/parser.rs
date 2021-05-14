//!
//! ACPI Machine Language Parse Helper
//!

use super::data_object::{ComputationalData, DataObject, DataRefObject};
use super::eisa_id_to_dword;
use super::name_object::NameString;
use super::named_object::{Device, External, FieldElement, NamedObject};
use super::namespace_modifier_object::NamespaceModifierObject;
use super::term_object::{TermList, TermObj};
use super::{AcpiInt, AmlError};

use crate::kernel::sync::spin_lock::Mutex;

use core::mem;

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

#[derive(Clone, Debug)]
enum ObjectListItem {
    DefScope(Arc<Mutex<ObjectList>>),
    DefName(DataRefObject),
    DefAlias(NameString),
    NamedObject(NamedObject),
}

#[derive(Debug)]
struct ObjectList {
    scope_name: NameString,
    parent: Option<Weak<Mutex<Self>>>,
    list: Vec<(NameString, ObjectListItem)>,
}

#[derive(Debug)]
pub enum ContentObject {
    NamedObject(NamedObject),
    DataRefObject(DataRefObject),
    Scope(NameString),
}

#[derive(Clone)]
pub struct ParseHelper {
    current_root_term_list: TermList,
    root_term_list_vec: Arc<Vec<TermList>>,
    root_object_list: Arc<Mutex<ObjectList>>,
    current_object_list: Arc<Mutex<ObjectList>>,
    original_name_searching: Option<NameString>,
    term_list_hierarchy: Vec<TermList>,
}

impl ObjectList {
    fn new(parent: Option<Weak<Mutex<Self>>>, scope: &NameString) -> Self {
        Self {
            scope_name: scope.clone(),
            parent,
            list: Vec::new(),
        }
    }

    fn add_item(&mut self, name: NameString, object: ObjectListItem) {
        self.list.push((name, object));
    }

    fn get_scope(&self, name: &NameString) -> Option<ObjectListItem> {
        self.list
            .iter()
            .find(|item| item.0 == *name && matches!(&item.1, ObjectListItem::DefScope(_)))
            .and_then(|item| Some(item.1.clone()))
    }
}

impl ParseHelper {
    pub fn new(
        current_root_term_list: TermList,
        root_term_list: Vec<TermList>,
        scope_name: &NameString,
    ) -> Self {
        assert!(!scope_name.is_null_name());
        assert!(!current_root_term_list.get_scope_name().is_null_name());
        let list = Arc::new(Mutex::new(ObjectList::new(None, scope_name)));
        Self {
            current_root_term_list,
            root_term_list_vec: Arc::new(root_term_list),
            current_object_list: list.clone(),
            root_object_list: list,
            original_name_searching: None,
            term_list_hierarchy: Vec::new(),
        }
    }

    pub fn init(&mut self) -> Result<(), AmlError> {
        self.move_current_scope(&NameString::root())?;

        /* Add builtin objects */
        let gl_name = NameString::from_array(&[*b"_GL\0"], true);
        let gl = NamedObject::DefMutex((gl_name.clone(), 0));
        let osi_name = NameString::from_array(&[*b"_OSI"], true);
        let osi =
            NamedObject::DefExternal(External::new(osi_name.clone(), 8 /*OK?(Method)*/, 1));
        let os_name = NameString::from_array(&[*b"_OS\0"], true);
        let os = DataRefObject::DataObject(DataObject::ComputationalData(
            ComputationalData::StringData(crate::OS_NAME),
        ));
        let rev_name = NameString::from_array(&[*b"_REV"], true);
        let rev = DataRefObject::DataObject(DataObject::ComputationalData(
            ComputationalData::ConstObj(2 /* ACPI 2.0 */),
        ));
        let dlm_name = NameString::from_array(&[*b"_DLM"], true);
        let dlm = DataRefObject::DataObject(DataObject::ComputationalData(
            ComputationalData::ConstObj(0 /* temporary */),
        ));

        self.add_named_object(&gl_name, &gl)?;
        self.add_named_object(&osi_name, &osi)?;
        self.add_def_name(&os_name, &os)?;
        self.add_def_name(&rev_name, &rev)?;
        self.add_def_name(&dlm_name, &dlm)?;
        return Ok(());
    }

    fn add_def_name(&mut self, name: &NameString, object: &DataRefObject) -> Result<(), AmlError> {
        Ok(self
            .current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .add_item(name.clone(), ObjectListItem::DefName(object.clone())))
    }

    fn add_alias_name(
        &mut self,
        name: &NameString,
        destination: &NameString,
    ) -> Result<(), AmlError> {
        Ok(self
            .current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .add_item(name.clone(), ObjectListItem::DefAlias(destination.clone())))
    }

    fn add_named_object(
        &mut self,
        name: &NameString,
        object: &NamedObject,
    ) -> Result<(), AmlError> {
        Ok(self
            .current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .add_item(name.clone(), ObjectListItem::NamedObject(object.clone())))
    }

    fn create_new_scope_and_move(&mut self, scope_name: &NameString) -> Result<(), AmlError> {
        let child_object_list = Arc::new(Mutex::new(ObjectList::new(
            Some(Arc::downgrade(&self.current_object_list)),
            scope_name,
        )));
        self.current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .add_item(
                scope_name.clone(),
                ObjectListItem::DefScope(child_object_list.clone()),
            );
        self.current_object_list = child_object_list;
        return Ok(());
    }

    fn move_current_scope(&mut self, scope_name: &NameString) -> Result<(), AmlError> {
        if scope_name.is_null_name() {
            return Ok(());
        }
        let locked_current_object_list = self
            .current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?;
        if &locked_current_object_list.scope_name == scope_name {
            return Ok(());
        }
        if locked_current_object_list.scope_name.is_child(scope_name) {
            if let Some(ObjectListItem::DefScope(scope)) =
                locked_current_object_list.get_scope(scope_name)
            {
                self.current_object_list = scope;
                return Ok(());
            }
            /* Search from the child tree */
            let mut searching_scope_name = locked_current_object_list.scope_name.clone();
            let mut searching_list = self.current_object_list.clone();
            let relative_name = scope_name
                .get_relative_name(&locked_current_object_list.scope_name)
                .unwrap();

            drop(locked_current_object_list);
            for index in 0..relative_name.len() {
                searching_scope_name = relative_name
                    .get_element_as_name_string(index)
                    .unwrap()
                    .get_full_name_path(&searching_scope_name);
                let result = searching_list
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    .get_scope(&searching_scope_name);
                if let Some(ObjectListItem::DefScope(scope)) = result {
                    self.current_object_list = scope.clone();
                    searching_list = scope;
                } else {
                    self.current_object_list = searching_list;
                    self.create_new_scope_and_move(&searching_scope_name)?;
                    searching_list = self.current_object_list.clone();
                }
            }
            return Ok(());
        }
        /* Search from the parent tree */
        let mut parent_list_option = locked_current_object_list.parent.clone();
        drop(locked_current_object_list);

        while let Some(parent_list_weak) = parent_list_option {
            let parent_list_arc = parent_list_weak
                .upgrade()
                .ok_or(AmlError::ObjectTreeError)?;
            if let Ok(locked_parent_list) = parent_list_arc.clone().try_lock() {
                if &locked_parent_list.scope_name == scope_name {
                    self.current_object_list = parent_list_arc;
                    return Ok(());
                }
                if locked_parent_list.scope_name.is_child(scope_name) {
                    drop(locked_parent_list);
                    self.current_object_list = parent_list_arc;
                    return self.move_current_scope(scope_name);
                }
                parent_list_option = locked_parent_list.parent.clone();
            } else {
                break;
            }
        }

        /* Search from root */
        self.current_object_list = self.root_object_list.clone();
        return self.move_current_scope(scope_name);
    }

    pub fn move_into_term_list(&mut self, term_list: TermList) -> Result<(), AmlError> {
        self.move_current_scope(term_list.get_scope_name())?;
        self.term_list_hierarchy.push(term_list);
        return Ok(());
    }

    pub fn move_out_from_current_term_list(&mut self) -> Result<(), AmlError> {
        self.term_list_hierarchy.pop();

        if let Some(name) = self
            .term_list_hierarchy
            .last()
            .and_then(|o| Some(o.get_scope_name().clone()))
        {
            self.move_current_scope(&name)?;
        } else {
            self.current_object_list = self.root_object_list.clone();
        }
        return Ok(());
    }

    fn find_target_scope(
        list: Arc<Mutex<ObjectList>>,
        name: &NameString,
    ) -> Result<Arc<Mutex<ObjectList>>, AmlError> {
        let locked_list = list.try_lock().or_else(|_| {
            pr_err!("Cannot Lock List.");
            Err(AmlError::MutexError)
        })?;
        for item in locked_list.list.iter() {
            if &item.0 == name {
                if let ObjectListItem::DefScope(scope) = &item.1 {
                    return Ok(scope.clone());
                }
            }
            if item.0.is_child(name) {
                if let ObjectListItem::DefScope(child) = &item.1 {
                    let child = child.clone();
                    drop(locked_list);
                    return Self::find_target_scope(child, name);
                }
            }
        }
        return Ok(list);
    }

    fn get_parent_object_list(
        list: Arc<Mutex<ObjectList>>,
    ) -> Result<Option<Arc<Mutex<ObjectList>>>, AmlError> {
        if let Some(p) = list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .parent
            .clone()
        {
            Ok(Some(p.upgrade().ok_or(AmlError::ObjectTreeError)?))
        } else {
            Ok(None)
        }
    }

    fn parse_term_list_recursive(
        &mut self,
        target_name: &NameString,
        mut term_list: TermList,
        search_scope: Option<&NameString>, /* To search like _SB.PCI0.^^_FOO */
        should_keep_term_list_hierarchy_when_found: bool,
    ) -> Result<Option<ObjectListItem>, AmlError> {
        if !term_list.get_scope_name().is_child(target_name)
            && search_scope
                .and_then(|s| Some(!term_list.get_scope_name().is_child(s)))
                .unwrap_or(true)
        {
            return Ok(None);
        }
        self.move_into_term_list(term_list.clone())?;
        let single_relative_path = target_name.get_single_name_path();
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

        while let Some(term_obj) = get_next_term_obj(&mut term_list, self)? {
            match term_obj {
                TermObj::NamespaceModifierObj(name_modifier_object) => {
                    if name_modifier_object.get_name() == target_name
                        || name_modifier_object.get_name().is_child(target_name)
                    {
                        match name_modifier_object {
                            NamespaceModifierObject::DefAlias(a) => {
                                if a.get_name().is_child(target_name) {
                                    /* SourceObject must be named object. */
                                    /* Ignore */
                                } else {
                                    /* Pop TermListHierarchy to re-search for the source object. */
                                    self.add_alias_name(a.get_name(), a.get_source())?;
                                    self.move_out_from_current_term_list()?;
                                    return Ok(Some(ObjectListItem::DefAlias(
                                        a.get_source().clone(),
                                    )));
                                }
                            }
                            NamespaceModifierObject::DefName(n) => {
                                if n.get_name() == target_name {
                                    self.add_def_name(n.get_name(), n.get_data_ref_object())?;
                                    if !should_keep_term_list_hierarchy_when_found {
                                        self.move_out_from_current_term_list()?;
                                    }
                                    return Ok(Some(ObjectListItem::DefName(
                                        n.get_data_ref_object().clone(),
                                    )));
                                }
                            }
                            NamespaceModifierObject::DefScope(s) => {
                                let result = self.parse_term_list_recursive(
                                    target_name,
                                    s.get_term_list().clone(),
                                    search_scope,
                                    should_keep_term_list_hierarchy_when_found,
                                );
                                match &result {
                                    Ok(Some(_)) => {
                                        if !should_keep_term_list_hierarchy_when_found {
                                            self.move_out_from_current_term_list()?;
                                        }
                                        return result;
                                    }
                                    Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
                                    Err(_) => {
                                        self.move_out_from_current_term_list()?;
                                        return result;
                                    }
                                };
                            }
                        }
                    } else if single_relative_path
                        .as_ref()
                        .and_then(|n| Some(name_modifier_object.get_name().suffix_search(n)))
                        .unwrap_or(false)
                    {
                        match name_modifier_object {
                            NamespaceModifierObject::DefAlias(a) => {
                                self.add_alias_name(a.get_name(), a.get_source())?;
                                self.move_out_from_current_term_list()?;
                                return Ok(Some(ObjectListItem::DefAlias(a.get_source().clone())));
                            }
                            NamespaceModifierObject::DefName(n) => {
                                self.add_def_name(n.get_name(), n.get_data_ref_object())?;
                                if !should_keep_term_list_hierarchy_when_found {
                                    self.move_out_from_current_term_list()?;
                                }
                                return Ok(Some(ObjectListItem::DefName(
                                    n.get_data_ref_object().clone(),
                                )));
                            }
                            NamespaceModifierObject::DefScope(_) => { /* Ignore */ }
                        }
                    }
                }
                TermObj::NamedObj(named_obj) => {
                    match self.parse_named_object_recursive(
                        target_name,
                        term_list.get_scope_name(),
                        named_obj,
                        search_scope,
                        should_keep_term_list_hierarchy_when_found,
                    ) {
                        Ok(Some(named_obj)) => {
                            if !should_keep_term_list_hierarchy_when_found {
                                self.move_out_from_current_term_list()?;
                            }
                            return Ok(Some(named_obj));
                        }
                        Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
                        Err(e) => {
                            self.move_out_from_current_term_list()?;
                            return Err(e);
                        }
                    }
                    self.move_current_scope(term_list.get_scope_name())?;
                }
                TermObj::StatementOpcode(_) => { /* Ignore */ }
                TermObj::ExpressionOpcode(_) => { /* Ignore */ }
            }
        }
        self.move_out_from_current_term_list()?;
        return Ok(None);
    }

    fn parse_named_object_recursive(
        &mut self,
        target_name: &NameString,
        current_scope: &NameString,
        named_object: NamedObject,
        search_scope: Option<&NameString>,
        should_keep_term_list_hierarchy_when_found: bool,
    ) -> Result<Option<ObjectListItem>, AmlError> {
        let single_path = target_name.get_single_name_path();

        if let Some(name) = named_object.get_name() {
            if name == target_name
                || single_path
                    .as_ref()
                    .and_then(|n| {
                        Some(current_scope.is_child(target_name) && name.suffix_search(n))
                    })
                    .unwrap_or(false)
            {
                self.add_named_object(name, &named_object)?;
                return Ok(Some(ObjectListItem::NamedObject(named_object)));
            }
        }
        if !target_name.is_single_relative_path_name()
            && !named_object
                .get_name()
                .unwrap_or(current_scope)
                .is_child(target_name)
        {
            return Ok(None);
        }

        if let Some(mut field_list) = named_object.get_field_list() {
            while let Some(e) = field_list.next()? {
                if let FieldElement::NameField((n, _)) = &e {
                    if n == target_name {
                        self.add_named_object(&n, &named_object)?;
                        return Ok(Some(ObjectListItem::NamedObject(named_object)));
                    } else if let Some(relative_name) = &single_path {
                        if current_scope.is_child(target_name) && n.suffix_search(relative_name) {
                            self.add_named_object(&n, &named_object)?;
                            return Ok(Some(ObjectListItem::NamedObject(named_object)));
                        }
                    }
                }
            }
            Ok(None)
        } else if let Some(term_list) = named_object.get_term_list() {
            self.parse_term_list_recursive(
                target_name,
                term_list,
                search_scope,
                should_keep_term_list_hierarchy_when_found,
            )
        } else {
            Ok(None)
        }
    }

    fn _search_object_from_list_with_parsing_term_list(
        &mut self,
        list: Arc<Mutex<ObjectList>>,
        target_name: &NameString,
        should_check_recursively: bool,
        should_keep_term_list_hierarchy_when_found: bool,
    ) -> Result<Option<ContentObject>, AmlError> {
        let mut locked_list = list.try_lock().or(Err(AmlError::MutexError))?;
        let mut list_to_search_later: Option<Arc<Mutex<ObjectList>>> = None;
        let single_relative_path = target_name.get_single_name_path();

        for index in 0.. {
            let (item_name, item) = if let Some(o) = locked_list.list.get(index) {
                o
            } else {
                break;
            };
            if item_name == target_name
                || single_relative_path
                    .as_ref()
                    .and_then(|n| Some(item_name.suffix_search(&n)))
                    .unwrap_or(false)
            {
                match item {
                    ObjectListItem::DefScope(_) => { /* Ignore */ }
                    ObjectListItem::DefName(d) => {
                        return Ok(Some(ContentObject::DataRefObject(d.clone())));
                    }
                    ObjectListItem::DefAlias(new_name) => {
                        let new_name = new_name.clone();
                        drop(locked_list);
                        return self.search_object(&new_name);
                    }
                    ObjectListItem::NamedObject(o) => {
                        return Ok(Some(ContentObject::NamedObject(o.clone())));
                    }
                }
            } else if item_name.is_child(target_name) {
                match item {
                    ObjectListItem::DefScope(s) => {
                        list_to_search_later = Some(s.clone());
                    }
                    ObjectListItem::DefName(_) => { /* Ignore */ }
                    ObjectListItem::DefAlias(_) => { /* Ignore */ }
                    ObjectListItem::NamedObject(n_o) => {
                        if should_check_recursively {
                            let n_o = n_o.clone();
                            let scope_name = locked_list.scope_name.clone();
                            drop(locked_list);

                            if let Some(o_i) = self.parse_named_object_recursive(
                                target_name,
                                &scope_name,
                                n_o,
                                None,
                                should_keep_term_list_hierarchy_when_found,
                            )? {
                                return self.convert_object_list_item_list_to_content_object(o_i);
                            }

                            locked_list = list.try_lock().or(Err(AmlError::MutexError))?;
                        }
                    }
                }
            }
        }

        drop(locked_list);
        if let Some(s) = list_to_search_later {
            self._search_object_from_list_with_parsing_term_list(
                s,
                target_name,
                should_check_recursively,
                should_keep_term_list_hierarchy_when_found,
            )
        } else {
            Ok(None)
        }
    }

    fn _search_object(
        &mut self,
        name: &NameString,
        should_enter_object: bool,
        preferred_search_scope: Option<&NameString>,
    ) -> Result<Option<ContentObject>, AmlError> {
        let back_up_of_original_name_searching =
            if let Some(searching) = self.original_name_searching.replace(name.clone()) {
                if name == &searching {
                    self.original_name_searching = Some(searching);
                    return Err(AmlError::NestedSearch);
                }
                Some(searching)
            } else {
                None
            };

        let current_scope_backup = if let Some(t) = self.term_list_hierarchy.last() {
            let t = t.clone();
            let locked_current_scope = self
                .current_object_list
                .try_lock()
                .or(Err(AmlError::MutexError))?;
            if t.get_scope_name() != &locked_current_scope.scope_name {
                drop(locked_current_scope);
                let backup = self.current_object_list.clone();
                self.move_current_scope(t.get_scope_name())?;
                backup
            } else {
                self.current_object_list.clone()
            }
        } else {
            self.current_object_list.clone()
        };

        let search_scope = if let Some(s) = preferred_search_scope {
            Some(s.clone())
        } else {
            self.term_list_hierarchy
                .last()
                .and_then(|s| Some(s.get_scope_name().clone()))
        };

        let target_list = if !should_enter_object {
            let is_child = self
                .current_object_list
                .try_lock()
                .or(Err(AmlError::MutexError))?
                .scope_name
                .is_child(name);

            if is_child {
                Self::find_target_scope(self.current_object_list.clone(), name)?
            } else {
                Self::find_target_scope(self.root_object_list.clone(), name)?
            }
        } else {
            self.root_object_list.clone()
        };

        let mut term_list_hierarchy_back_up: Vec<TermList> =
            Vec::with_capacity(self.term_list_hierarchy.len());

        if let Some(single_relative_path) = name.get_single_name_path() {
            let mut searching_list = Some(target_list.clone());
            for index in (0..self.term_list_hierarchy.len()).rev() {
                let term_list = self.term_list_hierarchy.get(index).unwrap().clone();
                let search_name =
                    single_relative_path.get_full_name_path(term_list.get_scope_name());
                /* Search from object list tree */
                if !should_enter_object {
                    if let Some(s) = &searching_list {
                        self.current_object_list = s.clone();
                        match self._search_object_from_list_with_parsing_term_list(
                            s.clone(),
                            &search_name,
                            true,
                            false,
                        ) {
                            Ok(Some(c)) => {
                                self.original_name_searching = back_up_of_original_name_searching;
                                self.current_object_list = current_scope_backup;
                                while let Some(e) = term_list_hierarchy_back_up.pop() {
                                    self.term_list_hierarchy.push(e);
                                }
                                return Ok(Some(c));
                            }
                            Ok(None) | Err(AmlError::NestedSearch) => {}
                            Err(e) => {
                                self.original_name_searching = back_up_of_original_name_searching;
                                self.current_object_list = current_scope_backup;
                                while let Some(e) = term_list_hierarchy_back_up.pop() {
                                    self.term_list_hierarchy.push(e);
                                }
                                return Err(e);
                            }
                        }
                        searching_list =
                            if let Some(parent) = Self::get_parent_object_list(s.clone())? {
                                Some(parent)
                            } else {
                                None
                            };
                    }
                }
                match self.parse_term_list_recursive(
                    &search_name,
                    term_list,
                    search_scope.as_ref(),
                    should_enter_object,
                ) {
                    Ok(Some(o_i)) => {
                        self.current_object_list = current_scope_backup;
                        self.original_name_searching = back_up_of_original_name_searching;
                        if !should_enter_object {
                            while let Some(e) = term_list_hierarchy_back_up.pop() {
                                self.term_list_hierarchy.push(e);
                            }
                        }
                        return self.convert_object_list_item_list_to_content_object(o_i);
                    }
                    Err(AmlError::NestedSearch) | Ok(None) => {}
                    Err(e) => {
                        self.current_object_list = current_scope_backup;
                        self.original_name_searching = back_up_of_original_name_searching;
                        while let Some(e) = term_list_hierarchy_back_up.pop() {
                            self.term_list_hierarchy.push(e);
                        }
                        return Err(e);
                    }
                }
                term_list_hierarchy_back_up.push(self.term_list_hierarchy.pop().unwrap());
            }
        } else if !should_enter_object {
            /* Search from Object List Tree */
            let temporary_backup = mem::replace(&mut self.current_object_list, target_list.clone());
            self.current_object_list = target_list.clone();
            if let Some(c) = self._search_object_from_list_with_parsing_term_list(
                target_list,
                name,
                true,
                false,
            )? {
                self.original_name_searching = back_up_of_original_name_searching;
                self.current_object_list = current_scope_backup;
                return Ok(Some(c));
            }
            while let Some(t) = self.term_list_hierarchy.pop() {
                term_list_hierarchy_back_up.push(t);
            }
            self.current_object_list = temporary_backup;
        }

        if self.term_list_hierarchy.len() != 0 {
            pr_err!(
                "TermListHierarchy is not empty => {:?}",
                self.term_list_hierarchy
            );
            return Err(AmlError::InvalidOperation);
        }

        /* Search from root_term_list including SSDT */
        self.current_object_list = self.root_object_list.clone();
        match self.parse_term_list_recursive(
            name,
            self.current_root_term_list.clone(),
            search_scope.as_ref(),
            should_enter_object,
        ) {
            Ok(Some(o_i)) => {
                self.current_object_list = current_scope_backup;
                self.original_name_searching = back_up_of_original_name_searching;
                if !should_enter_object {
                    while let Some(e) = term_list_hierarchy_back_up.pop() {
                        self.term_list_hierarchy.push(e);
                    }
                }
                return self.convert_object_list_item_list_to_content_object(o_i);
            }
            Err(AmlError::NestedSearch) | Ok(None) => {}
            Err(e) => {
                self.current_object_list = current_scope_backup;
                self.original_name_searching = back_up_of_original_name_searching;
                while let Some(e) = term_list_hierarchy_back_up.pop() {
                    self.term_list_hierarchy.push(e);
                }

                return Err(e);
            }
        }

        let term_list_vec = self.root_term_list_vec.clone();
        for t in term_list_vec.as_slice() {
            match self.parse_term_list_recursive(
                name,
                t.clone(),
                search_scope.as_ref(),
                should_enter_object,
            ) {
                Ok(Some(o_i)) => {
                    self.current_object_list = current_scope_backup;
                    self.original_name_searching = back_up_of_original_name_searching;
                    if !should_enter_object {
                        while let Some(e) = term_list_hierarchy_back_up.pop() {
                            self.term_list_hierarchy.push(e);
                        }
                    }
                    return self.convert_object_list_item_list_to_content_object(o_i);
                }
                Err(AmlError::NestedSearch) | Ok(None) => {}
                Err(e) => {
                    self.current_object_list = current_scope_backup;
                    self.original_name_searching = back_up_of_original_name_searching;
                    while let Some(e) = term_list_hierarchy_back_up.pop() {
                        self.term_list_hierarchy.push(e);
                    }

                    return Err(e);
                }
            }
        }

        self.current_object_list = current_scope_backup;
        self.original_name_searching = back_up_of_original_name_searching;
        Err(AmlError::InvalidMethodName(name.clone()))
    }

    /// Find Element with parsing Field and return the object including it.
    /// This function is the entrance of searching object.
    pub fn search_object(&mut self, name: &NameString) -> Result<Option<ContentObject>, AmlError> {
        if name.is_null_name() {
            return Ok(None);
        }
        self._search_object(name, false, None)
    }

    fn convert_object_list_item_list_to_content_object(
        &mut self,
        o_l_i: ObjectListItem,
    ) -> Result<Option<ContentObject>, AmlError> {
        match o_l_i {
            ObjectListItem::DefScope(s) => Ok(Some(ContentObject::Scope(
                s.try_lock()
                    .or(Err(AmlError::MutexError))?
                    .scope_name
                    .clone(),
            ))),
            ObjectListItem::DefName(d_n) => Ok(Some(ContentObject::DataRefObject(d_n))),
            ObjectListItem::DefAlias(a) => self.search_object(&a),
            ObjectListItem::NamedObject(n_o) => Ok(Some(ContentObject::NamedObject(n_o))),
        }
    }

    pub fn find_method_argument_count(
        &mut self,
        method_name: &NameString,
    ) -> Result<Option<AcpiInt>, AmlError> {
        if method_name.is_null_name() {
            return Ok(Some(0));
        }
        if let Some(result) = self.search_object(method_name)? {
            Ok(match result {
                ContentObject::DataRefObject(d_r) => match d_r {
                    DataRefObject::ObjectReference(_) => unimplemented!(),
                    DataRefObject::DataObject(_) => Some(0),
                },
                ContentObject::NamedObject(n_o) => n_o.get_argument_count(),
                ContentObject::Scope(_) => Some(0),
            })
        } else {
            Ok(None)
        }
    }

    fn _search_device(
        &mut self,
        hid: u32,
        mut term_list: TermList,
    ) -> Result<Option<Device>, AmlError> {
        while let Some(obj) = term_list.next(self)? {
            match obj {
                TermObj::NamespaceModifierObj(n_o) => match n_o {
                    NamespaceModifierObject::DefScope(scope) => {
                        self.move_into_term_list(scope.get_term_list().clone())?;
                        if let Some(d) = self._search_device(hid, scope.get_term_list().clone())? {
                            return Ok(Some(d));
                        }
                        self.move_out_from_current_term_list()?;
                    }
                    _ => { /* Ignore */ }
                },
                TermObj::NamedObj(n_o) => {
                    if let NamedObject::DefDevice(d) = n_o {
                        self.move_into_term_list(d.get_term_list().clone())?;
                        if d.get_hid(self)? == Some(hid) {
                            return Ok(Some(d));
                        }
                    } else {
                        if let Some(t) = n_o.get_term_list() {
                            self.move_into_term_list(t.clone())?;
                            if let Some(d) = self._search_device(hid, t)? {
                                return Ok(Some(d));
                            }
                            self.move_out_from_current_term_list()?;
                        }
                    }
                }
                TermObj::StatementOpcode(_) => { /* Ignore */ }
                TermObj::ExpressionOpcode(_) => { /* Ignore */ }
            }
        }
        Ok(None)
    }

    pub fn move_into_device(&mut self, hid: &[u8; 7]) -> Result<Option<Device>, AmlError> {
        let u32_hid = eisa_id_to_dword(hid);
        self.term_list_hierarchy.clear();
        self.current_object_list = self.root_object_list.clone();

        self._search_device(u32_hid, self.current_root_term_list.clone())
    }

    pub fn move_into_object(
        &mut self,
        name: &NameString,
        _term_list: Option<TermList>,
        search_scope: Option<&NameString>,
    ) -> Result<ContentObject, AmlError> {
        match self._search_object(name, true, search_scope) {
            Ok(Some(o_i)) => {
                if let ContentObject::Scope(scope) = o_i {
                    pr_err!("Expected a method, but found Scope({}).", scope);
                    Err(AmlError::InvalidType)
                } else if let ContentObject::DataRefObject(d) = o_i {
                    pr_err!("Expected a method, but found {:?}.", d);
                    Err(AmlError::InvalidType)
                } else {
                    Ok(o_i)
                }
            }
            Ok(None) => Err(AmlError::InvalidMethodName(name.clone())),
            Err(e) => Err(e),
        }
    }

    #[allow(dead_code)]
    pub fn move_into_object_old(
        &mut self,
        name: &NameString,
        term_list: Option<TermList>,
        search_scope: Option<&NameString>,
    ) -> Result<ContentObject, AmlError> {
        let f = |p: &mut Self,
                 term_list: TermList,
                 search_scope: Option<&NameString>|
         -> Result<Option<ContentObject>, AmlError> {
            match p.parse_term_list_recursive(name, term_list, search_scope, true) {
                Ok(Some(o_i)) => {
                    if let ObjectListItem::DefScope(scope) = o_i {
                        pr_err!(
                            "Expected a method, but found Scope({}).",
                            scope.try_lock().or(Err(AmlError::MutexError))?.scope_name
                        );
                        Err(AmlError::InvalidType)
                    } else if let ObjectListItem::DefAlias(alias) = o_i {
                        pr_err!("Expected a method, but found Alias({}).", alias);
                        Err(AmlError::InvalidType)
                    } else {
                        p.convert_object_list_item_list_to_content_object(o_i)
                    }
                }
                Ok(None) => Ok(None),
                Err(e) => Err(e),
            }
        };

        /* Search from Current Scope */
        if let Some(t) = term_list {
            if let Some(m) = f(self, t, search_scope)? {
                return Ok(m);
            }
        }

        /* Search from Current Root */
        self.current_object_list = self.root_object_list.clone();
        self.term_list_hierarchy.clear();

        if let Some(m) = f(self, self.current_root_term_list.clone(), search_scope)? {
            return Ok(m);
        }

        /* Search from SSDTs. */
        let term_list_vec = self.root_term_list_vec.clone();
        for t in term_list_vec.as_slice() {
            if let Some(m) = f(self, t.clone(), search_scope)? {
                return Ok(m);
            }
        }

        Err(AmlError::InvalidMethodName(name.clone()))
    }
}

impl core::fmt::Debug for ParseHelper {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        if let Ok(l) = self.current_object_list.try_lock() {
            f.write_fmt(format_args!("ParseHelper(Scope:{})", l.scope_name))
        } else {
            f.write_str("ParseHelper(Scope:MutexError)")
        }
    }
}
