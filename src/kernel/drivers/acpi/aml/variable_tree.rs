//!
//! Tree Object for AML Variables
//!

use super::{AmlError, AmlVariable, NameString};

use crate::kernel::sync::spin_lock::Mutex;

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

struct TreeNode {
    name: NameString,
    parent: Option<Weak<Self>>,
    children: Mutex<Vec<Arc<Self>>>,
    variables: Mutex<Vec<(NameString, Arc<Mutex<AmlVariable>>)>>,
}

#[derive(Clone)]
pub struct AmlVariableTree {
    root: Arc<TreeNode>,
    current: Arc<TreeNode>,
}

pub struct AmlVariableTreeBackup {
    current: Arc<TreeNode>,
}

impl TreeNode {
    const DEFAULT_CHILDREN_ENTRIES: usize = 8;
    const DEFAULT_VARIABLES_ENTRIES: usize = 16;

    fn root() -> Self {
        Self {
            name: NameString::root(),
            parent: None,
            children: Mutex::new(Vec::with_capacity(Self::DEFAULT_CHILDREN_ENTRIES)),
            variables: Mutex::new(Vec::with_capacity(Self::DEFAULT_VARIABLES_ENTRIES)),
        }
    }

    fn new(name: NameString, parent: Weak<Self>) -> Self {
        Self {
            name,
            parent: Some(parent),
            children: Mutex::new(Vec::with_capacity(Self::DEFAULT_CHILDREN_ENTRIES)),
            variables: Mutex::new(Vec::with_capacity(Self::DEFAULT_VARIABLES_ENTRIES)),
        }
    }
}

impl AmlVariableTree {
    pub fn create_tree() -> Self {
        let root_node = Arc::new(TreeNode::root());
        Self {
            root: root_node.clone(),
            current: root_node,
        }
    }

    fn _move_current_scope(
        &mut self,
        scope: &NameString,
        relative_scope_name: NameString,
        index: usize,
    ) -> Result<(), AmlError> {
        if let Some(path_element) = relative_scope_name.get_element_as_name_string(index) {
            let target_scope = path_element.get_full_name_path(&self.current.name, true);
            drop(path_element);
            let result = self
                .current
                .children
                .lock()
                .unwrap()
                .iter()
                .find(|n| n.name == target_scope)
                .and_then(|c| Some(c.clone())); /* For Mutex */
            if let Some(c) = result {
                self.current = c;
            } else {
                let node = Arc::new(TreeNode::new(target_scope, Arc::downgrade(&self.current)));
                self.current.children.lock().unwrap().push(node.clone());
                self.current = node;
            }
            if relative_scope_name.len() - 1 == index {
                Ok(())
            } else {
                self._move_current_scope(scope, relative_scope_name, index + 1)
            }
        } else {
            pr_err!("Invalid Scope Name: {:?}", scope);
            return Err(AmlError::InvalidName(scope.clone()));
        }
    }

    pub fn move_current_scope(&mut self, scope: &NameString) -> Result<(), AmlError> {
        if &self.current.name == scope {
            return Ok(());
        } else if scope.len() == 0 {
            if scope.is_root() {
                self.current = self.root.clone();
                return Ok(());
            }
            pr_err!("Invalid Scope Name: {:?}", scope);
            return Err(AmlError::InvalidName(scope.clone()));
        }
        if self.current.name.is_child(scope) {
            let relative_path = if let Some(n) = scope.get_relative_name(&self.current.name) {
                n
            } else {
                pr_err!("Invalid Scope Name: {:?}", scope);
                return Err(AmlError::InvalidName(scope.clone()));
            };
            self._move_current_scope(scope, relative_path, 0)
        } else {
            match self.move_to_parent() {
                Ok(true) => self.move_current_scope(scope),
                Ok(false) => {
                    pr_err!("Invalid Scope Name: {:?}", scope);
                    Err(AmlError::InvalidName(scope.clone()))
                }
                Err(_) => {
                    self.move_to_root()?;
                    self.move_current_scope(scope)
                }
            }
        }
    }

