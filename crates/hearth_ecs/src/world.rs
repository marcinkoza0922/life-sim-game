use std::any::{Any, TypeId};
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::sync::Mutex;

use crate::archetype::{Archetype, ArchetypeId};
use crate::blob::BlobVec;
use crate::borrow::{BorrowFlag, Ref, RefMut};
use crate::bundle::Bundle;
use crate::component::{Component, ComponentId, Components};
use crate::entity::{Entities, Entity, Location};
use crate::events::Events;
use crate::query::{QueryBorrow, QueryData, QueryFilter};
use crate::resource::{ResourceEntry, Resources};

type Deferred = Box<dyn FnOnce(&mut World) + Send>;

/// The container for all entities, components and resources.
pub struct World {
    entities: Entities,
    components: Components,
    archetypes: Vec<Archetype>,
    archetype_index: HashMap<Box<[ComponentId]>, u32>,
    resources: Resources,
    deferred: Mutex<Vec<Deferred>>,
    event_updaters: Vec<fn(&mut World)>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        let mut w = World {
            entities: Entities::default(),
            components: Components::default(),
            archetypes: Vec::new(),
            archetype_index: HashMap::new(),
            resources: Resources::default(),
            deferred: Mutex::new(Vec::new()),
            event_updaters: Vec::new(),
        };
        // archetype 0: the empty archetype
        w.archetype_for(&[]);
        w
    }

    // ---------------------------------------------------------------- internals

    pub(crate) fn components(&self) -> &Components {
        &self.components
    }
    pub(crate) fn archetypes_raw(&self) -> &[Archetype] {
        &self.archetypes
    }
    pub(crate) fn entities_raw(&self) -> &Entities {
        &self.entities
    }

    fn archetype_for(&mut self, sorted_ids: &[ComponentId]) -> u32 {
        if let Some(&a) = self.archetype_index.get(sorted_ids) {
            return a;
        }
        let columns = sorted_ids
            .iter()
            .map(|&id| {
                let info = self.components.info(id);
                BlobVec::new(info.layout, info.drop)
            })
            .collect();
        let id = self.archetypes.len() as u32;
        self.archetypes.push(Archetype { ids: sorted_ids.into(), columns, entities: Vec::new() });
        self.archetype_index.insert(sorted_ids.into(), id);
        id
    }

    fn two_archetypes(&mut self, a: u32, b: u32) -> (&mut Archetype, &mut Archetype) {
        assert_ne!(a, b);
        let (a, b) = (a as usize, b as usize);
        if a < b {
            let (l, r) = self.archetypes.split_at_mut(b);
            (&mut l[a], &mut r[0])
        } else {
            let (l, r) = self.archetypes.split_at_mut(a);
            (&mut r[0], &mut l[b])
        }
    }

    /// Moves `e` from its archetype to `target`. Columns absent from the target are dropped,
    /// except when `forget_missing` (their values were already moved out by the caller).
    fn move_entity(&mut self, e: Entity, from: Location, target: u32, forget_missing: bool) -> u32 {
        let row = from.row as usize;
        let (src, dst) = self.two_archetypes(from.archetype, target);
        for (i, id) in src.ids.iter().enumerate() {
            match dst.ids.binary_search(id) {
                Ok(j) => unsafe { src.columns[i].swap_remove_into(row, &mut dst.columns[j]) },
                Err(_) if forget_missing => unsafe { src.columns[i].swap_remove_forget(row) },
                Err(_) => unsafe { src.columns[i].swap_remove_drop(row) },
            }
        }
        src.entities.swap_remove(row);
        let swapped = src.entities.get(row).copied();
        let new_row = dst.entities.len() as u32;
        dst.entities.push(e);
        if let Some(s) = swapped {
            self.entities.set_location(s, Location { archetype: from.archetype, row: row as u32 });
        }
        self.entities.set_location(e, Location { archetype: target, row: new_row });
        new_row
    }

    // ---------------------------------------------------------------- entities

    /// Spawns an entity with the given components.
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let mut ids = Vec::new();
        B::register(&mut self.components, &mut ids);
        let mut sorted = ids.clone();
        sorted.sort();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "hearth_ecs: bundle contains a duplicate component type");
        let a = self.archetype_for(&sorted);
        let arch = &mut self.archetypes[a as usize];
        let row = arch.entities.len() as u32;
        let e = self.entities.alloc(Location { archetype: a, row });
        arch.entities.push(e);
        bundle.take(&mut |i, ptr| {
            let col = arch.ids.binary_search(&ids[i]).unwrap();
            unsafe { arch.columns[col].push(ptr) };
        });
        e
    }

    /// Spawns an entity with no components.
    pub fn spawn_empty(&mut self) -> Entity {
        self.spawn(())
    }

    /// Spawns an empty entity with exactly this handle (index and generation), so saved worlds
    /// can be restored without remapping references. Returns false if the index is in use.
    pub fn spawn_empty_at(&mut self, e: Entity) -> bool {
        let a = self.archetype_for(&[]);
        let row = self.archetypes[a as usize].entities.len() as u32;
        if !self.entities.alloc_at(e, Location { archetype: a, row }) {
            return false;
        }
        self.archetypes[a as usize].entities.push(e);
        true
    }

    /// The entity allocator's free list, for saving (see [`World::set_free_list`]).
    pub fn free_list(&self) -> Vec<Entity> {
        self.entities.free_list()
    }

    /// Restores an allocator free list after respawning a saved world with
    /// [`World::spawn_empty_at`], so future spawns get the same handles as the original.
    pub fn set_free_list(&mut self, list: &[Entity]) {
        self.entities.set_free_list(list);
    }

    pub fn despawn(&mut self, e: Entity) -> bool {
        let Some(loc) = self.entities.location(e) else { return false };
        let row = loc.row as usize;
        let arch = &mut self.archetypes[loc.archetype as usize];
        for col in arch.columns.iter_mut() {
            unsafe { col.swap_remove_drop(row) };
        }
        arch.entities.swap_remove(row);
        if let Some(&s) = arch.entities.get(row) {
            self.entities.set_location(s, loc);
        }
        self.entities.free(e);
        true
    }

    pub fn contains(&self, e: Entity) -> bool {
        self.entities.location(e).is_some()
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// All live entities (archetype order).
    pub fn entities(&self) -> Vec<Entity> {
        self.archetypes.iter().flat_map(|a| a.entities.iter().copied()).collect()
    }

    /// Adds (or replaces) the components in `bundle` on `e`. Returns false if `e` is dead.
    pub fn insert<B: Bundle>(&mut self, e: Entity, bundle: B) -> bool {
        let Some(loc) = self.entities.location(e) else { return false };
        let mut ids = Vec::new();
        B::register(&mut self.components, &mut ids);
        let src_ids = &self.archetypes[loc.archetype as usize].ids;
        let mut target_ids: Vec<ComponentId> = src_ids.iter().chain(ids.iter()).copied().collect();
        target_ids.sort();
        target_ids.dedup();
        let target = if target_ids.len() == src_ids.len() { loc.archetype } else { self.archetype_for(&target_ids) };
        let src_ids: Box<[ComponentId]> = self.archetypes[loc.archetype as usize].ids.clone();
        let row = if target == loc.archetype { loc.row } else { self.move_entity(e, loc, target, false) };
        let arch = &mut self.archetypes[target as usize];
        bundle.take(&mut |i, ptr| {
            let col = arch.ids.binary_search(&ids[i]).unwrap();
            unsafe {
                if src_ids.binary_search(&ids[i]).is_ok() {
                    arch.columns[col].replace(row as usize, ptr);
                } else {
                    arch.columns[col].push(ptr);
                }
            }
        });
        true
    }

    /// Adds or replaces a single component.
    pub fn insert_one<T: Component>(&mut self, e: Entity, c: T) -> bool {
        self.insert(e, (c,))
    }

    /// Removes and returns component `T`.
    pub fn remove<T: Component>(&mut self, e: Entity) -> Option<T> {
        let loc = self.entities.location(e)?;
        let id = self.components.id::<T>()?;
        let src = &self.archetypes[loc.archetype as usize];
        let col = src.column(id)?;
        let mut out = MaybeUninit::<T>::uninit();
        unsafe {
            std::ptr::copy_nonoverlapping(col.ptr_at(loc.row as usize) as *const T, out.as_mut_ptr(), 1);
        }
        let target_ids: Vec<ComponentId> = src.ids.iter().copied().filter(|&c| c != id).collect();
        let target = self.archetype_for(&target_ids);
        self.move_entity(e, loc, target, true);
        Some(unsafe { out.assume_init() })
    }

    pub fn has<T: Component>(&self, e: Entity) -> bool {
        let (Some(loc), Some(id)) = (self.entities.location(e), self.components.id::<T>()) else { return false };
        self.archetypes[loc.archetype as usize].has(id)
    }

    fn component_ptr<T: Component>(&self, e: Entity) -> Option<(*mut T, &BorrowFlag)> {
        let loc = self.entities.location(e)?;
        let id = self.components.id::<T>()?;
        let col = self.archetypes[loc.archetype as usize].column(id)?;
        Some((col.ptr_at(loc.row as usize) as *mut T, &self.components.info(id).flag))
    }

    /// Shared borrow of one component. Panics on borrow conflict.
    pub fn get<T: Component>(&self, e: Entity) -> Option<Ref<'_, T>> {
        let (p, flag) = self.component_ptr::<T>(e)?;
        flag.acquire(false, std::any::type_name::<T>());
        Some(Ref { value: unsafe { &*p }, flag })
    }

    /// Unique borrow of one component. Panics on borrow conflict.
    pub fn get_mut<T: Component>(&self, e: Entity) -> Option<RefMut<'_, T>> {
        let (p, flag) = self.component_ptr::<T>(e)?;
        flag.acquire(true, std::any::type_name::<T>());
        Some(RefMut { value: unsafe { &mut *p }, flag })
    }

    /// Direct mutable access when holding `&mut World` (no runtime borrow needed).
    pub fn get_mut_direct<T: Component>(&mut self, e: Entity) -> Option<&mut T> {
        let (p, _) = self.component_ptr::<T>(e)?;
        Some(unsafe { &mut *p })
    }

    /// Clone a component out.
    pub fn cloned<T: Component + Clone>(&self, e: Entity) -> Option<T> {
        self.get::<T>(e).map(|r| (*r).clone())
    }

    // ---------------------------------------------------------------- queries

    pub fn query<Q: QueryData>(&self) -> QueryBorrow<'_, Q, ()> {
        QueryBorrow::new(self)
    }

    pub fn query_filtered<Q: QueryData, F: QueryFilter>(&self) -> QueryBorrow<'_, Q, F> {
        QueryBorrow::new(self)
    }

    // ---------------------------------------------------------------- resources

    pub fn insert_resource<T: Component>(&mut self, value: T) {
        let tid = TypeId::of::<T>();
        if let Some(entry) = self.resources.map.get_mut(&tid) {
            *entry.value.get_mut() = Box::new(value);
        } else {
            self.resources.map.insert(
                tid,
                ResourceEntry {
                    name: std::any::type_name::<T>(),
                    flag: BorrowFlag::default(),
                    value: UnsafeCell::new(Box::new(value)),
                },
            );
        }
    }

    pub fn init_resource<T: Component + Default>(&mut self) {
        if !self.has_resource::<T>() {
            self.insert_resource(T::default());
        }
    }

    pub fn remove_resource<T: Component>(&mut self) -> Option<T> {
        let entry = self.resources.map.remove(&TypeId::of::<T>())?;
        let b: Box<dyn Any + Send + Sync> = entry.value.into_inner();
        b.downcast::<T>().ok().map(|b| *b)
    }

    pub fn has_resource<T: Component>(&self) -> bool {
        self.resources.map.contains_key(&TypeId::of::<T>())
    }

    pub fn get_resource<T: Component>(&self) -> Option<Ref<'_, T>> {
        let entry = self.resources.map.get(&TypeId::of::<T>())?;
        entry.flag.acquire(false, entry.name);
        let v = unsafe { &*entry.value.get() };
        Some(Ref { value: v.downcast_ref::<T>().unwrap(), flag: &entry.flag })
    }

    pub fn get_resource_mut<T: Component>(&self) -> Option<RefMut<'_, T>> {
        let entry = self.resources.map.get(&TypeId::of::<T>())?;
        entry.flag.acquire(true, entry.name);
        let v = unsafe { &mut *entry.value.get() };
        Some(RefMut { value: v.downcast_mut::<T>().unwrap(), flag: &entry.flag })
    }

    /// Shared borrow of a resource. Panics if missing or mutably borrowed.
    #[track_caller]
    pub fn res<T: Component>(&self) -> Ref<'_, T> {
        self.get_resource::<T>()
            .unwrap_or_else(|| panic!("missing resource {}", std::any::type_name::<T>()))
    }

    /// Unique borrow of a resource. Panics if missing or borrowed.
    #[track_caller]
    pub fn res_mut<T: Component>(&self) -> RefMut<'_, T> {
        self.get_resource_mut::<T>()
            .unwrap_or_else(|| panic!("missing resource {}", std::any::type_name::<T>()))
    }

    /// Temporarily removes a resource so it can be used alongside `&mut World`.
    pub fn resource_scope<T: Component, R>(&mut self, f: impl FnOnce(&mut World, &mut T) -> R) -> R {
        let mut r = self
            .remove_resource::<T>()
            .unwrap_or_else(|| panic!("missing resource {}", std::any::type_name::<T>()));
        let out = f(self, &mut r);
        self.insert_resource(r);
        out
    }

    // ---------------------------------------------------------------- events

    /// Registers an event channel; it is updated by [`World::update_events`].
    pub fn add_event<T: Component>(&mut self) {
        if !self.has_resource::<Events<T>>() {
            self.insert_resource(Events::<T>::default());
            self.event_updaters.push(|w| w.res_mut::<Events<T>>().update());
        }
    }

    pub fn send_event<T: Component>(&self, ev: T) {
        self.res_mut::<Events<T>>().send(ev);
    }

    /// Advances all event channels (call once per frame/tick).
    pub fn update_events(&mut self) {
        for u in self.event_updaters.clone() {
            u(self);
        }
    }

    // ---------------------------------------------------------------- deferred commands

    /// Queues a structural change to run at the next [`World::flush`].
    pub fn defer(&self, f: impl FnOnce(&mut World) + Send + 'static) {
        self.deferred.lock().unwrap().push(Box::new(f));
    }

    pub fn defer_despawn(&self, e: Entity) {
        self.defer(move |w| {
            w.despawn(e);
        });
    }

    pub fn defer_insert<B: Bundle>(&self, e: Entity, b: B) {
        self.defer(move |w| {
            w.insert(e, b);
        });
    }

    pub fn defer_remove<T: Component>(&self, e: Entity) {
        self.defer(move |w| {
            w.remove::<T>(e);
        });
    }

    /// Applies deferred commands (including ones queued by commands themselves).
    pub fn flush(&mut self) {
        loop {
            let cmds = std::mem::take(&mut *self.deferred.lock().unwrap());
            if cmds.is_empty() {
                break;
            }
            for c in cmds {
                c(self);
            }
        }
    }

    // ---------------------------------------------------------------- debugging

    /// Registers a `Debug` formatter so inspectors can print `T` generically.
    pub fn register_debug<T: Component + std::fmt::Debug>(&mut self) {
        let id = self.components.register::<T>();
        self.components.info_mut(id).debug = Some(|p| format!("{:#?}", unsafe { &*(p as *const T) }));
    }

    /// Lists component type names on `e`, with a Debug dump when registered via [`World::register_debug`].
    /// Components currently mutably borrowed are reported as `<borrowed>`.
    pub fn inspect(&self, e: Entity) -> Vec<(&'static str, Option<String>)> {
        let Some(loc) = self.entities.location(e) else { return Vec::new() };
        let arch = &self.archetypes[loc.archetype as usize];
        arch.ids
            .iter()
            .zip(arch.columns.iter())
            .map(|(&id, col)| {
                let info = self.components.info(id);
                let dump = info.debug.map(|f| {
                    if info.flag.try_shared() {
                        let s = f(col.ptr_at(loc.row as usize));
                        info.flag.release_shared();
                        s
                    } else {
                        "<borrowed>".to_string()
                    }
                });
                (info.name, dump)
            })
            .collect()
    }

    pub fn archetype_count(&self) -> usize {
        self.archetypes.len()
    }

    pub fn archetype(&self, id: ArchetypeId) -> Option<&Archetype> {
        self.archetypes.get(id.0 as usize)
    }
}
