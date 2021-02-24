//!
//! ACPI Machine Language Parse Helper
//!

use super::data_object::{ComputationalData, DataObject, DataRefObject};
use super::name_object::NameString;
use super::named_object::{External, FieldElement, NamedObject};
use super::namespace_modifier_object::NamespaceModifierObject;
use super::term_object::{TermList, TermObj};
use super::{AcpiInt, AmlError};

use crate::kernel::sync::spin_lock::Mutex;

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

#[derive(Clone)]
enum ObjectListItem {
    DefScope(Arc<Mutex<ObjectList>>),
    DefName(DataRefObject),
    DefAlias(NameString),
    NamedObject(NamedObject),
}

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

pub struct ParseHelper {
    root_term_list: TermList,
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
    pub fn new(root_term_list: TermList, scope_name: &NameString) -> Self {
        assert!(!scope_name.is_null_name());
        assert!(!root_term_list.get_scope_name().is_null_name());
        let list = Arc::new(Mutex::new(ObjectList::new(None, scope_name)));
        Self {
            root_term_list,
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

    fn back_up(&self) -> Arc<Mutex<ObjectList>> {
        self.current_object_list.clone()
    }

    fn restore(&mut self, back_up: Arc<Mutex<ObjectList>>) {
        self.current_object_list = back_up;
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
        relative_name: Option<&NameString>,
    ) -> Result<Option<ObjectListItem>, AmlError> {
        if !term_list.get_scope_name().is_child(target_name) {
            return Ok(None);
        }
        self.move_current_scope(term_list.get_scope_name())?;
        let mut matched_scope: Option<NameString> = None;

        while let Some(term_obj) = term_list.next(self)? {
            match term_obj {
                TermObj::NamespaceModifierObj(name_modifier_object) => {
                    if name_modifier_object.get_name() == target_name
                        || name_modifier_object.get_name().is_child(target_name)
                    {
                        match name_modifier_object {
                            NamespaceModifierObject::DefAlias(a) => {
                                if a.get_name().is_child(target_name) {
                                    unimplemented!()
                                }
                                self.add_alias_name(a.get_name(), a.get_source())?;
                                return Ok(Some(ObjectListItem::DefAlias(a.get_source().clone())));
                            }
                            NamespaceModifierObject::DefName(n) => {
                                if !n.get_name().is_child(target_name) {
                                    self.add_def_name(n.get_name(), n.get_data_ref_object())?;
                                    return Ok(Some(ObjectListItem::DefName(
                                        n.get_data_ref_object().clone(),
                                    )));
                                }
                            }
                            NamespaceModifierObject::DefScope(s) => {
                                if s.get_name() == target_name
                                    || relative_name
                                        .and_then(|r| Some(r == s.get_name()))
                                        .unwrap_or(false)
                                {
                                    matched_scope = Some(s.get_name().clone());
                                    continue;
                                }
                                self.move_into_term_list(s.get_term_list().clone())?;
                                let result = self.parse_term_list_recursive(
                                    target_name,
                                    s.get_term_list().clone(),
                                    relative_name,
                                );
                                self.move_out_from_current_term_list()?;
                                match result {
                                    Ok(Some(o)) => return Ok(Some(o)),
                                    Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
                                    Err(e) => return Err(e),
                                };
                                self.move_current_scope(term_list.get_scope_name())?;
                            }
                        }
                    } else if let Some(relative_name) = relative_name {
                        if name_modifier_object.get_name().suffix_search(relative_name) {
                            match name_modifier_object {
                                NamespaceModifierObject::DefAlias(a) => {
                                    self.add_alias_name(a.get_name(), a.get_source())?;
                                    return Ok(Some(ObjectListItem::DefAlias(
                                        a.get_source().clone(),
                                    )));
                                }
                                NamespaceModifierObject::DefName(n) => {
                                    self.add_def_name(n.get_name(), n.get_data_ref_object())?;
                                    return Ok(Some(ObjectListItem::DefName(
                                        n.get_data_ref_object().clone(),
                                    )));
                                }
                                NamespaceModifierObject::DefScope(_) => {
                                    unimplemented!();
                                }
                            }
                        }
                    }
                }
                TermObj::NamedObj(named_obj) => {
                    match self.parse_named_object_recursive(
                        target_name,
                        term_list.get_scope_name(),
                        named_obj,
                        relative_name,
                    ) {
                        Ok(Some(named_obj)) => return Ok(Some(named_obj)),
                        Ok(None) | Err(AmlError::NestedSearch) => { /* Continue */ }
                        Err(e) => return Err(e),
                    }
                    self.move_current_scope(term_list.get_scope_name())?;
                }
                TermObj::StatementOpcode(_) => { /* Ignore */ }
                TermObj::ExpressionOpcode(_) => { /* Ignore */ }
            }
        }

        if let Some(scope) = matched_scope {
            let target_scope_list =
                Self::find_target_scope(self.current_object_list.clone(), &scope)?;
            if target_scope_list
                .try_lock()
                .or(Err(AmlError::MutexError))?
                .scope_name
                == scope
            {
                return Ok(Some(ObjectListItem::DefScope(target_scope_list)));
            }
        }
        Ok(None)
    }

    fn parse_named_object_recursive(
        &mut self,
        target_name: &NameString,
        current_scope: &NameString,
        named_object: NamedObject,
        relative_name: Option<&NameString>,
    ) -> Result<Option<ObjectListItem>, AmlError> {
        if let Some(name) = named_object.get_name() {
            if name == target_name {
                return Ok(Some(ObjectListItem::NamedObject(named_object)));
            } else if let Some(relative_name) = relative_name {
                if current_scope.is_child(target_name) && name.suffix_search(relative_name) {
                    return Ok(Some(ObjectListItem::NamedObject(named_object)));
                }
            }
        }
        if relative_name.is_none()
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
                    } else if let Some(relative_name) = relative_name {
                        if current_scope.is_child(target_name) && n.suffix_search(relative_name) {
                            self.add_named_object(&n, &named_object)?;
                            return Ok(Some(ObjectListItem::NamedObject(named_object)));
                        }
                    }
                }
            }
            Ok(None)
        } else if let Some(term_list) = named_object.get_term_list() {
            self.move_into_term_list(term_list.clone())?;
            let result = self.parse_term_list_recursive(target_name, term_list, relative_name);
            self.move_out_from_current_term_list()?;
            result
        } else {
            Ok(None)
        }
    }

    fn _search_object_from_list_with_parsing_term_list(
        &mut self,
        list: Arc<Mutex<ObjectList>>,
        name: &NameString,
        relative_name: Option<&NameString>,
        should_check_recursively: bool,
    ) -> Result<Option<ContentObject>, AmlError> {
        let mut locked_list = list.try_lock().or(Err(AmlError::MutexError))?;
        let mut list_to_search_later: Option<Arc<Mutex<ObjectList>>> = None;

        for index in 0.. {
            let (item_name, item) = match locked_list.list.get(index) {
                Some(o) => o,
                None => break,
            };
            if item_name == name
                || relative_name
                    .and_then(|r| Some(item_name.suffix_search(r)))
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
                        return self.search_object_from_list_with_parsing_term_list(&new_name);
                    }
                    ObjectListItem::NamedObject(o) => {
                        return Ok(Some(ContentObject::NamedObject(o.clone())));
                    }
                }
            } else if item_name.is_child(name) {
                match item {
                    ObjectListItem::DefScope(s) => {
                        list_to_search_later = Some(s.clone());
                    }
                    ObjectListItem::DefName(_) => { /* Ignore */ }
                    ObjectListItem::DefAlias(_) => unimplemented!(),
                    ObjectListItem::NamedObject(n_o) => {
                        if should_check_recursively {
                            let n_o = n_o.clone();
                            let scope_name = locked_list.scope_name.clone();
                            drop(locked_list);

                            if let Some(o_i) = self.parse_named_object_recursive(
                                name,
                                &scope_name,
                                n_o,
                                relative_name,
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
                name,
                relative_name,
                should_check_recursively,
            )
        } else {
            Ok(None)
        }
    }

    /// Find Element with parsing Field and return the object including it.
    /// This function is the entrance of searching object.
    pub fn search_object_from_list_with_parsing_term_list(
        &mut self,
        name: &NameString,
    ) -> Result<Option<ContentObject>, AmlError> {
        if name.is_null_name() {
            return Ok(None);
        }
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

        let is_child = self
            .current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .scope_name
            .is_child(name);

        let original_target_list = if is_child {
            Self::find_target_scope(self.current_object_list.clone(), name)?
        } else {
            Self::find_target_scope(self.root_object_list.clone(), name)?
        };

        let relative_target_name = name
            .get_relative_name(
                &self
                    .current_object_list
                    .try_lock()
                    .or(Err(AmlError::MutexError))?
                    .scope_name,
            )
            .and_then(|r| if r.len() == 1 { Some(r) } else { None });

        /* Search from object list tree */
        if let Some(c) = self._search_object_from_list_with_parsing_term_list(
            original_target_list.clone(),
            name,
            relative_target_name.as_ref(),
            false,
        )? {
            self.original_name_searching = back_up_of_original_name_searching;
            return Ok(Some(c));
        }

        /* Search parent object lists */
        if let Some(relative_target_name) = relative_target_name.as_ref() {
            let mut target_list = original_target_list.clone();
            let back_up = self.back_up();

            loop {
                if let Some(parent) = Self::get_parent_object_list(target_list)? {
                    target_list = parent;
                } else {
                    break;
                };
                let search_name = relative_target_name.get_full_name_path(
                    &target_list
                        .try_lock()
                        .or(Err(AmlError::MutexError))?
                        .scope_name,
                );

                self.current_object_list = target_list.clone();
                if let Some(c) = self._search_object_from_list_with_parsing_term_list(
                    target_list.clone(),
                    &search_name,
                    Some(relative_target_name),
                    false,
                )? {
                    self.restore(back_up.clone());
                    self.original_name_searching = back_up_of_original_name_searching;
                    return Ok(Some(c));
                }
            }
            self.restore(back_up.clone());
        }

        /*Search from term_list_hierarchy */
        let back_up = self.back_up();
        let mut term_list_hierarchy_back_up: Vec<TermList> =
            Vec::with_capacity(self.term_list_hierarchy.len());

        for index in (0..self.term_list_hierarchy.len()).rev() {
            let term_list = self.term_list_hierarchy.get(index).unwrap().clone();
            let search_name = relative_target_name
                .as_ref()
                .and_then(|r| Some(r.get_full_name_path(term_list.get_scope_name())));

            match self.parse_term_list_recursive(
                search_name.as_ref().unwrap_or(name),
                term_list,
                relative_target_name.as_ref(),
            ) {
                Ok(Some(o_i)) => {
                    self.restore(back_up);
                    while let Some(e) = term_list_hierarchy_back_up.pop() {
                        self.term_list_hierarchy.push(e);
                    }
                    self.original_name_searching = back_up_of_original_name_searching;
                    return self.convert_object_list_item_list_to_content_object(o_i);
                }
                Err(AmlError::NestedSearch) | Ok(None) => {}
                Err(e) => {
                    self.restore(back_up);
                    while let Some(e) = term_list_hierarchy_back_up.pop() {
                        self.term_list_hierarchy.push(e);
                    }
                    self.original_name_searching = back_up_of_original_name_searching;
                    return Err(e);
                }
            }
            term_list_hierarchy_back_up.push(self.term_list_hierarchy.pop().unwrap());
        }
        assert_eq!(
            self.term_list_hierarchy.len(),
            0,
            "TermListHierarchy is not empty => {:?}",
            self.term_list_hierarchy
        );

        self.restore(back_up.clone());

        /* Search from root */
        match self.parse_term_list_recursive(
            name,
            self.root_term_list.clone(),
            relative_target_name.as_ref(),
        ) {
            Ok(Some(o_i)) => {
                self.restore(back_up);
                self.term_list_hierarchy.clear();
                while let Some(e) = term_list_hierarchy_back_up.pop() {
                    self.term_list_hierarchy.push(e);
                }
                self.original_name_searching = back_up_of_original_name_searching;
                return self.convert_object_list_item_list_to_content_object(o_i);
            }
            Err(AmlError::NestedSearch) | Ok(None) => {}
            Err(e) => {
                self.restore(back_up);
                self.term_list_hierarchy.clear();
                while let Some(e) = term_list_hierarchy_back_up.pop() {
                    self.term_list_hierarchy.push(e);
                }
                self.original_name_searching = back_up_of_original_name_searching;
                return Err(e);
            }
        }
        self.restore(back_up);
        self.term_list_hierarchy.clear();
        while let Some(e) = term_list_hierarchy_back_up.pop() {
            self.term_list_hierarchy.push(e);
        }
        self.original_name_searching = back_up_of_original_name_searching;
        return Ok(None);
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
            ObjectListItem::DefAlias(a) => self.search_object_from_list_with_parsing_term_list(&a),
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
        let result = self.search_object_from_list_with_parsing_term_list(method_name)?;
        if result.is_none() {
            return Ok(None);
        }
        Ok(match result.unwrap() {
            ContentObject::DataRefObject(d_r) => match d_r {
                DataRefObject::ObjectReference(_) => unimplemented!(),
                DataRefObject::DataObject(_) => Some(0),
            },
            ContentObject::NamedObject(n_o) => n_o.get_argument_count(),
            ContentObject::Scope(_) => Some(0),
        })
    }
}

impl core::fmt::Debug for ParseHelper {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Ok(l) = self.current_object_list.try_lock() {
            f.write_fmt(format_args!("ParseHelper(Scope:{})", l.scope_name))
        } else {
            f.write_str("ParseHelper(Scope:MutexError)")
        }
    }
}
