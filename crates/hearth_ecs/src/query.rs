//! Typed queries over archetype tables.

use std::marker::PhantomData;

use crate::archetype::Archetype;
use crate::component::{Component, ComponentId, Components};
use crate::entity::Entity;
use crate::world::World;

#[derive(Copy, Clone, Debug)]
pub struct Access {
    pub id: ComponentId,
    pub mutable: bool,
}

/// What a query fetches per entity. Implemented for `Entity`, `&T`, `&mut T`, `Option<&T>`,
/// `Option<&mut T>` and tuples of those (up to 10).
///
/// # Safety
/// `access` must report every component that `fetch`/`item` touch, with correct mutability.
pub unsafe trait QueryData {
    type Item<'a>;
    type State: Copy;
    type Fetch: Copy;
    fn state(components: &Components) -> Self::State;
    fn access(state: &Self::State, out: &mut Vec<Access>);
    fn matches(state: &Self::State, arch: &Archetype) -> bool;
    /// # Safety
    /// `arch` must match.
    unsafe fn fetch(state: &Self::State, arch: &Archetype) -> Self::Fetch;
    /// # Safety
    /// `row` must be in bounds, and the borrow rules established by `access` must hold for `'a`.
    unsafe fn item<'a>(fetch: Self::Fetch, row: usize) -> Self::Item<'a>;
}

unsafe impl QueryData for Entity {
    type Item<'a> = Entity;
    type State = ();
    type Fetch = *const Entity;
    fn state(_: &Components) {}
    fn access(_: &(), _: &mut Vec<Access>) {}
    fn matches(_: &(), _: &Archetype) -> bool {
        true
    }
    unsafe fn fetch(_: &(), arch: &Archetype) -> *const Entity {
        arch.entities.as_ptr()
    }
    unsafe fn item<'a>(f: *const Entity, row: usize) -> Self::Item<'a> {
        unsafe { *f.add(row) }
    }
}

unsafe impl<T: Component> QueryData for &T {
    type Item<'a> = &'a T;
    type State = Option<ComponentId>;
    type Fetch = *const T;
    fn state(c: &Components) -> Self::State {
        c.id::<T>()
    }
    fn access(s: &Self::State, out: &mut Vec<Access>) {
        if let Some(id) = *s {
            out.push(Access { id, mutable: false });
        }
    }
    fn matches(s: &Self::State, arch: &Archetype) -> bool {
        s.is_some_and(|id| arch.has(id))
    }
    unsafe fn fetch(s: &Self::State, arch: &Archetype) -> *const T {
        arch.column(s.unwrap()).unwrap().base_ptr() as *const T
    }
    unsafe fn item<'a>(f: *const T, row: usize) -> &'a T {
        unsafe { &*f.add(row) }
    }
}

unsafe impl<T: Component> QueryData for &mut T {
    type Item<'a> = &'a mut T;
    type State = Option<ComponentId>;
    type Fetch = *mut T;
    fn state(c: &Components) -> Self::State {
        c.id::<T>()
    }
    fn access(s: &Self::State, out: &mut Vec<Access>) {
        if let Some(id) = *s {
            out.push(Access { id, mutable: true });
        }
    }
    fn matches(s: &Self::State, arch: &Archetype) -> bool {
        s.is_some_and(|id| arch.has(id))
    }
    unsafe fn fetch(s: &Self::State, arch: &Archetype) -> *mut T {
        arch.column(s.unwrap()).unwrap().base_ptr() as *mut T
    }
    unsafe fn item<'a>(f: *mut T, row: usize) -> &'a mut T {
        unsafe { &mut *f.add(row) }
    }
}

unsafe impl<T: Component> QueryData for Option<&T> {
    type Item<'a> = Option<&'a T>;
    type State = Option<ComponentId>;
    type Fetch = Option<*const T>;
    fn state(c: &Components) -> Self::State {
        c.id::<T>()
    }
    fn access(s: &Self::State, out: &mut Vec<Access>) {
        if let Some(id) = *s {
            out.push(Access { id, mutable: false });
        }
    }
    fn matches(_: &Self::State, _: &Archetype) -> bool {
        true
    }
    unsafe fn fetch(s: &Self::State, arch: &Archetype) -> Self::Fetch {
        s.and_then(|id| arch.column(id)).map(|c| c.base_ptr() as *const T)
    }
    unsafe fn item<'a>(f: Self::Fetch, row: usize) -> Option<&'a T> {
        f.map(|p| unsafe { &*p.add(row) })
    }
}

