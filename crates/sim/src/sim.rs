//! The [`Sim`] facade: owns the ECS world and schedule, and exposes every player command.
//! UI and tools must mutate the simulation only through these methods so runs stay deterministic.

use std::sync::Arc;

use glam::{IVec2, Vec3};
use hearth_ecs::{Entity, Schedule, Without, World};
use serde::{Deserialize, Serialize};

use crate::actions;
use crate::components::*;
use crate::data::{GameData, LifeStage, Need, Skill};
use crate::economy::Household;
use crate::log::{LogKind, SimLog};
use crate::lot::Lot;
use crate::objects::ObjectInstance;
use crate::rng::SimRng;
use crate::social::Relationships;
use crate::time::{Clock, Settings};
use crate::{autonomy, career, economy, life, needs};

/// Shared, immutable content.
#[derive(Clone)]
pub struct Data(pub Arc<GameData>);

// ------------------------------------------------------------------------------------------------
// helpers used across systems

pub(crate) fn data(world: &World) -> Arc<GameData> {
    world.res::<Data>().0.clone()
}

pub(crate) fn now(world: &World) -> u64 {
    world.res::<Clock>().tick
}

pub(crate) fn name_of(world: &World, e: Entity) -> String {
    world.get::<Peon>(e).map(|p| p.first.clone()).unwrap_or_else(|| "Someone".into())
}

pub(crate) fn log(world: &World, kind: LogKind, subject: Option<Entity>, text: impl Into<String>) {
    let t = now(world);
    world.res_mut::<SimLog>().push(t, kind, subject, text);
}

/// Fills `{name}` (and `{item}`) placeholders.
pub(crate) fn fill(world: &World, e: Entity, text: &str) -> String {
    let mut s = text.replace("{name}", &name_of(world, e));
    if s.contains("{item}") {
        const ITEMS: [&str; 8] = [
            "Slicemaster Supreme",
            "self-stirring pot",
            "hat with a built-in umbrella",
            "banana protector",
            "18-piece knife set (17 are spoons)",
            "Snuggie for dogs",
            "inflatable office chair",
            "set of motivational socks",
        ];
        let i = world.res_mut::<SimRng>().below(ITEMS.len());
        s = s.replace("{item}", ITEMS[i]);
    }
    s
}

pub(crate) fn add_moodlet(world: &World, e: Entity, id: &str) {
    let data = data(world);
    let Some(def) = data.moodlet(id) else {
        log::warn!("unknown moodlet {id}");
        return;
    };
    let until = now(world) + (def.hours * 60.0) as u64;
    if let Some(mut m) = world.get_mut::<Mood>(e) {
        if let Some(existing) = m.moodlets.iter_mut().find(|x| x.id == id) {
            existing.until = existing.until.max(until);
        } else {
            m.moodlets.push(ActiveMoodlet { id: id.into(), name: def.name.clone(), value: def.value, until });
        }
    }
}

pub(crate) fn set_bubble(world: &World, e: Entity, icon: Option<String>, speech: Option<String>, minutes: u64) {
    let until = now(world) + minutes;
    if let Some(mut b) = world.get_mut::<Bubble>(e) {
        *b = Bubble { icon, speech, until };
    }
}

pub(crate) fn say(world: &World, e: Entity, icon: Option<&str>, minutes: u64) {
    let data = data(world);
    let words = 1 + world.res_mut::<SimRng>().below(3);
    let speech = crate::speech::gibberish(&mut world.res_mut::<SimRng>(), &data.names.syllables, words);
    set_bubble(world, e, icon.map(String::from), Some(speech), minutes);
}

/// Peons on the lot that are alive.
pub fn present_peons(world: &World) -> Vec<Entity> {
    let mut v: Vec<Entity> = world
        .query_filtered::<Entity, (hearth_ecs::With<Peon>, Without<Dead>, Without<AtWork>, Without<Offlot>)>()
        .iter()
        .collect();
    v.sort();
    v
}

// ------------------------------------------------------------------------------------------------

/// Everything needed to create a peon.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeonSpec {
    pub first: String,
    pub last: String,
    pub stage: LifeStage,
    pub axes: Axes,
    pub traits: Vec<String>,
    pub quirk: String,
    pub appearance: Appearance,
    pub skills: Vec<(Skill, f32)>,
    pub career: Option<String>,
    /// Inheritable genotype; derived from `appearance` when absent.
    #[serde(default)]
    pub genes: Option<Genes>,
    #[serde(default)]
    pub sex: Sex,
}

