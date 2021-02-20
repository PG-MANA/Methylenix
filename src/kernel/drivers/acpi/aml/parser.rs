//!
//! ACPI Machine Language Parse Helper
//!

use super::data_object::{ComputationalData, DataObject, DataRefObject, NameString};
use super::named_object::{External, FieldElement, NamedObject};
use super::namespace_modifier_object::NamespaceModifierObject;
use super::statement_opcode::StatementOpcode;
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
}

#[derive(Clone)]
pub struct ParseHelper {
    root_term_list: TermList,
    root_object_list: Arc<Mutex<ObjectList>>,
    current_object_list: Arc<Mutex<ObjectList>>,
    original_name_searching: Option<NameString>,
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

    fn get_item(&self, name: &NameString, should_ignore_scope: bool) -> Option<ObjectListItem> {
        self.list
            .iter()
            .find(|item| {
                &item.0 == name
                    && (!should_ignore_scope || !matches!(item.1, ObjectListItem::DefScope(_)))
            })
            .and_then(|item| Some(item.1.clone()))
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
        }
    }

    pub fn init(&mut self) -> Result<(), AmlError> {
        self.move_current_scope(&NameString::root())?;

        /* Add builtin objects */
        let gl_name = NameString::from_array(&[['_' as u8, 'G' as u8, 'L' as u8, 0]], true);
        let gl = NamedObject::DefMutex((gl_name.clone(), 0));
        let osi_name =
            NameString::from_array(&[['_' as u8, 'O' as u8, 'S' as u8, 'I' as u8]], true);
        let osi =
            NamedObject::DefExternal(External::new(osi_name.clone(), 8 /*OK?(Method)*/, 1));
        let os_name = NameString::from_array(&[['_' as u8, 'O' as u8, 'S' as u8, 0]], true);
        let os = DataRefObject::DataObject(DataObject::ComputationalData(
            ComputationalData::StringData("Methylenix"),
        ));
        let rev_name =
            NameString::from_array(&[['_' as u8, 'R' as u8, 'E' as u8, 'V' as u8]], true);
        let rev = DataRefObject::DataObject(DataObject::ComputationalData(
            ComputationalData::ConstObj(2 /* ACPI 2.0 */),
        ));
        let dlm_name =
            NameString::from_array(&[['_' as u8, 'D' as u8, 'L' as u8, 'M' as u8]], true);
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

    pub fn move_current_scope(&mut self, scope_name: &NameString) -> Result<(), AmlError> {
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
            let mut parent_list_option = locked_current_object_list.parent.clone();
            drop(locked_current_object_list);
            while let Some(parent_list) = parent_list_option {
                if let Ok(locked_parent_list) = parent_list
                    .clone()
                    .upgrade()
                    .ok_or(AmlError::ObjectTreeError)?
                    .try_lock()
                {
                    if locked_parent_list.scope_name.is_child(scope_name) {
                        drop(locked_parent_list);
                        let target_list = Self::find_target_scope(
                            parent_list
                                .clone()
                                .upgrade()
                                .ok_or(AmlError::ObjectTreeError)?,
                            scope_name,
                        );

                        if matches!(target_list, Err(AmlError::MutexError)) {
                            break;
                        }
                        let target_list = target_list?;
                        let locked_target_list =
                            target_list.try_lock().or(Err(AmlError::MutexError))?;
                        return if &locked_target_list.scope_name == scope_name {
                            self.current_object_list = target_list;
                            Ok(())
                        } else {
                            drop(locked_target_list);
                            self.current_object_list = target_list;
                            self.create_new_scope_and_move(scope_name)
                        };
                    }
                    if let Ok(locked_parent_list) = parent_list
                        .upgrade()
                        .ok_or(AmlError::ObjectTreeError)?
                        .try_lock()
                    {
                        parent_list_option = locked_parent_list.parent.clone();
                        continue;
                    }
                }
                break;
            }
        } else {
            drop(locked_current_object_list);
        }
        /* Find scope from root */
        self.current_object_list =
            Self::find_target_scope(self.root_object_list.clone(), scope_name)?;
        if &self
            .current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .scope_name
            == scope_name
        {
            return Ok(());
        }
        return self.create_new_scope_and_move(scope_name);
    }

    pub fn move_parent_scope(&mut self) -> Result<bool, AmlError> {
        if let Some(p) = self
            .current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .parent
            .clone()
        {
            self.current_object_list = p.upgrade().ok_or(AmlError::ObjectTreeError)?;
            return Ok(true);
        }
        return Ok(false);
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

    fn find_item_from_list(
        list: Arc<Mutex<ObjectList>>,
        name: &NameString,
        should_ignore_scope: bool,
    ) -> Result<Option<ObjectListItem>, AmlError> {
        Ok(list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .get_item(name, should_ignore_scope))
    }

    fn find_item(
        &self,
        name: &NameString,
        should_ignore_scope: bool,
        should_search_parent: bool,
    ) -> Result<Option<ObjectListItem>, AmlError> {
        if let Some(item) = Self::find_item_from_list(
            Self::find_target_scope(self.current_object_list.clone(), name)?,
            name,
            should_ignore_scope,
        )? {
            return Ok(Some(item));
        }
        let target_list = Self::find_target_scope(self.root_object_list.clone(), name)?;
        if let Some(item) =
            Self::find_item_from_list(target_list.clone(), name, should_ignore_scope)?
        {
            return Ok(Some(item));
        }
        if should_search_parent {
            let parent = target_list
                .try_lock()
                .or(Err(AmlError::MutexError))?
                .parent
                .clone();

            if let Some(parent) = parent {
                if let Some(item) = Self::find_item_from_list(
                    parent.upgrade().ok_or(AmlError::ObjectTreeError)?,
                    name,
                    should_ignore_scope,
                )? {
                    return Ok(Some(item));
                }
            }
        }
        Ok(None)
    }

    pub fn find_content_object(
        &self,
        name: &NameString,
    ) -> Result<Option<ContentObject>, AmlError> {
        if name.is_null_name() {
            return Ok(None);
        }
        if let Some(item) = self.find_item(name, true, true)? {
            match item {
                ObjectListItem::NamedObject(o) => Ok(Some(ContentObject::NamedObject(o))),
                ObjectListItem::DefName(d) => Ok(Some(ContentObject::DataRefObject(d))),
                ObjectListItem::DefAlias(new_name) => self.find_content_object(&new_name),
                ObjectListItem::DefScope(_) => unreachable!(),
            }
        } else {
            Ok(None)
        }
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

    fn _find_content_object_with_parsing(
        &mut self,
        list: Arc<Mutex<ObjectList>>,
        name: &NameString,
        relative_name: Option<&NameString>,
    ) -> Result<Option<ContentObject>, AmlError> {
        let mut locked_list = list.try_lock().or(Err(AmlError::MutexError))?;
        let mut list_to_search_later: Option<Arc<Mutex<ObjectList>>> = None;

        for index in 0.. {
            let i = locked_list.list.get(index);
            if i.is_none() {
                break;
            }
            let i = i.unwrap();

            if &i.0 == name {
                if let ObjectListItem::DefScope(s) = &i.1 {
                    list_to_search_later = Some(s.clone());
                } else {
                    let object = i.1.clone();
                    drop(locked_list);
                    return match object {
                        ObjectListItem::NamedObject(o) => Ok(Some(ContentObject::NamedObject(o))),
                        ObjectListItem::DefName(d) => Ok(Some(ContentObject::DataRefObject(d))),
                        ObjectListItem::DefAlias(new_name) => {
                            self.find_content_object_with_parsing(&new_name)
                        }
                        ObjectListItem::DefScope(_) => unreachable!(),
                    };
                }
            } else if i.0.is_child(name) {
                match &i.1 {
                    ObjectListItem::DefScope(s) => {
                        list_to_search_later = Some(s.clone());
                    }
                    ObjectListItem::DefName(d_r) => match d_r {
                        DataRefObject::DataObject(d_o) => match d_o {
                            DataObject::ComputationalData(_) => { /* Ignore */ }
                            DataObject::DefPackage(_) => unimplemented!(),
                            DataObject::DefVarPackage(_) => unimplemented!(),
                        },
                        DataRefObject::ObjectReference(_) => {
                            unimplemented!()
                        }
                    },
                    ObjectListItem::DefAlias(_) => unimplemented!(),
                    ObjectListItem::NamedObject(n_o) => {
                        let n_o = n_o.clone();
                        let scope_name = locked_list.scope_name.clone();
                        drop(locked_list);

                        if let Some(o_i) = self.parse_named_object_recursive(
                            name,
                            &scope_name,
                            n_o,
                            relative_name,
                            false,
                        )? {
                            return self.convert_object_list_item_list_to_content_object(o_i);
                        }

                        locked_list = list.try_lock().or(Err(AmlError::MutexError))?;
                    }
                }
            } else if let Some(relative_name) = relative_name {
                if i.0.suffix_search(relative_name) {
                    if let ObjectListItem::DefScope(_) = &i.1 {
                        unreachable!(); /* TODO: Think if it is ok. */
                    } else {
                        let object = i.1.clone();
                        drop(locked_list);
                        return match object {
                            ObjectListItem::NamedObject(o) => {
                                Ok(Some(ContentObject::NamedObject(o)))
                            }
                            ObjectListItem::DefName(d) => Ok(Some(ContentObject::DataRefObject(d))),
                            ObjectListItem::DefAlias(new_name) => {
                                self.find_content_object_with_parsing(&new_name)
                            }
                            ObjectListItem::DefScope(_) => unreachable!(),
                        };
                    }
                }
            }
        }
        drop(locked_list);
        if let Some(s) = list_to_search_later {
            self._find_content_object_with_parsing(s, name, relative_name)
        } else {
            Ok(None)
        }
    }

    /// Find Element with parsing Field and return the object including it.
    /// This function is the entrance of searching object.
    pub fn find_content_object_with_parsing(
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
        if let Some(o) = self.find_content_object(name)? {
            /* may be needless? */
            self.original_name_searching = back_up_of_original_name_searching;
            return Ok(Some(o));
        }
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

        let relative_target_name = name.get_relative_name(
            &original_target_list
                .try_lock()
                .or(Err(AmlError::MutexError))?
                .scope_name,
        );

        if let Some(c) = self._find_content_object_with_parsing(
            original_target_list.clone(),
            name,
            relative_target_name
                .as_ref()
                .and_then(|r| if r.len() == 1 { Some(r) } else { None }),
        )? {
            self.original_name_searching = back_up_of_original_name_searching;
            return Ok(Some(c));
        }
        let mut target_list = original_target_list.clone();
        let back_up = self.back_up();

        if let Some(relative_target_name) = relative_target_name.as_ref() {
            if relative_target_name.len() == 1 {
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
                    if let Some(c) = self._find_content_object_with_parsing(
                        target_list.clone(),
                        &search_name,
                        Some(relative_target_name),
                    )? {
                        self.restore(back_up.clone());
                        self.original_name_searching = back_up_of_original_name_searching;
                        return Ok(Some(c));
                    }
                }
                self.restore(back_up.clone());
            }
        }

        match self.parse_term_list_recursive(
            name,
            self.root_term_list.clone(),
            relative_target_name
                .as_ref()
                .and_then(|r| if r.len() == 1 { Some(r) } else { None }),
            false,
        ) {
            Ok(Some(o_i)) => {
                self.restore(back_up);
                self.original_name_searching = back_up_of_original_name_searching;
                return self.convert_object_list_item_list_to_content_object(o_i);
            }
            Err(AmlError::NestedSearch) | Ok(None) => {}
            Err(e) => {
                return Err(e);
            }
        }
        self.restore(back_up.clone());

        if let Some(relative_target_name) = relative_target_name.as_ref() {
            if relative_target_name.len() == 1 {
                let mut target_list = original_target_list;
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
                    match self.parse_term_list_recursive(
                        &search_name,
                        self.root_term_list.clone(),
                        Some(relative_target_name),
                        false,
                    ) {
                        Ok(Some(o_i)) => {
                            self.restore(back_up);
                            self.original_name_searching = back_up_of_original_name_searching;
                            return self.convert_object_list_item_list_to_content_object(o_i);
                        }
                        Err(AmlError::NestedSearch) | Ok(None) => {}
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
            }
            /* TODO: delete this extreme process. */
            match self.parse_term_list_recursive(
                &name,
                self.root_term_list.clone(),
                Some(relative_target_name),
                true,
            ) {
                Ok(Some(o_i)) => {
                    self.restore(back_up);
                    self.original_name_searching = back_up_of_original_name_searching;
                    return self.convert_object_list_item_list_to_content_object(o_i);
                }
                Err(AmlError::NestedSearch) | Ok(None) => {}
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok(None)
    }

    fn convert_object_list_item_list_to_content_object(
        &mut self,
        o_l_i: ObjectListItem,
    ) -> Result<Option<ContentObject>, AmlError> {
        match o_l_i {
            ObjectListItem::DefScope(_) => {
                unimplemented!()
            }
            ObjectListItem::DefName(d_n) => {
                return Ok(Some(ContentObject::DataRefObject(d_n)));
            }
            ObjectListItem::DefAlias(a) => {
                return self.find_content_object_with_parsing(&a);
            }
            ObjectListItem::NamedObject(n_o) => {
                return Ok(Some(ContentObject::NamedObject(n_o)));
            }
        }
    }

    pub fn find_method_argument_count(
        &mut self,
        method_name: &NameString,
    ) -> Result<Option<AcpiInt>, AmlError> {
        if method_name.is_null_name() {
            return Ok(Some(0));
        }
        let result = self.find_content_object_with_parsing(method_name)?;
        if result.is_none() {
            return Ok(None);
        }
        Ok(match result.unwrap() {
            ContentObject::DataRefObject(d_r) => match d_r {
                DataRefObject::ObjectReference(_) => unimplemented!(),
                DataRefObject::DataObject(_) => Some(0),
            },
            ContentObject::NamedObject(n_o) => n_o.get_argument_count(),
        })
    }

    fn parse_term_list_recursive(
        &mut self,
        target_name: &NameString,
        mut term_list: TermList,
        relative_name: Option<&NameString>,
        disable_scope_check: bool,
    ) -> Result<Option<ObjectListItem>, AmlError> {
        if !term_list.get_scope_name().is_child(target_name) && !disable_scope_check {
            return Ok(None);
        }
        self.move_current_scope(term_list.get_scope_name())?;
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
                                if s.get_name() == target_name {
                                    continue;
                                }
                                if let Some(obj) = self.parse_term_list_recursive(
                                    target_name,
                                    s.get_term_list().clone(),
                                    relative_name,
                                    disable_scope_check,
                                )? {
                                    return Ok(Some(obj));
                                }
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
                    if let Some(named_obj) = self.parse_named_object_recursive(
                        target_name,
                        term_list.get_scope_name(),
                        named_obj,
                        relative_name,
                        disable_scope_check,
                    )? {
                        return Ok(Some(named_obj));
                    }
                }
                TermObj::StatementOpcode(statement_op) => {
                    match statement_op {
                        /* TODO: Check the target object is if should in the statement. */
                        StatementOpcode::DefIfElse(ie) => {
                            if let Some(obj) = self.parse_term_list_recursive(
                                target_name,
                                ie.get_if_true_term_list().clone(),
                                relative_name,
                                disable_scope_check,
                            )? {
                                return Ok(Some(obj));
                            }
                            if let Some(e_t) = ie.get_if_false_term_list() {
                                if let Some(obj) = self.parse_term_list_recursive(
                                    target_name,
                                    e_t.clone(),
                                    relative_name,
                                    disable_scope_check,
                                )? {
                                    return Ok(Some(obj));
                                }
                            }
                        }
                        StatementOpcode::DefWhile(w) => {
                            if let Some(obj) = self.parse_term_list_recursive(
                                target_name,
                                w.get_term_list().clone(),
                                relative_name,
                                disable_scope_check,
                            )? {
                                return Ok(Some(obj));
                            }
                        }
                        _ => { /* Ignore */ }
                    }
                }
                TermObj::ExpressionOpcode(_) => { /* Ignore */ }
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
        disable_scope_check: bool,
    ) -> Result<Option<ObjectListItem>, AmlError> {
        if let Some(name) = named_object.get_name() {
            if name == target_name {
                return Ok(Some(ObjectListItem::NamedObject(named_object)));
            } else if let Some(relative_name) = relative_name {
                if name.suffix_search(relative_name) {
                    return Ok(Some(ObjectListItem::NamedObject(named_object)));
                }
            }
        }
        if !named_object
            .get_name()
            .unwrap_or(current_scope)
            .is_child(target_name)
            && relative_name.is_none()
            && !disable_scope_check
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
                        if !current_scope.is_child(n) && n.suffix_search(relative_name) {
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
                relative_name,
                disable_scope_check,
            )
        } else {
            Ok(None)
        }
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
