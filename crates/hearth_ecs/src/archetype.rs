use crate::blob::BlobVec;
use crate::component::ComponentId;
use crate::entity::Entity;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ArchetypeId(pub(crate) u32);

/// A table of all entities that have exactly the component set `ids`.
pub struct Archetype {
    pub(crate) ids: Box<[ComponentId]>,
    pub(crate) columns: Box<[BlobVec]>,
    pub(crate) entities: Vec<Entity>,
}

impl Archetype {
    #[inline]
    pub(crate) fn column(&self, id: ComponentId) -> Option<&BlobVec> {
        self.ids.binary_search(&id).ok().map(|i| &self.columns[i])
    }
    #[inline]
    pub(crate) fn has(&self, id: ComponentId) -> bool {
        self.ids.binary_search(&id).is_ok()
    }
    pub fn len(&self) -> usize {
        self.entities.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }
}