pub fn random_spec(world: &World, stage: LifeStage, last: Option<&str>) -> PeonSpec {
    let data = data(world);
    let mut rng = world.res_mut::<SimRng>().fork();
    let taken: Vec<String> = world
        .query::<&Peon>()
        .iter()
        .map(|p| p.first.clone())
        .collect();
    let mut first = rng.pick(&data.names.first).cloned().unwrap_or_else(|| "Pat".into());
    for _ in 0..50 {
        if !taken.contains(&first) {
            break;
        }
        first = rng.pick(&data.names.first).cloned().unwrap();
    }
    let last = last.map(String::from).unwrap_or_else(|| rng.pick(&data.names.last).cloned().unwrap());
    // axes: distribute 25 points
    let mut axes = [0u8; 5];
    let mut pts = 25;
    while pts > 0 {
        let i = rng.below(5);
        if axes[i] < 10 {
            axes[i] += 1;
            pts -= 1;
        }
    }
    let n_traits = if stage <= LifeStage::Toddler { 1 } else if stage <= LifeStage::Teen { 2 } else { 3 };
    let mut traits: Vec<String> = Vec::new();
    let mut guard = 0;
    while traits.len() < n_traits && guard < 100 {
        guard += 1;
        let t = rng.pick(&data.traits).unwrap();
        let clash = traits.iter().any(|x| x == &t.id || t.conflicts.contains(x));
        if !clash {
            traits.push(t.id.clone());
        }
    }
    let quirk = rng.pick(&data.quirks).unwrap().id.clone();
    let skin_tones = [[0.96, 0.8, 0.69], [0.87, 0.67, 0.5], [0.72, 0.5, 0.36], [0.55, 0.37, 0.25], [0.36, 0.24, 0.16]];
    let hair = [[0.1, 0.07, 0.05], [0.35, 0.22, 0.1], [0.75, 0.6, 0.3], [0.6, 0.25, 0.1], [0.85, 0.85, 0.85], [0.3, 0.6, 0.55]];
    let rcol = |rng: &mut SimRng| [rng.range_f32(0.15, 0.9), rng.range_f32(0.15, 0.9), rng.range_f32(0.15, 0.9)];
    let genes = Genes {
        skin: *rng.pick(&skin_tones).unwrap(),
        hair: *rng.pick(&hair).unwrap(),
        eyes: *rng.pick(&crate::genetics::EYE_COLORS).unwrap(),
        height: rng.range_f32(0.92, 1.08),
        build: rng.range_f32(0.0, 1.0),
        shape: Default::default(),
    };
    let mut worn = Appearance { shirt: rcol(&mut rng), pants: rcol(&mut rng), ..Default::default() };
    worn.hair_style = rng.below(4) as u8;
    let mut genes = genes;
    let appearance = crate::genetics::express(&genes, stage, &worn);
    let career = if stage.is_adult() && stage != LifeStage::Elder && rng.chance(0.8) {
        let adult: Vec<_> = data.careers.iter().filter(|c| c.min_stage >= LifeStage::YoungAdult).collect();
        Some(rng.pick(&adult).unwrap().id.clone())
    } else {
        None
    };
    // picked last so the rest of a spec doesn't depend on it
    let sex = if rng.chance(0.5) { Sex::Female } else { Sex::Male };
    // face, body, hair and outfit by sex, from their own stream so they don't shift the rest
    let mut look = rng.fork();
    genes.shape = crate::genetics::random_shape(&mut look, sex);
    let mut appearance = crate::genetics::express(&genes, stage, &appearance);
    crate::genetics::random_style(&mut look, sex, stage, &mut appearance);
    PeonSpec {
        first,
        last,
        stage,
        axes: Axes { neat: axes[0], outgoing: axes[1], active: axes[2], playful: axes[3], nice: axes[4] },
        traits,
        quirk,
        appearance,
        skills: Vec::new(),
        career,
        genes: Some(genes),
        sex,
    }
}

pub fn spawn_peon(world: &mut World, spec: PeonSpec, pos: Vec3) -> Entity {
    spawn_peon_as(world, spec, pos, true)
}

