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

impl TreeNode {
    const DEFAULT_CHILDREN_ENTRIES: usize = 64;
    const DEFAULT_VARIABLES_ENTRIES: usize = 64;

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
        target_scope: NameString,
        current_index: usize,
    ) -> Result<(), AmlError> {
        let child_scope = target_scope
            .get_element_as_name_string(current_index)
            .unwrap();
        let result = self
            .current
            .children
            .lock()
            .unwrap()
            .iter()
            .find(|n| -> bool { n.name == child_scope })
            .and_then(|n| Some(n.clone()));
        /* For Mutex */
        if let Some(c) = result {
            self.current = c;
        } else {
            let node = Arc::new(TreeNode::new(child_scope, Arc::downgrade(&self.current)));
            self.current.children.lock().unwrap().push(node.clone());
            self.current = node;
        }
        if target_scope.len() - 1 == current_index {
            Ok(())
        } else {
            self._move_current_scope(target_scope, current_index + 1)
        }
    }

    pub fn move_current_scope(&mut self, scope: &NameString) -> Result<(), AmlError> {
        if scope.len() > 0 {
            self.current = self.root.clone();
            self._move_current_scope(scope.clone(), 0)
        } else {
            if scope.is_root() {
                self.current = self.root.clone();
                Ok(())
            } else {
                pr_err!(
                    "Failed to get the relative name(target: {}, current: {})",
                    scope,
                    self.current.name
                );
                Err(AmlError::InvalidScope(scope.clone()))
            }
        }
    }

    pub fn move_to_parent(&mut self) -> Result<bool, AmlError> {
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

    fn _find_data_and_then_clone(
        v: &Vec<(NameString, Arc<Mutex<AmlVariable>>)>,
        name: &NameString,
    ) -> Option<Arc<Mutex<AmlVariable>>> {
        v.iter()
            .find(|e| &e.0 == name)
            .and_then(|e| Some(e.1.clone()))
    }

    fn _find_data_from_child_scope(
        scope: Arc<TreeNode>,
        relative_target_scope: &NameString,
        current_index: usize,
    ) -> Result<Option<Arc<Mutex<AmlVariable>>>, AmlError> {
        if relative_target_scope.len() - 1 == current_index {
            let name = relative_target_scope
                .get_element_as_name_string(current_index)
                .unwrap();
            Ok(Self::_find_data_and_then_clone(
                &scope.variables.lock().unwrap(),
                &name,
            ))
        } else {
            let child_scope = relative_target_scope
                .get_element_as_name_string(current_index)
                .unwrap();
            let child = scope
                .children
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.name == child_scope)
                .and_then(|c| Some(c.clone()));
            if let Some(c) = child {
                Self::_find_data_from_child_scope(c, relative_target_scope, current_index + 1)
            } else {
                return Ok(None);
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
        relative_name: &NameString,
    ) -> Result<Option<Arc<Mutex<AmlVariable>>>, AmlError> {
        if relative_name.len() != 1 {
            Self::_find_data_from_child_scope(self.current.clone(), relative_name, 0)
        } else {
            Ok(Self::_find_data_and_then_clone(
                &self.current.variables.lock().unwrap(),
                relative_name,
            ))
        }
    }

    #[allow(dead_code)]
    pub fn find_data_recursively(
        &self,
        name: &NameString,
    ) -> Result<Option<Arc<Mutex<AmlVariable>>>, AmlError> {
        if name.len() != 1 {
            pr_err!("{} is not single name.", name);
            Err(AmlError::InvalidMethodName(name.clone()))?;
        }
        let result = self.find_data_from_current_scope(name)?;
        if result.is_some() {
            return Ok(result);
        }
        drop(result);

        let mut current = self.current.clone();
        while let Some(parent) = current
            .parent
            .as_ref()
            .and_then(|p| Some(p.upgrade().ok_or(AmlError::ObjectTreeError)))
        {
            current = parent?;
            let r = Self::_find_data_and_then_clone(&current.variables.lock().unwrap(), name);
            if r.is_some() {
                return Ok(r);
            }
        }
        return Ok(None);
    }

    pub fn add_data(
        &self,
        name: NameString,
        data: AmlVariable,
    ) -> Result<Arc<Mutex<AmlVariable>>, AmlError> {
        if name.len() != 1 {
            pr_err!("{} is not single name.", name);
            return Err(AmlError::InvalidMethodName(name));
        }
        /* Maybe needless */
        if let Some(d) = self.find_data_from_current_scope(&name)? {
            pr_warn!("{} exists already, it will be overwritten.", name);
            *d.lock().unwrap() = data;
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