unsafe impl<T: Component> QueryData for Option<&mut T> {
    type Item<'a> = Option<&'a mut T>;
    type State = Option<ComponentId>;
    type Fetch = Option<*mut T>;
    fn state(c: &Components) -> Self::State {
        c.id::<T>()
    }
    fn access(s: &Self::State, out: &mut Vec<Access>) {
        if let Some(id) = *s {
            out.push(Access { id, mutable: true });
        }
    }
    fn matches(_: &Self::State, _: &Archetype) -> bool {
        true
    }
    unsafe fn fetch(s: &Self::State, arch: &Archetype) -> Self::Fetch {
        s.and_then(|id| arch.column(id)).map(|c| c.base_ptr() as *mut T)
    }
    unsafe fn item<'a>(f: Self::Fetch, row: usize) -> Option<&'a mut T> {
        f.map(|p| unsafe { &mut *p.add(row) })
    }
}

macro_rules! impl_query_tuple {
    ($($n:ident $i:tt),+) => {
        unsafe impl<$($n: QueryData),+> QueryData for ($($n,)+) {
            type Item<'a> = ($($n::Item<'a>,)+);
            type State = ($($n::State,)+);
            type Fetch = ($($n::Fetch,)+);
            fn state(c: &Components) -> Self::State {
                ($($n::state(c),)+)
            }
            fn access(s: &Self::State, out: &mut Vec<Access>) {
                $( $n::access(&s.$i, out); )+
            }
            fn matches(s: &Self::State, arch: &Archetype) -> bool {
                $( $n::matches(&s.$i, arch) )&&+
            }
            unsafe fn fetch(s: &Self::State, arch: &Archetype) -> Self::Fetch {
                unsafe { ($($n::fetch(&s.$i, arch),)+) }
            }
            unsafe fn item<'a>(f: Self::Fetch, row: usize) -> Self::Item<'a> {
                unsafe { ($($n::item(f.$i, row),)+) }
            }
        }
    };
}

impl_query_tuple!(A 0);
impl_query_tuple!(A 0, B 1);
impl_query_tuple!(A 0, B 1, C 2);
impl_query_tuple!(A 0, B 1, C 2, D 3);
impl_query_tuple!(A 0, B 1, C 2, D 3, E 4);
impl_query_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
impl_query_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
impl_query_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
impl_query_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
impl_query_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);

/// Archetype-level filter. Implemented for `()`, [`With`], [`Without`], [`Or`] and tuples (all must match).
pub trait QueryFilter {
    type State: Copy;
    fn state(components: &Components) -> Self::State;
    fn matches(state: &Self::State, arch: &Archetype) -> bool;
}

impl QueryFilter for () {
    type State = ();
    fn state(_: &Components) {}
    fn matches(_: &(), _: &Archetype) -> bool {
        true
    }
}

/// Only entities that have `T` (without fetching it).
pub struct With<T>(PhantomData<T>);
/// Only entities that lack `T`.
pub struct Without<T>(PhantomData<T>);
/// Entities matching either filter.
pub struct Or<A, B>(PhantomData<(A, B)>);

impl<T: Component> QueryFilter for With<T> {
    type State = Option<ComponentId>;
    fn state(c: &Components) -> Self::State {
        c.id::<T>()
    }
    fn matches(s: &Self::State, arch: &Archetype) -> bool {
        s.is_some_and(|id| arch.has(id))
    }
}

impl<T: Component> QueryFilter for Without<T> {
    type State = Option<ComponentId>;
    fn state(c: &Components) -> Self::State {
        c.id::<T>()
    }
    fn matches(s: &Self::State, arch: &Archetype) -> bool {
        s.is_none_or(|id| !arch.has(id))
    }
}

impl<A: QueryFilter, B: QueryFilter> QueryFilter for Or<A, B> {
    type State = (A::State, B::State);
    fn state(c: &Components) -> Self::State {
        (A::state(c), B::state(c))
    }
    fn matches(s: &Self::State, arch: &Archetype) -> bool {
        A::matches(&s.0, arch) || B::matches(&s.1, arch)
    }
}

