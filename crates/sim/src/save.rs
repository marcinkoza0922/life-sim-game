//! Save games. The whole simulation state (resources + every entity's components) is written as
//! RON. Entity handles are restored exactly (index and generation), so references between
//! entities (families, relationships, actions, slot users) need no remapping. Object definitions
//! are stored by string id so content edits between versions don't scramble furniture.

use std::path::Path;

use hearth_ecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::actions::NextActionId;
use crate::career::PendingChoices;
use crate::components::*;
use crate::data::GameData;
use crate::economy::Household;
use crate::log::SimLog;
use crate::lot::Lot;
use crate::needs::Nanny;
use crate::neighborhood::Neighborhood;
use crate::objects::ObjectInstance;
use crate::rng::SimRng;
use crate::sim::{Sim, data};
use crate::social::{Rel, Relationships};
use crate::time::{Clock, Settings};

pub const SAVE_VERSION: u32 = 2;

/// Every component the sim knows about, for one entity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedEntity {
    pub id: Entity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peon: Option<Peon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age: Option<Age>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<Needs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personality: Option<Personality>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Skills>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mood: Option<Mood>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posture: Option<Posture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bubble: Option<Bubble>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<ActionQueue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain: Option<Brain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<Family>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub member: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_work: Option<AtWork>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead: Option<Dead>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genes: Option<Genes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub student: Option<Student>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pregnant: Option<Pregnant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ghost: Option<Ghost>,
    /// Old saves (v1): a neighbor with an address but no house.
    #[serde(default, skip_serializing)]
    pub townie: Option<crate::neighborhood::OldTownie>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offlot: Option<Offlot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visitor: Option<Visitor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectInstance>,
    /// String id of `object.def`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_def: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaveGame {
    pub version: u32,
    pub clock: Clock,
    pub settings: Settings,
    pub rng: SimRng,
    pub log: SimLog,
    pub lot: Lot,
    pub relationships: Vec<(Entity, Entity, Rel)>,
    pub household: Household,
    pub pending: PendingChoices,
    pub nanny: Nanny,
    pub next_action_id: u64,
    /// v1 saves: just the street's name.
    #[serde(default, skip_serializing)]
    pub neighborhood: Option<crate::neighborhood::OldNeighborhood>,
    /// v2+: every lot and household on the street.
    #[serde(default)]
    pub hood: Option<Neighborhood>,
    pub entities: Vec<SavedEntity>,
    /// Entity allocator free list, so loaded games hand out the same handles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub free: Vec<Entity>,
}

fn take<T: hearth_ecs::Component + Clone>(world: &World, e: Entity) -> Option<T> {
    world.get::<T>(e).map(|c| c.clone())
}

/// Snapshot of the whole simulation.
pub fn capture(world: &World) -> SaveGame {
    let data = data(world);
    let mut entities: Vec<SavedEntity> = world
        .entities()
        .into_iter()
        .map(|e| {
            let object = take::<ObjectInstance>(world, e);
            SavedEntity {
                id: e,
                peon: take(world, e),
                age: take(world, e),
                needs: take(world, e),
                personality: take(world, e),
                skills: take(world, e),
                mood: take(world, e),
                transform: take(world, e),
                appearance: take(world, e),
                posture: take(world, e),
                bubble: take(world, e),
                actions: take(world, e),
                brain: take(world, e),
                family: take(world, e),
                member: world.has::<Member>(e),
                job: take(world, e),
                at_work: take(world, e),
                dead: take(world, e),
                genes: take(world, e),
                student: take(world, e),
                pregnant: take(world, e),
                ghost: take(world, e),
                townie: None,
                offlot: take(world, e),
                visitor: take(world, e),
                object_def: object.as_ref().map(|o| data.object(o.def).id.clone()),
                object,
            }
        })
        .collect();
    entities.sort_by_key(|s| s.id);
    SaveGame {
        version: SAVE_VERSION,
        clock: *world.res::<Clock>(),
        settings: world.res::<Settings>().clone(),
        rng: world.res::<SimRng>().clone(),
        log: world.res::<SimLog>().clone(),
        lot: world.res::<Lot>().clone(),
        relationships: world.res::<Relationships>().map.iter().map(|(&(a, b), r)| (a, b, *r)).collect(),
        household: world.res::<Household>().clone(),
        pending: world.res::<PendingChoices>().clone(),
        nanny: world.res::<Nanny>().clone(),
        next_action_id: world.res::<NextActionId>().0,
        neighborhood: None,
        hood: Some(world.res::<Neighborhood>().clone()),
        entities,
        free: world.free_list(),
    }
}