/// Spawns a peon; household members are logged as moving in and added to the [`Household`].
pub fn spawn_peon_as(world: &mut World, spec: PeonSpec, pos: Vec3, member: bool) -> Entity {
    let data = data(world);
    let scale = world.res::<Settings>().lifespan_scale;
    let stage_len = spec.stage.base_days() * scale * world.res_mut::<SimRng>().range_f32(0.9, 1.1);
    let mut needs = Needs::new();
    for t in &spec.traits {
        if let Some(n) = data.trait_def(t).and_then(|d| d.grants) {
            needs.active[n.idx()] = true;
        }
    }
    for n in Need::ALL {
        needs.v[n.idx()] = world.res_mut::<SimRng>().range_f32(60.0, 95.0);
    }
    let mut skills = Skills::default();
    for (s, v) in &spec.skills {
        skills.v[s.idx()] = *v;
    }
    let e = world.spawn((
        Peon { first: spec.first.clone(), last: spec.last.clone() },
        Age { stage: spec.stage, days_in_stage: 0.0, stage_len, total_days: 0.0 },
        needs,
        Personality { axes: spec.axes, traits: spec.traits.clone(), quirk: spec.quirk.clone() },
        skills,
        Mood::default(),
        Transform::at(pos),
        spec.appearance.clone(),
        Posture::default(),
        Bubble::default(),
        ActionQueue::default(),
        Brain::default(),
    ));
    let genes = spec.genes.clone().unwrap_or_else(|| crate::genetics::genes_from_appearance(&spec.appearance));
    world.insert(e, (Family::default(), genes, spec.sex));
    if member {
        world.insert_one(e, Member);
    }
    if matches!(spec.stage, LifeStage::Child | LifeStage::Teen) {
        world.insert_one(e, Student::new());
    }
    if let Some(c) = &spec.career
        && data.career(c).is_some() {
            world.insert_one(e, Job { career: c.clone(), level: 0, performance: 0.0, days_worked: 0, last_shift_day: None });
        }
    if member {
        world.res_mut::<Household>().members.push(e);
        log(world, LogKind::Life, Some(e), format!("{} {} moved in.", spec.first, spec.last));
    }
    e
}


pub fn can_place(world: &World, def_id: &str, level: i32, tile: IVec2, rot: u8) -> Result<(), String> {
    let data = data(world);
    let id = data.object_id(def_id).ok_or("unknown object")?;
    let def = data.object(id);
    let inst = ObjectInstance::new(id, def, level, tile, rot);
    let lot = world.res::<Lot>();
    for t in inst.footprint_tiles(def) {
        if !lot.has_ground(level, t) {
            return Err("nothing to stand on".into());
        }
        if lot.occupancy.contains_key(&(level, t.x, t.y)) {
            return Err("tile occupied".into());
        }
        if lot.stair_at(level, t).is_some() {
            return Err("stairs in the way".into());
        }
        if level == 0 && !lot.has_floor(0, t) && !lot.terrain.tile_flat(t) {
            return Err("the ground is too uneven".into());
        }
    }
    // walls may not cross the footprint interior
    let tiles = inst.footprint_tiles(def);
    for a in &tiles {
        for d in [IVec2::X, IVec2::Y] {
            let b = *a + d;
            if tiles.contains(&b)
                && let Some(e) = crate::lot::Edge::between(level, *a, b)
                    && lot.walls.contains_key(&e) {
                        return Err("a wall is in the way".into());
                    }
        }
    }
    Ok(())
}

/// Buys (if `charge`) and places an object.
pub fn place_object(world: &mut World, def_id: &str, level: i32, tile: IVec2, rot: u8, charge: bool) -> Result<Entity, String> {
    can_place(world, def_id, level, tile, rot)?;
    let data = data(world);
    let id = data.object_id(def_id).unwrap();
    let def = data.object(id);
    if charge {
        let mut hh = world.res_mut::<Household>();
        if hh.funds < def.price {
            return Err("can't afford it".into());
        }
        hh.funds -= def.price;
    }
    let mut inst = ObjectInstance::new(id, def, level, tile, rot);
    if level == 0 && !world.res::<Lot>().has_floor(0, tile) {
        inst.ground = world.res::<Lot>().terrain.corner(tile);
    }
    let tiles = inst.footprint_tiles(def);
    let walkable = def.walkable;
    let e = world.spawn((inst,));
    let mut lot = world.res_mut::<Lot>();
    if !walkable {
        for t in tiles {
            lot.occupancy.insert((level, t.x, t.y), e);
        }
    }
    lot.rooms_dirty = true;
    lot.version += 1;
    Ok(e)
}

