//! Hearth ECS — a small archetype-table entity component system built for the life sim.
//!
//! * Entities are generational handles ([`Entity`]).
//! * Components are any `'static + Send + Sync` type, stored in type-erased columns,
//!   grouped into archetype tables by their exact component set.
//! * Queries (`world.query::<(Entity, &A, &mut B)>()`) borrow `&World` and check aliasing
//!   at runtime per component type, so several non-conflicting queries and resources can be
//!   held at once.
//! * Structural changes (spawn/insert/remove/despawn) need `&mut World`, or can be deferred
//!   with [`World::defer`] and applied at the next [`World::flush`].

mod archetype;
mod blob;
mod borrow;
mod bundle;
mod component;
mod entity;
mod events;
mod query;
mod resource;
mod schedule;
mod world;

pub use archetype::{Archetype, ArchetypeId};
pub use borrow::{Ref, RefMut};
pub use bundle::Bundle;
pub use component::{Component, ComponentId};
#[doc(hidden)]
pub use component::Components;
pub use entity::Entity;
pub use events::{EventReader, Events};
pub use query::{Or, QueryBorrow, QueryData, QueryFilter, QueryIter, With, Without};
pub use schedule::{Schedule, SystemTiming};
pub use world::World;

pub mod prelude {
    pub use crate::{
        Bundle, Entity, EventReader, Events, Or, Ref, RefMut, Schedule, With, Without, World,
    };
}