macro_rules! impl_filter_tuple {
    ($($n:ident $i:tt),+) => {
        impl<$($n: QueryFilter),+> QueryFilter for ($($n,)+) {
            type State = ($($n::State,)+);
            fn state(c: &Components) -> Self::State {
                ($($n::state(c),)+)
            }
            fn matches(s: &Self::State, arch: &Archetype) -> bool {
                $( $n::matches(&s.$i, arch) )&&+
            }
        }
    };
}
impl_filter_tuple!(A 0);
impl_filter_tuple!(A 0, B 1);
impl_filter_tuple!(A 0, B 1, C 2);
impl_filter_tuple!(A 0, B 1, C 2, D 3);

/// An active query. Holds runtime borrows on the accessed component types until dropped.
pub struct QueryBorrow<'w, Q: QueryData, F: QueryFilter = ()> {
    world: &'w World,
    state: Q::State,
    filter: F::State,
    access: Vec<Access>,
    archetypes: Vec<u32>,
}

impl<'w, Q: QueryData, F: QueryFilter> QueryBorrow<'w, Q, F> {
    pub(crate) fn new(world: &'w World) -> Self {
        let components = world.components();
        let state = Q::state(components);
        let filter = F::state(components);
        let mut access = Vec::new();
        Q::access(&state, &mut access);
        for (i, a) in access.iter().enumerate() {
            for b in &access[i + 1..] {
                if a.id == b.id && (a.mutable || b.mutable) {
                    panic!(
                        "hearth_ecs: query `{}` accesses `{}` mutably more than once",
                        std::any::type_name::<Q>(),
                        components.info(a.id).name
                    );
                }
            }
        }
        for (i, a) in access.iter().enumerate() {
            let info = components.info(a.id);
            if !(if a.mutable { info.flag.try_unique() } else { info.flag.try_shared() }) {
                // roll back what we took, then panic with a useful message
                for b in &access[..i] {
                    components.info(b.id).flag.release(b.mutable);
                }
                panic!(
                    "hearth_ecs: query `{}` conflicts with an existing borrow of `{}`",
                    std::any::type_name::<Q>(),
                    info.name
                );
            }
        }
        let archetypes = world
            .archetypes_raw()
            .iter()
            .enumerate()
            .filter(|(_, a)| Q::matches(&state, a) && F::matches(&filter, a))
            .map(|(i, _)| i as u32)
            .collect();
        QueryBorrow { world, state, filter, access, archetypes }
    }

    pub fn iter(&mut self) -> QueryIter<'_, Q> {
        QueryIter {
            world: self.world,
            archetypes: self.archetypes.iter(),
            state: self.state,
            fetch: None,
            row: 0,
            len: 0,
        }
    }

    /// Fetch a single entity if it matches the query.
    pub fn get(&mut self, e: Entity) -> Option<Q::Item<'_>> {
        let loc = self.world.entities_raw().location(e)?;
        let arch = &self.world.archetypes_raw()[loc.archetype as usize];
        if !(Q::matches(&self.state, arch) && F::matches(&self.filter, arch)) {
            return None;
        }
        unsafe { Some(Q::item(Q::fetch(&self.state, arch), loc.row as usize)) }
    }

    /// Number of matching entities.
    pub fn count(&self) -> usize {
        self.archetypes.iter().map(|&a| self.world.archetypes_raw()[a as usize].len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }
}

impl<Q: QueryData, F: QueryFilter> Drop for QueryBorrow<'_, Q, F> {
    fn drop(&mut self) {
        let components = self.world.components();
        for a in &self.access {
            components.info(a.id).flag.release(a.mutable);
        }
    }
}

pub struct QueryIter<'q, Q: QueryData> {
    world: &'q World,
    archetypes: std::slice::Iter<'q, u32>,
    state: Q::State,
    fetch: Option<Q::Fetch>,
    row: usize,
    len: usize,
}

impl<'q, Q: QueryData> Iterator for QueryIter<'q, Q> {
    type Item = Q::Item<'q>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(f) = self.fetch
                && self.row < self.len
            {
                let row = self.row;
                self.row += 1;
                return Some(unsafe { Q::item(f, row) });
            }
            let &a = self.archetypes.next()?;
            let arch = &self.world.archetypes_raw()[a as usize];
            self.len = arch.len();
            self.row = 0;
            self.fetch = Some(unsafe { Q::fetch(&self.state, arch) });
        }
    }
}