/// Takes an object off the lot (whoever was using it stops).
pub fn remove_object(world: &mut World, e: Entity) -> Option<ObjectInstance> {
    let inst = world.remove::<ObjectInstance>(e)?;
    for u in inst.slot_users.iter().flatten() {
        actions::abort_current(world, *u);
    }
    world.despawn(e);
    let mut lot = world.res_mut::<Lot>();
    lot.occupancy.retain(|_, v| *v != e);
    lot.rooms_dirty = true;
    lot.version += 1;
    Some(inst)
}

pub struct Sim {
    pub world: World,
    schedule: Schedule,
}

impl Sim {
    pub fn new(seed: u64, data: GameData) -> Sim {
        let mut world = World::new();
        world.insert_resource(Data(Arc::new(data)));
        world.insert_resource(Clock::default());
        world.insert_resource(Settings::default());
        world.insert_resource(SimRng::new(seed));
        world.insert_resource(SimLog::default());
        world.insert_resource(Lot::new(32, 32));
        world.insert_resource(Relationships::default());
        world.insert_resource(Household::default());
        world.insert_resource(career::PendingChoices::default());
        world.insert_resource(actions::NextActionId(1));
        world.insert_resource(needs::Nanny::default());
        world.insert_resource(crate::neighborhood::Neighborhood::default());
        for_debug(&mut world);

        let mut schedule = Schedule::new();
        schedule.add_stage("pre").add_stage("needs").add_stage("act").add_stage("post");
        schedule
            .add_system("pre", advance_clock)
            .add_system("pre", needs::update_rooms)
            .add_system("needs", needs::decay_needs)
            .add_system("needs", needs::update_mood)
            .add_system("act", career::schedule_work)
            .add_system("act", crate::school::schedule_school)
            .add_system("act", actions::run_actions)
            .add_system("post", needs::consequences)
            .add_system("post", career::run_shifts)
            .add_system("post", crate::school::run_school)
            .add_system("post", crate::family::pregnancies)
            .add_system("post", crate::ghosts::haunt)
            .add_system("post", crate::neighborhood::trips)
            .add_system("post", crate::neighborhood::visits)
            .add_system("post", crate::neighborhood::offlot_daily)
            .add_system("post", crate::romance::weddings)
            .add_system("post", crate::romance::daily)
            .add_system("post", crate::neighborhood::newcomers)
            .add_system("post", life::aging)
            .add_system("post", economy::bills)
            .add_system("post", daily_upkeep);
        Sim { world, schedule }
    }

    /// Advance one game minute.
    pub fn tick(&mut self) {
        self.schedule.run(&mut self.world);
    }

    pub fn run_minutes(&mut self, n: u64) {
        for _ in 0..n {
            self.tick();
        }
    }

    pub fn run_days(&mut self, n: u64) {
        self.run_minutes(n * crate::time::TICKS_PER_DAY);
    }

    pub fn timings(&self) -> Vec<hearth_ecs::SystemTiming> {
        self.schedule.timings()
    }

    pub fn clock(&self) -> Clock {
        *self.world.res::<Clock>()
    }

    pub fn data(&self) -> Arc<GameData> {
        data(&self.world)
    }

    /// All peons (alive or dead), sorted.
    pub fn peons(&self) -> Vec<Entity> {
        let mut v: Vec<Entity> = self.world.query_filtered::<Entity, hearth_ecs::With<Peon>>().iter().collect();
        v.sort();
        v
    }

    pub fn living_peons(&self) -> Vec<Entity> {
        let mut v: Vec<Entity> =
            self.world.query_filtered::<Entity, (hearth_ecs::With<Peon>, Without<Dead>)>().iter().collect();
        v.sort();
        v
    }

    pub fn objects(&self) -> Vec<Entity> {
        let mut v: Vec<Entity> = self.world.query_filtered::<Entity, hearth_ecs::With<ObjectInstance>>().iter().collect();
        v.sort();
        v
    }

    pub fn name(&self, e: Entity) -> String {
        self.world.get::<Peon>(e).map(|p| p.full_name()).unwrap_or_default()
    }

    // -------------------------------------------------------------------- peons

    pub fn random_spec(&mut self, stage: LifeStage, last: Option<&str>) -> PeonSpec {
        random_spec(&self.world, stage, last)
    }

    pub fn spawn_peon(&mut self, spec: PeonSpec, pos: Vec3) -> Entity {
        spawn_peon(&mut self.world, spec, pos)
    }

    /// Records parent/child links (for genetics and romance rules).
    pub fn set_parent(&mut self, parent: Entity, child: Entity) {
        crate::family::set_parent(&mut self.world, parent, child);
    }

