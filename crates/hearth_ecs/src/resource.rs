use std::any::{Any, TypeId};
use std::cell::UnsafeCell;
use std::collections::HashMap;

use crate::borrow::BorrowFlag;

pub(crate) struct ResourceEntry {
    pub name: &'static str,
    pub flag: BorrowFlag,
    pub value: UnsafeCell<Box<dyn Any + Send + Sync>>,
}

// SAFETY: access to `value` through `&ResourceEntry` is guarded by `flag`.
unsafe impl Sync for ResourceEntry {}

#[derive(Default)]
pub(crate) struct Resources {
    pub map: HashMap<TypeId, ResourceEntry>,
}
