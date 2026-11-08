use std::fmt;

/// A generational entity handle. Stale handles (to despawned entities) are detected.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    /// A handle that never refers to a live entity. Useful as a default.
    pub const PLACEHOLDER: Entity = Entity { index: u32::MAX, generation: u32::MAX };

    pub fn index(self) -> u32 {
        self.index
    }
    pub fn generation(self) -> u32 {
        self.generation
    }
    pub fn to_bits(self) -> u64 {
        (self.generation as u64) << 32 | self.index as u64
    }
    pub fn from_bits(bits: u64) -> Entity {
        Entity { index: bits as u32, generation: (bits >> 32) as u32 }
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}
impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Location {
    pub archetype: u32,
    pub row: u32,
}

struct Meta {
    generation: u32,
    location: Option<Location>,
}

#[derive(Default)]
pub(crate) struct Entities {
    meta: Vec<Meta>,
    free: Vec<u32>,
    len: usize,
}

impl Entities {
    pub fn alloc(&mut self, location: Location) -> Entity {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let m = &mut self.meta[index as usize];
            m.location = Some(location);
            Entity { index, generation: m.generation }
        } else {
            let index = self.meta.len() as u32;
            assert!(index != u32::MAX, "entity index space exhausted");
            self.meta.push(Meta { generation: 0, location: Some(location) });
            Entity { index, generation: 0 }
        }
    }

    pub fn free(&mut self, e: Entity) -> bool {
        match self.meta.get_mut(e.index as usize) {
            Some(m) if m.generation == e.generation && m.location.is_some() => {
                m.location = None;
                m.generation = m.generation.wrapping_add(1);
                self.free.push(e.index);
                self.len -= 1;
                true
            }
            _ => false,
        }
    }

    pub fn location(&self, e: Entity) -> Option<Location> {
        self.meta
            .get(e.index as usize)
            .filter(|m| m.generation == e.generation)
            .and_then(|m| m.location)
    }

    pub fn set_location(&mut self, e: Entity, loc: Location) {
        let m = &mut self.meta[e.index as usize];
        debug_assert_eq!(m.generation, e.generation);
        m.location = Some(loc);
    }

    pub fn len(&self) -> usize {
        self.len
    }
}