    pub fn marry(&mut self, a: Entity, b: Entity) {
        crate::family::marry(&mut self.world, a, b);
    }

    /// Adopts a child (charges the agency fee).
    pub fn adopt(&mut self, stage: LifeStage) -> Result<Entity, String> {
        let r = crate::family::adopt(&mut self.world, stage);
        self.world.flush();
        r
    }

    // -------------------------------------------------------------------- orders

    /// What can `peon` do with `target` (an object or another peon)? For pie menus.
    pub fn options(&self, peon: Entity, target: Entity) -> Vec<(String, ActionKind, Result<(), String>)> {
        autonomy::menu_options(&self.world, peon, target)
    }

    /// Player order. Returns Err with a reason if the peon refuses or it is impossible.
    pub fn order(&mut self, peon: Entity, kind: ActionKind) -> Result<(), String> {
        actions::player_order(&mut self.world, peon, kind)
    }

    /// Cancel a queued action, or the current one (by id).
    pub fn cancel(&mut self, peon: Entity, action_id: u64) {
        actions::cancel(&mut self.world, peon, action_id);
    }

    pub fn set_free_will(&mut self, peon: Option<Entity>, fw: crate::time::FreeWill) {
        match peon {
            Some(p) => {
                if let Some(mut b) = self.world.get_mut::<Brain>(p) {
                    b.free_will = Some(fw);
                }
            }
            None => self.world.res_mut::<Settings>().free_will = fw,
        }
    }

    pub fn resolve_choice(&mut self, index: usize, choice: usize) {
        career::resolve_pending(&mut self.world, index, choice);
    }

    // -------------------------------------------------------------------- neighborhood

    /// Fills out the street (neighbors, a house for sale, empty plots, a park).
    pub fn populate_neighborhood(&mut self) {
        crate::neighborhood::populate(&mut self.world);
        self.world.flush();
    }

    /// Switches to the household living on `lot`.
    pub fn play_household(&mut self, lot: usize) -> Result<(), String> {
        let r = crate::neighborhood::play_household(&mut self.world, lot);
        self.world.flush();
        r
    }

    /// Sends the household out to a community lot (on foot; see [`Sim::finish_trip`]).
    pub fn go_out(&mut self, lot: usize) -> Result<(), String> {
        let r = crate::neighborhood::go_out(&mut self.world, lot);
        self.world.flush();
        r
    }

    /// Sends the household over to a neighbor's home (on foot; see [`Sim::finish_trip`]).
    pub fn visit(&mut self, lot: usize) -> Result<(), String> {
        let r = crate::neighborhood::visit(&mut self.world, lot);
        self.world.flush();
        r
    }

    /// The household's walk to another lot, if one is underway.
    pub fn trip(&self) -> Option<crate::neighborhood::Trip> {
        crate::neighborhood::trip(&self.world)
    }

    /// Runs the clock until the household's walk (if any) is over: they've left, the destination
    /// is live, and they've walked onto it. Returns the minutes it took.
    pub fn finish_trip(&mut self) -> u32 {
        let mut n = 0;
        while self.trip().is_some() && n < 24 * 60 {
            self.tick();
            n += 1;
        }
        n
    }

    /// Sends the household home (on foot; see [`Sim::finish_trip`]).
    pub fn go_home(&mut self) -> Result<(), String> {
        let r = crate::neighborhood::go_home(&mut self.world);
        self.world.flush();
        r
    }

    /// Sells the home and moves the household to a vacant lot.
    pub fn move_household(&mut self, lot: usize) -> Result<(), String> {
        let r = crate::neighborhood::move_household(&mut self.world, lot);
        self.world.flush();
        r
    }

    /// Parks the played household and starts a new one on a vacant lot; members are spawned
    /// next with [`Sim::spawn_peon`] at [`crate::neighborhood::move_in_spot`].
    pub fn found_household(&mut self, lot: usize, name: String, funds: i64) -> Result<(), String> {
        let r = crate::neighborhood::found_household(&mut self.world, lot, name, funds);
        self.world.flush();
        r
    }

    /// Asks a neighbor over.
    pub fn invite(&mut self, e: Entity) {
        crate::neighborhood::invite(&mut self.world, e);
    }

    // -------------------------------------------------------------------- build / buy

    pub fn can_place(&self, def_id: &str, level: i32, tile: IVec2, rot: u8) -> Result<(), String> {
        can_place(&self.world, def_id, level, tile, rot)
    }

