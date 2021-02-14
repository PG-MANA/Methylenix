//!
//! ACPI Machine Language Parse Helper
//!

use super::data_object::{DataRefObject, NameString};
use super::named_object::NamedObject;
use super::{AmlError, AmlStream};

use crate::kernel::sync::spin_lock::Mutex;

use crate::kernel::drivers::acpi::aml::data_object::DataObject;
use crate::kernel::drivers::acpi::aml::AcpiInt;
use alloc::sync::Arc;
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
    /* parent: Option<Arc<Mutex<Self>>>, */
    list: Vec<(NameString, ObjectListItem)>,
}

pub enum ContentObject {
    NamedObject(NamedObject),
    DataRefObject(DataRefObject),
}

#[derive(Clone)]
pub struct ParseHelper {
    root_stream: AmlStream,
    root_object_list: Arc<Mutex<ObjectList>>,
    current_object_list: Arc<Mutex<ObjectList>>,
}

impl ObjectList {
    fn new(_parent: Option<Arc<Mutex<Self>>>, scope: &NameString) -> Self {
        Self {
            scope_name: scope.clone(),
            /* parent, */
            list: Vec::new(),
        }
    }

    pub fn add_item(&mut self, name: NameString, object: ObjectListItem) {
        self.list.push((name, object));
    }

    pub fn get_item(&self, name: &NameString) -> Option<ObjectListItem> {
        self.list
            .iter()
            .find(|item| item.0 == *name)
            .and_then(|item| Some(item.1.clone()))
    }
}

impl ParseHelper {
    pub fn new(stream: AmlStream, scope_name: &NameString) -> Self {
        let list = Arc::new(Mutex::new(ObjectList::new(None, scope_name)));
        Self {
            root_stream: stream,
            current_object_list: list.clone(),
            root_object_list: list,
        }
    }

    pub fn add_def_name(
        &mut self,
        name: &NameString,
        object: &DataRefObject,
    ) -> Result<(), AmlError> {
        Ok(self
            .current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .add_item(name.clone(), ObjectListItem::DefName(object.clone())))
    }