    fn move_to_parent(&mut self) -> Result<bool, AmlError> {
        if let Some(p) = &self.current.parent {
            self.current = p.upgrade().ok_or(AmlError::ObjectTreeError)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn move_to_root(&mut self) -> Result<(), AmlError> {
        self.current = self.root.clone();
        Ok(())
    }

    pub fn get_current_scope_name(&self) -> &NameString {
        &self.current.name
    }

    pub fn backup_current_scope(&self) -> AmlVariableTreeBackup {
        AmlVariableTreeBackup {
            current: self.current.clone(),
        }
    }

    pub fn restore_current_scope(&mut self, backup: AmlVariableTreeBackup) {
        self.current = backup.current;
    }

    fn _find_data_and_then_clone(
        v: &Vec<(NameString, Arc<Mutex<AmlVariable>>)>,
        single_name: &NameString,
    ) -> Option<Arc<Mutex<AmlVariable>>> {
        v.iter()
            .find(|e| &e.0 == single_name)
            .and_then(|e| Some(e.1.clone()))
    }

    fn _find_data_from_child_scope(
        scope: Arc<TreeNode>,
        name: &NameString,
        current_index: usize,
    ) -> Result<Option<Arc<Mutex<AmlVariable>>>, AmlError> {
        if name.len() - 1 == current_index {
            Ok(Self::_find_data_and_then_clone(
                &scope.variables.lock().unwrap(),
                &name.get_element_as_name_string(current_index).unwrap(),
            ))
        } else {
            let child_scope = name
                .get_element_as_name_string(current_index)
                .and_then(|e| Some(e.get_full_name_path(&scope.name, true)))
                .unwrap_or_else(|| NameString::root());
            let child = scope
                .children
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.name == child_scope)
                .and_then(|c| Some(c.clone()));
            if let Some(c) = child {
                Self::_find_data_from_child_scope(c, name, current_index + 1)
            } else {
                Ok(None)
            }
        }
    }

    pub fn find_data_from_root(
        &self,
        name: &NameString,
    ) -> Result<Option<Arc<Mutex<AmlVariable>>>, AmlError> {
        Self::_find_data_from_child_scope(self.root.clone(), name, 0)
    }

    pub fn find_data_from_current_scope(
        &self,
        name: &NameString,
    ) -> Result<Option<Arc<Mutex<AmlVariable>>>, AmlError> {
        if name.len() != 1 {
            if let Some(relative_name) = name.get_relative_name(&self.current.name) {
                if relative_name.len() == 1 {
                    return Ok(Self::_find_data_and_then_clone(
                        &self.current.variables.lock().unwrap(),
                        &relative_name,
                    ));
                }
            }
            Self::_find_data_from_child_scope(self.current.clone(), name, 0)
        } else {
            Ok(Self::_find_data_and_then_clone(
                &self.current.variables.lock().unwrap(),
                name,
            ))
        }
    }

    pub fn add_data(
        &self,
        name: NameString,
        data: AmlVariable,
        allow_overwrite: bool,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        if name.len() != 1 {
            if let Some(relative_name) = name.get_relative_name(&self.current.name) {
                if relative_name.len() == 1 {
                    return self.add_data(relative_name, data, allow_overwrite);
                }
            }
            let scope = name.get_scope_name();
            let mut d = self.clone();
            pr_warn!(
                "Change the scope to {} from {} temporary.",
                scope,
                self.current.name
            );
            d.move_current_scope(&scope)?;
            return d.add_data(name, data, allow_overwrite);
        }
        if let Some(d) = self.find_data_from_current_scope(&name)? {
            if allow_overwrite {
                pr_warn!("{} exists already, it will be overwritten.", name);
                *d.try_lock().or(Err(AmlError::MutexError))? = data;
            }
            return Ok(d);
        }
        let d = Arc::new(Mutex::new(data));
        self.current
            .variables
            .lock()
            .unwrap()
            .push((name, d.clone()));
        return Ok(d);
    }
}
