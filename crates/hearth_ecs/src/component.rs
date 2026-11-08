use std::alloc::Layout;
use std::any::TypeId;
use std::collections::HashMap;

use crate::borrow::BorrowFlag;

/// Anything `'static + Send + Sync` can be a component.
pub trait Component: 'static + Send + Sync {}
impl<T: 'static + Send + Sync> Component for T {}

#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ComponentId(pub(crate) u32);

pub(crate) struct ComponentInfo {
    pub name: &'static str,
    pub layout: Layout,
    pub drop: Option<unsafe fn(*mut u8)>,
    pub debug: Option<fn(*const u8) -> String>,
    pub flag: BorrowFlag,
}

unsafe fn drop_ptr<T>(p: *mut u8) {
    unsafe { p.cast::<T>().drop_in_place() }
}

#[derive(Default)]
pub struct Components {
    infos: Vec<ComponentInfo>,
    by_type: HashMap<TypeId, ComponentId>,
}

impl Components {
    pub(crate) fn register<T: Component>(&mut self) -> ComponentId {
        let tid = TypeId::of::<T>();
        if let Some(&id) = self.by_type.get(&tid) {
            return id;
        }
        let id = ComponentId(self.infos.len() as u32);
        self.infos.push(ComponentInfo {
            name: std::any::type_name::<T>(),
            layout: Layout::new::<T>(),
            drop: std::mem::needs_drop::<T>().then_some(drop_ptr::<T> as unsafe fn(*mut u8)),
            debug: None,
            flag: BorrowFlag::default(),
        });
        self.by_type.insert(tid, id);
        id
    }

    pub(crate) fn id<T: Component>(&self) -> Option<ComponentId> {
        self.by_type.get(&TypeId::of::<T>()).copied()
    }

    pub(crate) fn info(&self, id: ComponentId) -> &ComponentInfo {
        &self.infos[id.0 as usize]
    }

    pub(crate) fn info_mut(&mut self, id: ComponentId) -> &mut ComponentInfo {
        &mut self.infos[id.0 as usize]
    }
}