    /// Buys (if `charge`) and places an object.
    pub fn place_object(&mut self, def_id: &str, level: i32, tile: IVec2, rot: u8, charge: bool) -> Result<Entity, String> {
        place_object(&mut self.world, def_id, level, tile, rot, charge)
    }

    /// Sells an object for its depreciated value.
    pub fn sell_object(&mut self, e: Entity) -> Option<i64> {
        let inst = remove_object(&mut self.world, e)?;
        let value = (inst.price_paid as f32 * if inst.uses == 0 { 1.0 } else { 0.8 } * if inst.broken { 0.5 } else { 1.0 }) as i64;
        self.world.res_mut::<Household>().funds += value;
        Some(value)
    }

    /// Direct lot access for build tools. Remember costs are charged by the caller via `charge`.
    pub fn lot_mut(&mut self) -> hearth_ecs::RefMut<'_, Lot> {
        self.world.res_mut::<Lot>()
    }

    pub fn charge(&mut self, amount: i64) -> bool {
        let mut hh = self.world.res_mut::<Household>();
        if hh.funds < amount {
            return false;
        }
        hh.funds -= amount;
        true
    }

    /// Summary for logging/tests.
    pub fn summary(&self) -> serde_json::Value {
        let mut peons = Vec::new();
        for e in self.peons() {
            let w = &self.world;
            let p = w.get::<Peon>(e).unwrap();
            let age = w.get::<Age>(e).unwrap();
            let needs = w.get::<Needs>(e).unwrap();
            let mood = w.get::<Mood>(e).unwrap();
            let skills = w.get::<Skills>(e).unwrap();
            let cur = w.get::<ActionQueue>(e).unwrap().current.as_ref().map(|c| c.action.label.clone());
            let job = w.get::<Job>(e).map(|j| {
                let d = self.data();
                let c = d.career(&j.career).unwrap();
                format!("{} L{} ({}) perf {:.0}", c.name, j.level + 1, c.levels[j.level].title, j.performance)
            });
            let mut nv = serde_json::Map::new();
            for n in needs.active_needs() {
                nv.insert(n.label().into(), serde_json::json!((needs.get(n) * 10.0).round() / 10.0));
            }
            peons.push(serde_json::json!({
                "name": p.full_name(),
                "stage": age.stage.label(),
                "age_days": (age.total_days * 10.0).round() / 10.0,
                "dead": w.get::<Dead>(e).map(|d| d.cause.clone()),
                "at_work": w.has::<AtWork>(e),
                "mood": mood.value.round(),
                "needs": nv,
                "skills": Skill::ALL.iter().filter(|s| skills.get(**s) >= 0.1).map(|s| format!("{:?} {:.1}", s, skills.get(*s))).collect::<Vec<_>>(),
                "doing": cur,
                "job": job,
            }));
        }
        let hh = self.world.res::<Household>();
        serde_json::json!({
            "time": self.clock().label(),
            "funds": hh.funds,
            "peons": peons,
        })
    }
}

fn advance_clock(world: &mut World) {
    world.res_mut::<Clock>().tick += 1;
    for t in world.query::<&mut Transform>().iter() {
        t.prev_pos = t.pos;
        t.prev_yaw = t.yaw;
    }
}

fn daily_upkeep(world: &mut World) {
    let clock = *world.res::<Clock>();
    if clock.minute_of_day() == 0 {
        world.res_mut::<Relationships>().daily_decay();
    }
    // expire bubbles
    let t = clock.tick;
    for b in world.query::<&mut Bubble>().iter() {
        if b.until <= t {
            b.icon = None;
            b.speech = None;
        }
    }
}

fn for_debug(world: &mut World) {
    world.register_debug::<Peon>();
    world.register_debug::<Age>();
    world.register_debug::<Needs>();
    world.register_debug::<Personality>();
    world.register_debug::<Skills>();
    world.register_debug::<Mood>();
    world.register_debug::<Transform>();
    world.register_debug::<Posture>();
    world.register_debug::<ActionQueue>();
    world.register_debug::<Brain>();
    world.register_debug::<Job>();
    world.register_debug::<AtWork>();
    world.register_debug::<Family>();
    world.register_debug::<Dead>();
    world.register_debug::<ObjectInstance>();
    world.register_debug::<Appearance>();
    world.register_debug::<Genes>();
    world.register_debug::<Student>();
    world.register_debug::<Pregnant>();
    world.register_debug::<Ghost>();
    world.register_debug::<Offlot>();
    world.register_debug::<Visitor>();
}