    pub fn add_alias_name(
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

    pub fn add_named_object(
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

    pub fn move_into_scope(&mut self, scope_name: &NameString) -> Result<(), AmlError> {
        let current_object_list = self
            .current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?;
        if current_object_list.scope_name == *scope_name {
            return Ok(());
        }
        let scope = current_object_list.get_item(scope_name);

        if let Some(scope) = scope {
            return match scope {
                ObjectListItem::DefScope(c) => {
                    self.current_object_list = c;
                    Ok(())
                }
                _ => Err(AmlError::InvalidScope(scope_name.clone())),
            };
        }
        let target_scope = if current_object_list.scope_name.is_child(scope_name) {
            drop(current_object_list);
            Self::find_target_scope(self.current_object_list.clone(), scope_name)?
        } else {
            drop(current_object_list);
            Self::find_target_scope(self.root_object_list.clone(), scope_name)?
        };

        if &target_scope
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .scope_name
            == scope_name
        {
            self.current_object_list = target_scope;
        } else {
            let child_object_list = Arc::new(Mutex::new(ObjectList::new(
                Some(self.current_object_list.clone()),
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
        }
        return Ok(());
    }

    /* pub fn out_from_current_scope(&mut self) {
        if let Some(p) = self.current_object_list.lock().unwrap().parent.clone() {
            self.current_object_list = p;
        }
    } */

    fn find_target_scope(
        list: Arc<Mutex<ObjectList>>,
        name: &NameString,
    ) -> Result<Arc<Mutex<ObjectList>>, AmlError> {
        let list_lock = list.try_lock().or(Err(AmlError::MutexError))?;
        for item in list_lock.list.iter() {
            if item.0.is_child(name) {
                if let ObjectListItem::DefScope(child) = &item.1 {
                    let child = child.clone();
                    drop(list_lock);
                    return Self::find_target_scope(child, name);
                }
            }
        }
        return Ok(list);
    }

    fn find_item_from_list(
        list: Arc<Mutex<ObjectList>>,
        name: &NameString,
    ) -> Result<Option<ObjectListItem>, AmlError> {
        let list = list.try_lock().or(Err(AmlError::MutexError))?;
        for item in list.list.iter() {
            if &item.0 == name {
                return Ok(Some(item.1.clone()));
            }
        }
        Ok(None)
    }

    fn find_item(&self, name: &NameString) -> Result<Option<ObjectListItem>, AmlError> {
        if let Some(item) = Self::find_item_from_list(
            Self::find_target_scope(self.current_object_list.clone(), name)?,
            name,
        )? {
            return Ok(Some(item));
        }
        Self::find_item_from_list(
            Self::find_target_scope(self.root_object_list.clone(), name)?,
            name,
        )
    }

    pub fn find_content_object(
        &self,
        name: &NameString,
    ) -> Result<Option<ContentObject>, AmlError> {
        if let Some(item) = self.find_item(name)? {
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

    fn _find_content_object_with_parsing(
        &mut self,
        list: Arc<Mutex<ObjectList>>,
        name: &NameString,
    ) -> Result<Option<ContentObject>, AmlError> {
        let mut list_locked = list.try_lock().or(Err(AmlError::MutexError))?;
        let mut index = 0;
        loop {
            let i = list_locked.list.get(index);
            if i.is_none() {
                return Ok(None);
            }
            let i = i.unwrap();

            if &i.0 == name {
                if !matches!(i.1, ObjectListItem::DefScope(_)) {
                    let object = i.1.clone();
                    drop(list_locked);
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
                        let s = s.clone();
                        drop(list_locked);
                        return self._find_content_object_with_parsing(s, name);
                    }
                    ObjectListItem::DefName(d_r) => match d_r {
                        DataRefObject::DataObject(d_o) => match d_o {
                            DataObject::ComputationalData(_) => {
                                return Ok(Some(ContentObject::DataRefObject(d_r.clone())))
                            }
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
                        /* let current_scope = list_locked.scope_name.clone(); */
                        drop(list_locked);

                        n_o.parse_list(self)?;

                        /* let new_scope = Self::find_target_scope(list.clone(), name)?;
                        if new_scope
                            .try_lock()
                            .or(Err(AmlError::MutexError))?
                            .scope_name
                            != current_scope
                        {
                            return self._find_content_object_with_parsing(new_scope, name);
                        } */
                        list_locked = list.try_lock().or(Err(AmlError::MutexError))?;
                    }
                }
            }
            index += 1;
        }
    }

    /// Find Element with parsing Field and return the object including it.
    pub fn find_content_object_with_parsing(
        &mut self,
        name: &NameString,
    ) -> Result<Option<ContentObject>, AmlError> {
        if let Some(o) = self.find_content_object(name)? {
            /* may be needless? */
            return Ok(Some(o));
        }
        let is_child = self
            .current_object_list
            .try_lock()
            .or(Err(AmlError::MutexError))?
            .scope_name
            .is_child(name);
        let target_list = if is_child {
            Self::find_target_scope(self.current_object_list.clone(), name)?
        } else {
            Self::find_target_scope(self.root_object_list.clone(), name)?
        };
        self._find_content_object_with_parsing(target_list, name)
    }

    pub fn find_method_argument_count(
        &mut self,
        method_name: &NameString,
    ) -> Result<Option<AcpiInt>, AmlError> {
        let result = self.find_content_object_with_parsing(method_name)?;

        Ok(match result.unwrap() {
            ContentObject::DataRefObject(d_r) => match d_r {
                DataRefObject::ObjectReference(_) => unimplemented!(),
                DataRefObject::DataObject(_) => Some(0),
            },
            ContentObject::NamedObject(n_o) => n_o.get_argument_count(),
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