/// Rebuilds a running sim from a snapshot, using the given content.
pub fn restore(save: SaveGame, data: GameData) -> Result<Sim, String> {
    if save.version > SAVE_VERSION {
        return Err(format!("save version {} is newer than this game ({SAVE_VERSION})", save.version));
    }
    let mut sim = Sim::new(0, data);
    let data = sim.data();
    let w = &mut sim.world;
    w.insert_resource(save.clock);
    w.insert_resource(save.settings);
    w.insert_resource(save.rng);
    w.insert_resource(save.log);
    w.insert_resource(Relationships { map: save.relationships.into_iter().map(|(a, b, r)| ((a, b), r)).collect() });
    w.insert_resource(save.household);
    w.insert_resource(save.pending);
    w.insert_resource(save.nanny);
    w.insert_resource(NextActionId(save.next_action_id));
    let old_hood = save.neighborhood;
    let migrating = save.hood.is_none();
    w.insert_resource(save.hood.unwrap_or_default());
    let mut townies = Vec::new();
    let mut lot = save.lot;
    lot.occupancy.clear();
    for s in save.entities {
        let e = s.id;
        if !w.spawn_empty_at(e) {
            return Err(format!("duplicate entity {e:?} in save"));
        }
        macro_rules! put {
            ($($field:ident),*) => { $( if let Some(c) = s.$field { w.insert_one(e, c); } )* };
        }
        let away = s.at_work.is_some();
        put!(peon, age, needs, personality, skills, mood, transform, appearance, posture, bubble, actions, brain, family, job, at_work, dead, genes, student, pregnant, ghost, offlot, visitor);
        if let Some(t) = s.townie {
            townies.push((e, t, away));
        }
        if s.member {
            w.insert_one(e, Member);
        }
        if let Some(mut o) = s.object {
            let def_id = s.object_def.as_deref().unwrap_or_default();
            let Some(id) = data.object_id(def_id) else {
                log::warn!("save: dropping object with unknown definition '{def_id}'");
                w.despawn(e);
                continue;
            };
            o.def = id;
            let def = data.object(id);
            o.slot_users.resize(def.slots.len(), None);
            if !def.walkable {
                for t in o.footprint_tiles(def) {
                    lot.occupancy.insert((o.level, t.x, t.y), e);
                }
            }
            w.insert_one(e, o);
        }
    }
    if !save.free.is_empty() {
        w.set_free_list(&save.free);
    }
    lot.rooms_dirty = true;
    lot.version += 1;
    w.insert_resource(lot);
    if migrating {
        crate::neighborhood::migrate(w, old_hood.map(|h| h.name), townies);
        w.flush();
    } else if w.res::<Neighborhood>().lots.len() > 1 {
        // streets saved before the café and gym opened
        crate::neighborhood::add_community_lots(w);
    }
    Ok(sim)
}

pub fn to_string(save: &SaveGame) -> Result<String, String> {
    ron::ser::to_string(save).map_err(|e| e.to_string())
}

pub fn from_str(text: &str) -> Result<SaveGame, String> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(text)
        .map_err(|e| e.to_string())
}

impl Sim {
    pub fn save_to_string(&self) -> Result<String, String> {
        to_string(&capture(&self.world))
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let text = self.save_to_string()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // write-then-rename so a crash mid-save never eats the previous save
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn load_from_str(text: &str, data: GameData) -> Result<Sim, String> {
        restore(from_str(text)?, data)
    }

    pub fn load_from_file(path: &Path, data: GameData) -> Result<Sim, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::load_from_str(&text, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starter::starter_sim;

    #[test]
    fn save_load_continues_identically() {
        let mut a = starter_sim(17);
        a.run_minutes(9 * 60 + 13);
        let text = a.save_to_string().unwrap();
        let mut b = Sim::load_from_str(&text, GameData::embedded()).unwrap();
        assert_eq!(a.summary(), b.summary());
        // determinism survives the round trip: both continue to the same future
        a.run_days(2);
        b.run_days(2);
        assert_eq!(a.summary(), b.summary());
        let la: Vec<String> = a.world.res::<SimLog>().entries.iter().map(|e| e.text.clone()).collect();
        let lb: Vec<String> = b.world.res::<SimLog>().entries.iter().map(|e| e.text.clone()).collect();
        assert_eq!(la, lb);
        // and saving again gives the same file
        assert_eq!(a.save_to_string().unwrap(), b.save_to_string().unwrap());
    }

    #[test]
    fn unknown_objects_are_dropped_not_fatal() {
        let sim = starter_sim(1);
        let text = sim.save_to_string().unwrap().replace("\"tv_basic\"", "\"tv_from_the_future\"");
        let b = Sim::load_from_str(&text, GameData::embedded()).unwrap();
        assert_eq!(b.objects().len(), sim.objects().len() - 1);
    }
}
