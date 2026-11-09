//! The neighborhood: a street of lots, the households living on them, and community lots.
//!
//! Only one lot is live at a time: the [`Lot`] resource plus its object entities. The others are
//! parked here as plain data ([`ParkedLot`]). Every person stays in the world wherever they are, so
//! relationships and family links span lots. People who aren't on the active lot carry [`Offlot`]
//! and are simulated roughly once a day ([`offlot_daily`]: pay, promotions, grades, bills,
//! births); aging runs for everyone on the shared clock. Neighbors drop by as [`Visitor`]s, and the
//! played household can go and visit them ([`visit`]): the hosts are at home on their own lot.
//! Getting anywhere is a walk down the sidewalk ([`Trip`], [`trips`]), and nobody sleeps over.
//! When the street thins out, [`newcomers`] move into vacant lots.

use std::sync::Arc;

use glam::{IVec2, Vec3};
use hearth_ecs::{Entity, With, Without, World};
use serde::{Deserialize, Serialize};

use crate::actions;
use crate::components::*;
use crate::data::{LifeStage, Need};
use crate::economy::Household;
use crate::log::LogKind;
use crate::lot::Lot;
use crate::objects::ObjectInstance;
use crate::rng::SimRng;
use crate::sim::{Data, data, log, name_of, now, random_spec, set_bubble, spawn_peon_as};
use crate::time::{Clock, TICKS_PER_DAY};

pub type LotId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LotKind {
    /// Somewhere to live: occupied by one household, or vacant and for sale.
    Residential,
    /// A public place households go out to (no residents, no build/buy).
    Community,
}

/// An object of a parked lot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParkedObject {
    /// Its entity while the lot was last live, so ghosts can find their grave again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Entity>,
    /// String id of the definition (content may change while a lot is parked).
    pub def: String,
    pub inst: ObjectInstance,
}

/// A lot that isn't live: its build and furniture as data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParkedLot {
    pub lot: Lot,
    pub objects: Vec<ParkedObject>,
}

impl ParkedLot {
    pub fn build_value(&self) -> i64 {
        crate::economy::build_value(&self.lot, self.objects.iter().map(|o| o.inst.price_paid))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LotSlot {
    pub address: String,
    /// Community lots have a name ("Gulch Green").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub kind: LotKind,
    /// What the land alone costs.
    pub land_price: i64,
    /// None while this is the active lot.
    pub parked: Option<ParkedLot>,
    /// Build value when it was last parked (for the active lot, use [`crate::economy::lot_value`]).
    #[serde(default)]
    pub value: i64,
}

impl LotSlot {
    pub fn title(&self) -> String {
        match &self.name {
            Some(n) => format!("{n} ({})", self.address),
            None => self.address.clone(),
        }
    }

    pub fn build_value(&self) -> i64 {
        self.parked.as_ref().map(|p| p.build_value()).unwrap_or(self.value)
    }
}

/// The street the household lives on, and everyone else on it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Neighborhood {
    pub name: String,
    pub lots: Vec<LotSlot>,
    /// The lot that is live in the world.
    pub active: LotId,
    /// Households not being played (the played one is the [`Household`] resource), by home lot.
    pub households: Vec<Household>,
    /// The played household walking to another lot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trip: Option<Trip>,
}

/// Why the household is walking somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TripKind {
    Visit,
    GoOut,
    Home,
}

/// The played household walking from one lot to another: first out to the sidewalk and off down
/// the street, then (once the destination is live) in from the end of the street they came from.
/// Babies are carried: they're off-lot for the whole walk.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trip {
    pub kind: TripKind,
    pub to: LotId,
    pub travelers: Vec<Entity>,
    /// false: leaving the old lot; true: walking onto the new one.
    pub arriving: bool,
    /// Stragglers are brought along at this tick.
    pub until: u64,
}

impl Default for Neighborhood {
    fn default() -> Self {
        Neighborhood {
            name: "Pleasant Gulch".into(),
            lots: vec![LotSlot { address: "12 Sycamore Lane".into(), name: None, kind: LotKind::Residential, land_price: 3000, parked: None, value: 0 }],
            active: 0,
            households: Vec::new(),
            trip: None,
        }
    }
}

/// Who lives on a lot, as seen from outside.
#[derive(Clone, Debug)]
pub struct Residents {
    pub name: String,
    pub members: Vec<Entity>,
    pub funds: i64,
    /// The household being played.
    pub played: bool,
}

const STREETS: [&str; 4] = ["Sycamore Lane", "Gravy Boat Court", "Mild Peril Avenue", "Ennui Street"];

// ------------------------------------------------------------------------------------------------
// queries

pub fn active_lot(world: &World) -> LotId {
    world.res::<Neighborhood>().active
}

pub fn active_kind(world: &World) -> LotKind {
    let hood = world.res::<Neighborhood>();
    hood.lots[hood.active].kind
}

/// Is the played household on its own lot?
pub fn at_home(world: &World) -> bool {
    world.res::<Household>().home == Some(active_lot(world))
}

/// Who lives on `lot` (None: vacant or community).
pub fn residents(world: &World, lot: LotId) -> Option<Residents> {
    let hh = world.res::<Household>();
    if hh.home == Some(lot) {
        return Some(Residents { name: hh.name.clone(), members: hh.members.clone(), funds: hh.funds, played: true });
    }
    let hood = world.res::<Neighborhood>();
    hood.households
        .iter()
        .find(|h| h.home == Some(lot))
        .map(|h| Residents { name: h.name.clone(), members: h.members.clone(), funds: h.funds, played: false })
}

/// Vacant residential lots.
pub fn vacant_lots(world: &World) -> Vec<LotId> {
    let n = world.res::<Neighborhood>().lots.len();
    (0..n)
        .filter(|&i| world.res::<Neighborhood>().lots[i].kind == LotKind::Residential && residents(world, i).is_none())
        .collect()
}

/// Build value of any lot.
pub fn build_value(world: &World, lot: LotId) -> i64 {
    if lot == active_lot(world) {
        crate::economy::lot_value(world)
    } else {
        world.res::<Neighborhood>().lots[lot].build_value()
    }
}

/// What it costs to buy a lot (land + whatever is built on it).
pub fn price(world: &World, lot: LotId) -> i64 {
    world.res::<Neighborhood>().lots[lot].land_price + build_value(world, lot)
}

/// The lot `e` lives on.
pub fn home_of(world: &World, e: Entity) -> Option<LotId> {
    let hh = world.res::<Household>();
    if hh.members.contains(&e) {
        return hh.home;
    }
    world.res::<Neighborhood>().households.iter().find(|h| h.members.contains(&e)).and_then(|h| h.home)
}

/// Neighbors (members of other households), alive or dead, sorted.
pub fn neighbors(world: &World) -> Vec<Entity> {
    let mut v: Vec<Entity> = world.res::<Neighborhood>().households.iter().flat_map(|h| h.members.iter().copied()).filter(|e| world.contains(*e)).collect();
    v.sort();
    v
}

/// Is `e` a guest on the live lot (a visiting neighbor, or the household away from home)?
pub fn is_guest(world: &World, e: Entity) -> bool {
    world.has::<Visitor>(e) || (world.has::<Member>(e) && !at_home(world))
}

/// Guests currently on the lot.
pub fn visitors(world: &World) -> Vec<Entity> {
    let mut v: Vec<Entity> = world.query_filtered::<Entity, (With<Visitor>, Without<Dead>)>().iter().collect();
    v.sort();
    v
}

// ------------------------------------------------------------------------------------------------
// parking lots

/// Takes the live lot out of the world: objects are despawned and kept as data.
pub fn park_active(world: &mut World) -> ParkedLot {
    let data = data(world);
    let mut objs: Vec<(Entity, ObjectInstance)> = world.query::<(Entity, &ObjectInstance)>().iter().map(|(e, o)| (e, o.clone())).collect();
    objs.sort_by_key(|(e, _)| *e);
    let mut objects = Vec::with_capacity(objs.len());
    for (e, mut inst) in objs {
        world.despawn(e);
        inst.slot_users.iter_mut().for_each(|u| *u = None);
        objects.push(ParkedObject { id: Some(e), def: data.object(inst.def).id.clone(), inst });
    }
    let (w, d, version) = {
        let lot = world.res::<Lot>();
        (lot.width, lot.depth, lot.version)
    };
    let mut empty = Lot::new(w, d);
    empty.version = version + 1;
    let mut lot = std::mem::replace(&mut *world.res_mut::<Lot>(), empty);
    lot.occupancy.clear();
    ParkedLot { lot, objects }
}

/// Makes a parked lot live. Objects get fresh entities; ghosts bound to a grave on this lot are
/// pointed at the new one. Returns the (old, new) entity pairs.
pub fn unpark(world: &mut World, parked: ParkedLot) -> Vec<(Entity, Entity)> {
    let data = data(world);
    let prev_version = world.res::<Lot>().version;
    let mut lot = parked.lot;
    lot.occupancy.clear();
    let mut map = Vec::new();
    for o in parked.objects {
        let Some(id) = data.object_id(&o.def) else {
            log::warn!("neighborhood: dropping object with unknown definition '{}'", o.def);
            continue;
        };
        let def = data.object(id);
        let mut inst = o.inst;
        inst.def = id;
        inst.slot_users = vec![None; def.slots.len()];
        let tiles = inst.footprint_tiles(def);
        let (walkable, level) = (def.walkable, inst.level);
        let e = world.spawn((inst,));
        if !walkable {
            for t in tiles {
                lot.occupancy.insert((level, t.x, t.y), e);
            }
        }
        if let Some(old) = o.id {
            map.push((old, e));
        }
    }
    lot.rooms_dirty = true;
    lot.version = prev_version + 1;
    world.insert_resource(lot);
    let ghosts: Vec<Entity> = world.query_filtered::<Entity, With<Ghost>>().iter().collect();
    for g in ghosts {
        let mut gh = world.get_mut::<Ghost>(g).unwrap();
        if let Some(&(_, new)) = map.iter().find(|(old, _)| *old == gh.grave) {
            gh.grave = new;
        }
    }
    map
}

/// Builds a lot in a scratch world (the live lot is untouched) and returns it parked.
pub fn build_parked(world: &World, build: impl FnOnce(&mut World)) -> ParkedLot {
    let mut scratch = World::new();
    scratch.insert_resource(Data(Arc::clone(&world.res::<Data>().0)));
    scratch.insert_resource(Lot::new(32, 32));
    scratch.insert_resource(Household::default());
    build(&mut scratch);
    let mut parked = park_active(&mut scratch);
    // scratch handles mean nothing in the real world
    parked.objects.iter_mut().for_each(|o| o.id = None);
    parked
}

/// Adds a lot to the street; returns its id.
pub fn add_lot(world: &World, address: String, name: Option<String>, kind: LotKind, land_price: i64, build: impl FnOnce(&mut World)) -> LotId {
    let parked = build_parked(world, build);
    let value = parked.build_value();
    let mut hood = world.res_mut::<Neighborhood>();
    hood.lots.push(LotSlot { address, name, kind, land_price, parked: Some(parked), value });
    hood.lots.len() - 1
}

fn next_address(world: &World) -> String {
    let n = world.res::<Neighborhood>().lots.len();
    format!("{} {}", 3 + 2 * n, STREETS[n % STREETS.len()])
}

// ------------------------------------------------------------------------------------------------
// setting up the street

/// When `e` will next drop by: daytime, or evenings for people with a day job.
fn next_visit(world: &World, e: Entity, earliest_days: f32, latest_days: f32) -> u64 {
    let clock = *world.res::<Clock>();
    let (from, to) = if world.has::<Job>(e) { (17 * 60, 20 * 60) } else { (9 * 60, 19 * 60) };
    let mut rng = world.res_mut::<SimRng>();
    let delay = (rng.range_f32(earliest_days, latest_days) * TICKS_PER_DAY as f32) as u64;
    let t = clock.tick + delay;
    let mod_ = t % TICKS_PER_DAY;
    let t = if !(from..to).contains(&mod_) {
        let day = t / TICKS_PER_DAY + if mod_ >= to { 1 } else { 0 };
        day * TICKS_PER_DAY + from + rng.below((to - from) as usize) as u64
    } else {
        t
    };
    drop(rng);
    // nobody misses their own wedding
    match crate::romance::own_wedding(world, e) {
        Some(w) if w.saturating_sub(40) > clock.tick => t.min(w - 40),
        _ => t,
    }
}

/// The house a household of `size` gets when it builds: a studio for one, the bungalow for
/// two, a family cottage for more.
pub fn plan_for(size: usize) -> fn(&mut World) {
    match size {
        0 | 1 => crate::starter::build_studio,
        2 => crate::starter::build_bungalow,
        _ => crate::starter::build_cottage,
    }
}

/// Spawns a household of neighbors (off-lot) and moves them into a new house down the street.
pub fn add_neighbor_household(world: &mut World, stages: &[LifeStage], couple: bool) -> LotId {
    let address = next_address(world);
    let lot = add_lot(world, address, None, LotKind::Residential, 2500, plan_for(stages.len()));
    settle_household(world, lot, stages, couple);
    lot
}

/// Spawns a household of neighbors (off-lot) living on `lot`; returns its name.
fn settle_household(world: &mut World, lot: LotId, stages: &[LifeStage], couple: bool) -> String {
    let mut members = Vec::new();
    let mut last: Option<String> = None;
    for &stage in stages {
        let spec = random_spec(world, stage, last.as_deref());
        last.get_or_insert_with(|| spec.last.clone());
        let e = spawn_peon_as(world, spec, Vec3::ZERO, false);
        members.push(e);
    }
    for &e in &members {
        let visit_at = next_visit(world, e, 0.1, 2.0);
        world.insert_one(e, Offlot { visit_at });
    }
    let adults: Vec<Entity> = members.iter().copied().filter(|e| world.get::<Age>(*e).unwrap().stage.is_adult()).collect();
    if couple && adults.len() >= 2 {
        let sex = *world.get::<Sex>(adults[0]).unwrap();
        world.insert_one(adults[1], sex.opposite());
        crate::family::marry(world, adults[0], adults[1]);
    }
    for &k in members.iter().filter(|e| !adults.contains(e)) {
        for &p in adults.iter().take(if couple { 2 } else { 1 }) {
            crate::family::set_parent(world, p, k);
        }
    }
    let funds = 3000 + world.res_mut::<SimRng>().below(12_000) as i64;
    let name = format!("The {} Household", last.unwrap_or_default());
    push_household(world, Household { name: name.clone(), funds, members, unpaid: 0, unpaid_since: None, home: Some(lot) });
    name
}

/// How many households the street likes to have (counting the played one); below this,
/// newcomers move into vacant lots.
pub const STREET_HOUSEHOLDS: usize = 5;

/// Who moves in: (stages, couple).
const NEWCOMERS: [(&[LifeStage], bool); 7] = {
    use LifeStage::*;
    [
        (&[YoungAdult], false),
        (&[YoungAdult, YoungAdult], true),
        (&[Adult, Adult, Child], true),
        (&[Adult, Adult, Toddler, Child], true),
        (&[Adult, Teen], false),
        (&[Elder, Elder], true),
        (&[Adult], false),
    ]
};

/// Households on the street, the played one included.
pub fn household_count(world: &World) -> usize {
    world.res::<Neighborhood>().households.len() + usize::from(world.res::<Household>().home.is_some())
}

/// Every morning at 10:00, if the street is short of households, a new one may move into a vacant
/// lot (a furnished house first; on an empty plot they build). One lot is always left for sale.
pub fn newcomers(world: &mut World) {
    if world.res::<Clock>().minute_of_day() != 10 * 60 || household_count(world) >= STREET_HOUSEHOLDS {
        return;
    }
    let active = active_lot(world);
    let open: Vec<LotId> = vacant_lots(world).into_iter().filter(|&l| l != active).collect();
    if open.len() < 2 || world.res_mut::<SimRng>().below(100) >= 40 {
        return;
    }
    let furnished: Vec<LotId> = open.iter().copied().filter(|&l| build_value(world, l) > 0).collect();
    let pool = if furnished.is_empty() { &open } else { &furnished };
    let lot = pool[world.res_mut::<SimRng>().below(pool.len())];
    let (stages, couple) = NEWCOMERS[world.res_mut::<SimRng>().below(NEWCOMERS.len())];
    move_in_newcomers(world, lot, stages, couple);
}

/// A new household of townies moves into vacant `lot` (not the live one), building a house first
/// if it's an empty plot.
pub fn move_in_newcomers(world: &mut World, lot: LotId, stages: &[LifeStage], couple: bool) -> String {
    assert!(lot != active_lot(world), "newcomers move into a parked lot");
    if build_value(world, lot) == 0 {
        let parked = build_parked(world, plan_for(stages.len()));
        let mut hood = world.res_mut::<Neighborhood>();
        hood.lots[lot].value = parked.build_value();
        hood.lots[lot].parked = Some(parked);
    }
    let name = settle_household(world, lot, stages, couple);
    let address = world.res::<Neighborhood>().lots[lot].address.clone();
    log(world, LogKind::Info, None, format!("A moving van pulled up: {name} moved into {address}."));
    world.flush();
    name
}

fn push_household(world: &World, h: Household) {
    let mut hood = world.res_mut::<Neighborhood>();
    hood.households.push(h);
    hood.households.sort_by_key(|h| h.home);
}

/// Fills out the street around the played household: four neighbor households, a furnished
/// house for sale, two empty plots and a park. Does nothing if the street already has lots.
pub fn populate(world: &mut World) {
    if world.res::<Neighborhood>().lots.len() > 1 {
        return;
    }
    use LifeStage::*;
    add_neighbor_household(world, &[YoungAdult], false);
    add_neighbor_household(world, &[Adult, Adult], true);
    add_neighbor_household(world, &[Elder], false);
    add_neighbor_household(world, &[Adult, Adult, Child], true);
    add_rest_of_street(world);
}

fn add_rest_of_street(world: &mut World) {
    let a = next_address(world);
    add_lot(world, a, None, LotKind::Residential, 3000, crate::starter::build_house);
    let a = next_address(world);
    add_lot(world, a, None, LotKind::Residential, 2000, |w| crate::starter::build_empty(w, true));
    let a = next_address(world);
    add_lot(world, a, None, LotKind::Residential, 2500, |w| crate::starter::build_empty(w, false));
    add_community_lots(world);
}

/// The community lots: (address, name, builder).
type CommunityLot = (&'static str, &'static str, fn(&mut World));

const COMMUNITY: [CommunityLot; 3] = [
    ("1 Town Square", "Gulch Green", crate::starter::build_park),
    ("2 Town Square", "The Grumpy Bean", crate::starter::build_cafe),
    ("3 Town Square", "Sweat Equity Gym", crate::starter::build_gym),
];

/// Adds whichever community lots the street doesn't have yet (older saves predate some).
pub fn add_community_lots(world: &mut World) {
    for (address, name, build) in COMMUNITY {
        let has = world.res::<Neighborhood>().lots.iter().any(|l| l.name.as_deref() == Some(name));
        if !has {
            add_lot(world, address.into(), Some(name.into()), LotKind::Community, 0, build);
        }
    }
}

// ------------------------------------------------------------------------------------------------
// going places

/// Clears what `e` was doing (they're about to be somewhere else).
fn reset(world: &mut World, e: Entity) {
    actions::abort_current(world, e);
    if let Some(mut q) = world.get_mut::<ActionQueue>(e) {
        q.current = None;
        q.queue.clear();
    }
    if let Some(mut p) = world.get_mut::<Posture>(e) {
        *p = Posture::default();
    }
    world.remove::<Visitor>(e);
}

/// They took care of themselves while away.
fn freshen(world: &World, e: Entity) {
    if let Some(mut n) = world.get_mut::<Needs>(e) {
        for k in Need::ALL {
            let v = n.get(k).max(70.0);
            n.set(k, v);
        }
        n.starving_minutes = 0;
    }
}

/// A tile by the street for the `i`th arrival.
fn arrival_tile(world: &World, i: usize) -> IVec2 {
    let lot = world.res::<Lot>();
    let exit = actions::exit_tile(&lot);
    let i = i as i32;
    let t = exit + IVec2::new(i % 5 - 2, -(i / 5));
    if lot.walkable(0, t) { t } else { exit }
}

/// A spot by the street for the `i`th arrival.
fn arrival_spot(world: &World, i: usize) -> Vec3 {
    let t = arrival_tile(world, i);
    world.res::<Lot>().ground_center(0, t)
}

// ------------------------------------------------------------------------------------------------
// the sidewalk

/// How far past the lot's street edge the middle of the sidewalk is.
const SIDEWALK: f32 = 1.0;
/// How far past the side of the lot people walk before they're out of sight.
const OFFSTAGE: f32 = 10.0;
/// Label of the actions that walk people between lots.
pub const HEAD_OUT: &str = "Head Out";
pub const HEAD_IN: &str = "Head In";

/// Which way down the street `to` lies, seen from `from` (lots are numbered along the street).
fn direction(from: LotId, to: LotId) -> f32 {
    if to < from { -1.0 } else { 1.0 }
}

/// Walking side by side: two lanes on the sidewalk.
fn lane(i: usize) -> f32 {
    if i.is_multiple_of(2) { -0.3 } else { 0.3 }
}

/// From the front walk onto the sidewalk and off down the street toward `dir`.
fn walk_out(world: &World, dir: f32, i: usize) -> Vec<Vec3> {
    let lot = world.res::<Lot>();
    let exit = actions::exit_tile(&lot);
    let z = lot.depth as f32 + SIDEWALK + lane(i);
    let end = if dir < 0.0 { -OFFSTAGE } else { lot.width as f32 + OFFSTAGE };
    vec![Vec3::new(exit.x as f32 + 0.5, 0.0, z), Vec3::new(end, 0.0, z)]
}

/// Coming up the sidewalk from the end of the street on `side`: where to start (a little
/// behind the one in front), and the way to the front walk.
fn walk_in(world: &World, side: f32, i: usize) -> (Vec3, Vec<Vec3>) {
    let lot = world.res::<Lot>();
    let exit = actions::exit_tile(&lot);
    let z = lot.depth as f32 + SIDEWALK + lane(i);
    let edge = if side < 0.0 { -OFFSTAGE } else { lot.width as f32 + OFFSTAGE };
    let start = Vec3::new(edge + side * 1.2 * (i / 2) as f32, 0.0, z);
    (start, vec![Vec3::new(exit.x as f32 + 0.5, 0.0, z), lot.ground_center(0, exit)])
}

fn can_walk(world: &World, e: Entity) -> bool {
    world.get::<Age>(e).is_some_and(|a| a.stage.can_walk())
}

/// Is `e` still walking to or from the street (an action labelled `label` queued or underway)?
fn walking(world: &World, e: Entity, label: &str) -> bool {
    world.get::<ActionQueue>(e).is_some_and(|q| {
        q.current.as_ref().is_some_and(|c| c.action.label == label) || q.queue.iter().any(|a| a.label == label)
    })
}

/// Sends `e` up the sidewalk from `side` onto the live lot, to the `i`th arrival tile.
fn send_in(world: &mut World, e: Entity, side: f32, i: usize, label: &str) {
    let (start, path) = walk_in(world, side, i);
    place_at(world, e, start);
    if let Some(mut tf) = world.get_mut::<Transform>(e) {
        tf.yaw = (-side).atan2(0.0);
        tf.prev_yaw = tf.yaw;
    }
    let tile = arrival_tile(world, i);
    actions::push_back(world, e, ActionKind::Walk { path }, Source::System, label.into());
    actions::push_back(world, e, ActionKind::GoTo { level: 0, tile }, Source::System, label.into());
}

/// Sends `e` out to the street and off down the sidewalk toward `dir`.
fn send_out(world: &mut World, e: Entity, dir: f32, i: usize, label: &str) {
    let exit = actions::exit_tile(&world.res::<Lot>());
    let path = walk_out(world, dir, i);
    actions::push_back(world, e, ActionKind::GoTo { level: 0, tile: exit }, Source::System, label.into());
    actions::push_back(world, e, ActionKind::Walk { path }, Source::System, label.into());
}

fn place_at(world: &World, e: Entity, pos: Vec3) {
    if let Some(mut tf) = world.get_mut::<Transform>(e) {
        tf.pos = pos;
        tf.prev_pos = pos;
    }
}

/// Members of the played household who can travel with it (alive, not at work or school).
pub fn travelers(world: &World) -> Vec<Entity> {
    let mut v: Vec<Entity> =
        world.res::<Household>().members.iter().copied().filter(|e| world.contains(*e) && !world.has::<Dead>(*e) && !world.has::<AtWork>(*e)).collect();
    v.sort();
    v
}

/// Makes `target` the live lot. Everyone on the current lot except `travelers` leaves it
/// (they go home, off-lot); the travelers arrive at the street of the new lot.
pub fn switch_lot(world: &mut World, target: LotId, travelers: &[Entity]) -> Result<(), String> {
    let active = active_lot(world);
    if target == active {
        return Err("already there".into());
    }
    if world.res::<Neighborhood>().lots.get(target).and_then(|l| l.parked.as_ref()).is_none() {
        return Err("no such lot".into());
    }
    // a walk that was underway is called off: whoever had walked off is back on the street
    let cancelled = world.res_mut::<Neighborhood>().trip.take();
    for e in cancelled.map(|t| t.travelers).unwrap_or_default() {
        if world.contains(e) && !world.has::<Dead>(e) && !world.has::<AtWork>(e) {
            world.remove::<Offlot>(e);
        }
    }
    // everyone else leaves
    for e in crate::sim::present_peons(world) {
        reset(world, e);
        if !travelers.contains(&e) {
            let visit_at = next_visit(world, e, 1.0, 3.0);
            world.insert_one(e, Offlot { visit_at });
        }
    }
    // hosts and guests at work or school go home afterwards, not here
    let mut out: Vec<Entity> = world.query_filtered::<Entity, (With<AtWork>, Without<Offlot>, Without<Member>)>().iter().collect();
    out.sort();
    for e in out {
        world.remove::<Visitor>(e);
        let visit_at = next_visit(world, e, 1.0, 3.0);
        world.insert_one(e, Offlot { visit_at });
    }
    // the ghosts stay with their graves
    let ghosts: Vec<Entity> = world.query_filtered::<Entity, (With<Ghost>, Without<Offlot>)>().iter().collect();
    for g in ghosts {
        world.get_mut::<Ghost>(g).unwrap().visible_until = 0;
        world.insert_one(g, Offlot { visit_at: u64::MAX });
    }
    let parked = park_active(world);
    let incoming = {
        let mut hood = world.res_mut::<Neighborhood>();
        hood.lots[active].value = parked.build_value();
        hood.lots[active].parked = Some(parked);
        hood.active = target;
        hood.lots[target].parked.take().unwrap()
    };
    let map = unpark(world, incoming);
    // ghosts whose grave is here come back
    let ghosts: Vec<Entity> = world.query_filtered::<Entity, (With<Ghost>, With<Offlot>)>().iter().collect();
    for g in ghosts {
        let grave = world.get::<Ghost>(g).unwrap().grave;
        if map.iter().any(|(_, new)| *new == grave) {
            world.remove::<Offlot>(g);
            let pos = world.get::<ObjectInstance>(grave).map(|o| o.center(data(world).object(o.def)));
            if let Some(p) = pos {
                place_at(world, g, p);
            }
        }
    }
    for (i, &e) in travelers.iter().enumerate() {
        if world.remove::<Offlot>(e).is_some() {
            freshen(world, e);
        }
        reset(world, e);
        let pos = arrival_spot(world, i);
        place_at(world, e, pos);
    }
    world.flush();
    Ok(())
}

/// Plays another household: the current one is parked at home and the other takes over.
pub fn play_household(world: &mut World, home: LotId) -> Result<(), String> {
    let idx = world.res::<Neighborhood>().households.iter().position(|h| h.home == Some(home)).ok_or("nobody lives there")?;
    let new = world.res_mut::<Neighborhood>().households.remove(idx);
    let old = std::mem::replace(&mut *world.res_mut::<Household>(), new.clone());
    for &m in &old.members {
        world.remove::<Member>(m);
        if world.has::<AtWork>(m) {
            let visit_at = next_visit(world, m, 1.0, 3.0);
            world.insert_one(m, Offlot { visit_at });
        }
    }
    push_household(world, old);
    for &m in &new.members {
        if world.contains(m) && !world.has::<Dead>(m) {
            world.insert_one(m, Member);
            if world.has::<AtWork>(m) {
                world.remove::<Offlot>(m);
            }
        }
    }
    let travelers = travelers(world);
    if home != active_lot(world) {
        switch_lot(world, home, &travelers)?;
    }
    crate::romance::took_over(world, &new.members);
    log(world, LogKind::Info, None, format!("Now playing {} at {}.", new.name, world.res::<Neighborhood>().lots[home].address));
    Ok(())
}

/// Sends the household out to a community lot (on foot: see [`trips`]).
pub fn go_out(world: &mut World, lot: LotId) -> Result<(), String> {
    if world.res::<Neighborhood>().lots.get(lot).map(|l| l.kind) != Some(LotKind::Community) {
        return Err("that's not somewhere to go out to".into());
    }
    start_trip(world, TripKind::GoOut, lot)?;
    let (hh, place) = (world.res::<Household>().name.clone(), world.res::<Neighborhood>().lots[lot].title());
    log(world, LogKind::Info, None, format!("{hh} headed out to {place}."));
    Ok(())
}

/// A few neighbors are out and about too (they turn up over the next couple of hours).
fn locals_turn_up(world: &mut World) {
    let tick = now(world);
    let mut locals: Vec<Entity> = world
        .query_filtered::<(Entity, &Age), (With<Offlot>, Without<Dead>, Without<AtWork>, Without<Member>)>()
        .iter()
        .filter(|(_, a)| a.stage >= LifeStage::Teen)
        .map(|(e, _)| e)
        .collect();
    locals.sort();
    let n = locals.len().min(2 + world.res_mut::<SimRng>().below(3));
    for _ in 0..n {
        let i = world.res_mut::<SimRng>().below(locals.len());
        let e = locals.swap_remove(i);
        let at = tick + 10 + world.res_mut::<SimRng>().below(150) as u64;
        world.get_mut::<Offlot>(e).unwrap().visit_at = at;
    }
}

/// Sends the household over to a neighbor's (on foot: see [`trips`]). When they get there the
/// hosts are at home (unless they're at work or school), and go about their evening.
pub fn visit(world: &mut World, lot: LotId) -> Result<(), String> {
    let r = residents(world, lot).ok_or("nobody lives there")?;
    if r.played {
        return Err("that's your own place".into());
    }
    if !(8..22).contains(&world.res::<Clock>().hour()) {
        return Err(format!("it's too late to drop in on {}", r.name));
    }
    start_trip(world, TripKind::Visit, lot)?;
    let hh = world.res::<Household>().name.clone();
    log(world, LogKind::Social, None, format!("{hh} set off for {}'s place.", r.name));
    Ok(())
}

/// The household has walked over to a neighbor's: the hosts are at home (unless they're at work or
/// school) and indoors.
fn hosts_at_home(world: &mut World, lot: LotId) {
    let Some(r) = residents(world, lot) else {
        let (hh, addr) = (world.res::<Household>().name.clone(), world.res::<Neighborhood>().lots[lot].address.clone());
        log(world, LogKind::Social, None, format!("{hh} went over to {addr}, but nobody lives there anymore."));
        return;
    };
    let mut hosts: Vec<Entity> =
        r.members.iter().copied().filter(|&e| world.contains(e) && !world.has::<Dead>(e) && !world.has::<AtWork>(e) && world.has::<Offlot>(e)).collect();
    hosts.sort();
    for (i, &e) in hosts.iter().enumerate() {
        world.remove::<Offlot>(e);
        reset(world, e);
        freshen(world, e);
        let pos = indoor_spot(world, i);
        place_at(world, e, pos);
    }
    world.flush();
    let (hh, addr) = (world.res::<Household>().name.clone(), world.res::<Neighborhood>().lots[lot].address.clone());
    let line = if hosts.is_empty() { format!("{hh} went over to {addr}, but {} aren't home.", r.name) } else { format!("{hh} dropped in on {} at {addr}.", r.name) };
    log(world, LogKind::Social, None, line);
}

/// Is the household a guest at someone else's home?
pub fn visiting(world: &World) -> bool {
    let active = active_lot(world);
    active_kind(world) == LotKind::Residential && world.res::<Household>().home != Some(active) && residents(world, active).is_some()
}

/// A walkable indoor (floored) ground-floor spot for the `i`th resident at home.
fn indoor_spot(world: &World, i: usize) -> Vec3 {
    let tiles: Vec<IVec2> = {
        let lot = world.res::<Lot>();
        lot.floors.keys().filter(|(l, _, _)| *l == 0).map(|&(_, x, z)| IVec2::new(x, z)).filter(|t| lot.walkable(0, *t)).collect()
    };
    if tiles.is_empty() {
        return arrival_spot(world, i);
    }
    let t = tiles[(tiles.len() / 3 + i * 7) % tiles.len()];
    world.res::<Lot>().ground_center(0, t)
}

/// Sends the household home from an outing (on foot: see [`trips`]).
pub fn go_home(world: &mut World) -> Result<(), String> {
    let home = world.res::<Household>().home.ok_or("they have no home")?;
    if home == active_lot(world) {
        return Err("they're already home".into());
    }
    start_trip(world, TripKind::Home, home)?;
    log(world, LogKind::Info, None, format!("{} headed home.", world.res::<Household>().name));
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// trips

/// The household's walk to another lot, if one is underway.
pub fn trip(world: &World) -> Option<Trip> {
    world.res::<Neighborhood>().trip.clone()
}

/// Sets the household walking to `to`: out to the street and off down the sidewalk. The lot
/// switches once they've all left (see [`trips`]).
fn start_trip(world: &mut World, kind: TripKind, to: LotId) -> Result<(), String> {
    if world.res::<Neighborhood>().trip.is_some() {
        return Err("they're already on their way somewhere".into());
    }
    if to == active_lot(world) {
        return Err("they're already there".into());
    }
    let travelers = travelers(world);
    if travelers.is_empty() {
        return Err("nobody's home to go".into());
    }
    let dir = direction(active_lot(world), to);
    for (i, &e) in travelers.iter().enumerate() {
        reset(world, e);
        if can_walk(world, e) {
            send_out(world, e, dir, i, HEAD_OUT);
        } else {
            // carried
            world.insert_one(e, Offlot { visit_at: u64::MAX });
        }
    }
    let until = now(world) + 120;
    world.res_mut::<Neighborhood>().trip = Some(Trip { kind, to, travelers, arriving: false, until });
    Ok(())
}

/// Still part of the household's trip (not dead, gone to work, or moved out meanwhile)?
fn on_trip(world: &World, e: Entity) -> bool {
    world.contains(e) && !world.has::<Dead>(e) && !world.has::<AtWork>(e) && world.has::<Member>(e)
}

/// Walks the household between lots: whoever reaches the end of the street leaves the lot; when
/// everyone has (or two hours are up), the destination goes live and they walk onto it.
pub fn trips(world: &mut World) {
    let Some(trip) = trip(world) else { return };
    let tick = now(world);
    let label = if trip.arriving { HEAD_IN } else { HEAD_OUT };
    let mut underway = false;
    for &e in &trip.travelers {
        if !on_trip(world, e) || world.has::<Offlot>(e) {
            continue;
        }
        if walking(world, e, label) {
            underway = true;
        } else if !trip.arriving {
            world.insert_one(e, Offlot { visit_at: u64::MAX });
        }
    }
    if underway && tick < trip.until {
        return;
    }
    if trip.arriving {
        world.res_mut::<Neighborhood>().trip = None;
        // the babies are put down by the front door
        let carried: Vec<Entity> = trip.travelers.iter().copied().filter(|&e| on_trip(world, e) && world.has::<Offlot>(e)).collect();
        for (i, e) in carried.into_iter().enumerate() {
            world.remove::<Offlot>(e);
            let pos = arrival_spot(world, i);
            place_at(world, e, pos);
        }
    } else {
        reach(world, trip);
    }
    world.flush();
}

/// The household reaches the end of the street: the destination goes live and they walk onto it.
fn reach(world: &mut World, trip: Trip) {
    let from = active_lot(world);
    let travelers: Vec<Entity> = trip.travelers.iter().copied().filter(|&e| on_trip(world, e)).collect();
    world.res_mut::<Neighborhood>().trip = None;
    for &e in &travelers {
        world.remove::<Offlot>(e);
    }
    if let Err(why) = switch_lot(world, trip.to, &travelers) {
        log::warn!("trip to lot {} called off: {why}", trip.to);
        return;
    }
    let side = direction(trip.to, from);
    for (i, &e) in travelers.iter().enumerate() {
        if can_walk(world, e) {
            send_in(world, e, side, i, HEAD_IN);
        } else {
            world.insert_one(e, Offlot { visit_at: u64::MAX });
        }
    }
    match trip.kind {
        TripKind::GoOut => locals_turn_up(world),
        TripKind::Visit => hosts_at_home(world, trip.to),
        TripKind::Home => log(world, LogKind::Info, None, format!("{} got home.", world.res::<Household>().name)),
    }
    let until = now(world) + 90;
    world.res_mut::<Neighborhood>().trip = Some(Trip { arriving: true, until, travelers, ..trip });
}

/// Sells the current home and buys a vacant lot (with whatever is built on it).
pub fn move_household(world: &mut World, lot: LotId) -> Result<(), String> {
    if !vacant_lots(world).contains(&lot) {
        return Err("that lot isn't for sale".into());
    }
    let cost = price(world, lot);
    let sale = world.res::<Household>().home.map(|h| price(world, h)).unwrap_or(0);
    let funds = world.res::<Household>().funds;
    if funds + sale < cost {
        return Err(format!("can't afford it (${cost}, with ${} from selling the old place)", sale));
    }
    {
        let mut hh = world.res_mut::<Household>();
        hh.funds += sale - cost;
        hh.home = Some(lot);
    }
    let travelers = travelers(world);
    if lot != active_lot(world) {
        switch_lot(world, lot, &travelers)?;
    }
    let (hh, addr) = (world.res::<Household>().name.clone(), world.res::<Neighborhood>().lots[lot].address.clone());
    log(world, LogKind::Money, None, format!("{hh} moved to {addr}: sold the old place for ${sale}, paid ${cost}."));
    Ok(())
}

/// Parks the played household and starts a new, empty one on a vacant lot. Spawn its members
/// with [`crate::sim::spawn_peon`] next.
pub fn found_household(world: &mut World, lot: LotId, name: String, funds: i64) -> Result<(), String> {
    if !vacant_lots(world).contains(&lot) {
        return Err("that lot isn't vacant".into());
    }
    let new = Household { name, funds, members: Vec::new(), unpaid: 0, unpaid_since: None, home: Some(lot) };
    let old = std::mem::replace(&mut *world.res_mut::<Household>(), new);
    for &m in &old.members {
        world.remove::<Member>(m);
        if world.has::<AtWork>(m) {
            let visit_at = next_visit(world, m, 1.0, 3.0);
            world.insert_one(m, Offlot { visit_at });
        }
    }
    if old.home.is_some() {
        push_household(world, old);
    }
    if lot != active_lot(world) {
        switch_lot(world, lot, &[])?;
    }
    Ok(())
}

/// Where the `i`th member of a household moving in should appear.
pub fn move_in_spot(world: &World, i: usize) -> Vec3 {
    arrival_spot(world, i)
}

// ------------------------------------------------------------------------------------------------
// visits

/// Ask a neighbor to come over now.
pub fn invite(world: &mut World, e: Entity) {
    let tick = now(world);
    if let Some(mut o) = world.get_mut::<Offlot>(e) {
        o.visit_at = tick;
    }
}

/// Arrivals and departures of neighbors.
pub fn visits(world: &mut World) {
    let tick = now(world);
    let hour = world.res::<Clock>().hour();
    let community = active_kind(world) == LotKind::Community;
    let mut due: Vec<(Entity, LifeStage)> = world
        .query_filtered::<(Entity, &Offlot, &Age), (Without<Dead>, Without<AtWork>, Without<Member>)>()
        .iter()
        .filter(|(_, o, _)| o.visit_at <= tick)
        .map(|(e, _, a)| (e, a.stage))
        .collect();
    due.sort_by_key(|(e, _)| *e);
    for (e, stage) in due {
        if stage < LifeStage::Teen {
            // kids don't wander the neighborhood alone
            world.get_mut::<Offlot>(e).unwrap().visit_at = u64::MAX;
        } else if (8..21).contains(&hour) {
            arrive(world, e, tick, community);
        }
    }
    if world.res::<Clock>().minute_of_day() == 0 {
        closing_time(world);
    }
    if world.res::<Clock>().minute_of_day() == 23 * 60 && visiting(world) {
        let hosts = residents(world, active_lot(world)).map(|r| r.members).unwrap_or_default();
        if let Some(&h) = hosts.iter().find(|&&h| world.contains(h) && !world.has::<Dead>(h) && !crate::components::is_away(world, h)) {
            set_bubble(world, h, Some("Zzz".into()), None, 30);
            log(world, LogKind::Funny, Some(h), format!("{} yawned pointedly and started turning off lights. It might be time to go home.", name_of(world, h)));
        }
    }
    let mut guests: Vec<(Entity, Visitor)> = world.query_filtered::<(Entity, &Visitor), Without<Dead>>().iter().map(|(e, v)| (e, v.clone())).collect();
    guests.sort_by_key(|(e, _)| *e);
    for (e, v) in guests {
        let needs_home = world.get::<Needs>(e).is_some_and(|n| !n.critical_physical(15.0).is_empty());
        if !v.leaving && (tick >= v.leave_at || needs_home || !(8..22).contains(&hour)) {
            start_leaving(world, e, needs_home && !community);
        } else if v.leaving {
            // gone once they're off down the street
            if !walking(world, e, "Go Home") || tick > v.leave_at + 120 {
                leave(world, e);
            }
        }
    }
}

/// Midnight: nobody's staying over. Hosts show the household the door, and community lots close.
fn closing_time(world: &mut World) {
    let Some(home) = world.res::<Household>().home else { return };
    if home == active_lot(world) || world.res::<Neighborhood>().trip.is_some() || travelers(world).is_empty() {
        return;
    }
    let hh = world.res::<Household>().name.clone();
    let line = match residents(world, active_lot(world)) {
        Some(r) => {
            let host = r.members.iter().copied().find(|&h| world.contains(h) && !world.has::<Dead>(h) && !is_away(world, h));
            match host {
                Some(h) => format!("{} handed {hh} their coats. It's past midnight, and the guest room is 'being painted'.", name_of(world, h)),
                None => format!("It's past midnight; {hh} let themselves out of {}'s place.", r.name),
            }
        }
        None => format!("{} is closing for the night; {hh} headed home.", world.res::<Neighborhood>().lots[active_lot(world)].title()),
    };
    if start_trip(world, TripKind::Home, home).is_ok() {
        log(world, LogKind::Funny, None, line);
    }
}

fn arrive(world: &mut World, e: Entity, tick: u64, community: bool) {
    world.remove::<Offlot>(e);
    reset(world, e);
    let mut stay = 120 + world.res_mut::<SimRng>().below(180) as u64;
    // a bride or groom who came over for their wedding stays for it
    if let Some(w) = crate::romance::wedding_with_household(world, e).filter(|&w| w > tick && w < tick + 6 * 60) {
        stay = stay.max(w + 120 - tick);
    }
    world.insert_one(e, Visitor { leave_at: tick + stay, leaving: false });
    // up the sidewalk from their end of the street
    let side = home_of(world, e).map(|h| direction(active_lot(world), h)).unwrap_or(1.0);
    let i = world.res_mut::<SimRng>().below(5);
    send_in(world, e, side, i, "Come Over");
    freshen(world, e);
    let address = home_of(world, e).map(|l| world.res::<Neighborhood>().lots[l].address.clone()).unwrap_or_else(|| "down the street".into());
    let name = name_of(world, e);
    if community {
        let place = world.res::<Neighborhood>().lots[active_lot(world)].title();
        log(world, LogKind::Social, Some(e), format!("{name} from {address} is at {place} too."));
        return;
    }
    set_bubble(world, e, Some("hi".into()), Some("Yoo-hoo!".into()), 20);
    const LINES: [&str; 5] = [
        "{n} from {a} dropped by to say hi.",
        "{n} from {a} rang the doorbell. Twice. Then knocked.",
        "{n} from {a} came over to borrow a cup of sugar and stayed for the drama.",
        "{n} from {a} is here, holding a casserole nobody asked for.",
        "{n} from {a} wandered over 'just for a minute'.",
    ];
    let i = world.res_mut::<SimRng>().below(LINES.len());
    let line = LINES[i].replace("{n}", &name).replace("{a}", &address);
    log(world, LogKind::Social, Some(e), line);
}

fn start_leaving(world: &mut World, e: Entity, needs_home: bool) {
    actions::abort_current(world, e);
    if let Some(mut q) = world.get_mut::<ActionQueue>(e) {
        q.queue.clear();
    }
    let dir = home_of(world, e).map(|h| direction(active_lot(world), h)).unwrap_or(1.0);
    let i = world.res_mut::<SimRng>().below(2);
    send_out(world, e, dir, i, "Go Home");
    world.get_mut::<Visitor>(e).unwrap().leaving = true;
    if needs_home {
        log(world, LogKind::Funny, Some(e), format!("{} suddenly remembered they left the oven on. (They need their own bathroom.)", name_of(world, e)));
    }
}

fn leave(world: &mut World, e: Entity) {
    reset(world, e);
    let visit_at = next_visit(world, e, 1.0, 3.0);
    world.insert_one(e, Offlot { visit_at });
    log(world, LogKind::Info, Some(e), format!("{} went home.", name_of(world, e)));
}

// ------------------------------------------------------------------------------------------------
// life off the lot

/// Once a day at 18:00, life goes on for households that aren't being played: jobs pay and
/// promote, grades drift, bills come due, and babies are born.
pub fn offlot_daily(world: &mut World) {
    let clock = *world.res::<Clock>();
    if clock.minute_of_day() != 18 * 60 {
        return;
    }
    let data = data(world);
    let n = world.res::<Neighborhood>().households.len();
    for i in 0..n {
        let (members, home) = {
            let hood = world.res::<Neighborhood>();
            (hood.households[i].members.clone(), hood.households[i].home)
        };
        let mut earned = 0;
        for e in members {
            if !world.contains(e) || world.has::<Dead>(e) || !world.has::<Offlot>(e) {
                continue;
            }
            if let Some(job) = world.get::<Job>(e).map(|j| j.clone())
                && let Some(c) = data.career(&job.career)
                && c.levels[job.level].days.contains(&clock.weekday())
            {
                let lvl = &c.levels[job.level];
                earned += lvl.pay * lvl.hours as i64;
                let gain = world.res_mut::<SimRng>().range_f32(4.0, 12.0);
                // they practise what the next rung asks for, a little each workday
                if let Some(next) = c.levels.get(job.level + 1) {
                    let mut sk = world.get_mut::<Skills>(e).unwrap();
                    for (&s, &need) in &next.skills {
                        if sk.get(s) < need {
                            sk.add(s, 0.25);
                        }
                    }
                }
                let skills = world.get::<Skills>(e).unwrap().clone();
                let mut promoted = None;
                {
                    let mut j = world.get_mut::<Job>(e).unwrap();
                    j.days_worked += 1;
                    j.performance = (j.performance + gain).min(150.0);
                    if j.performance >= 100.0 && j.level + 1 < c.levels.len() {
                        let next = &c.levels[j.level + 1];
                        if next.skills.iter().all(|(s, v)| skills.get(*s) >= *v) {
                            j.level += 1;
                            j.performance = 25.0;
                            promoted = Some(next.title.clone());
                        }
                    }
                }
                if let Some(t) = promoted {
                    log(world, LogKind::Career, Some(e), format!("Word on the street: {} was promoted to {t}.", name_of(world, e)));
                }
            }
            if crate::school::SCHOOL_DAYS.contains(&clock.weekday()) && world.has::<Student>(e) {
                let d = world.res_mut::<SimRng>().range_f32(-3.0, 5.0);
                let mut s = world.get_mut::<Student>(e).unwrap();
                s.grade = (s.grade + d).clamp(-100.0, 100.0);
            }
        }
        let bills = if clock.day() % 3 == 2 { 40 + home.map(|h| build_value(world, h)).unwrap_or(0) / 100 } else { 0 };
        let mut hood = world.res_mut::<Neighborhood>();
        let h = &mut hood.households[i];
        h.funds = (h.funds + earned - bills).max(0);
    }
    // births off the lot
    let tick = clock.tick;
    let mut due: Vec<(Entity, Entity)> = world
        .query_filtered::<(Entity, &Pregnant), (With<Offlot>, Without<Dead>, Without<Member>)>()
        .iter()
        .filter(|(_, p)| tick >= p.due)
        .map(|(e, p)| (e, p.other))
        .collect();
    due.sort();
    for (carrier, other) in due {
        world.remove::<Pregnant>(carrier);
        let baby = crate::family::spawn_child_as(world, carrier, Some(other), false);
        world.insert_one(baby, Offlot { visit_at: u64::MAX });
        let mut hood = world.res_mut::<Neighborhood>();
        if let Some(h) = hood.households.iter_mut().find(|h| h.members.contains(&carrier)) {
            h.members.push(baby);
        }
        drop(hood);
        log(world, LogKind::Life, Some(baby), format!("{} had a baby down the street: {}.", name_of(world, carrier), name_of(world, baby)));
    }
    world.flush();
}

/// Forgets a dead or departed person in every household. A neighbor household with nobody
/// left moves out of the street, and its house goes up for sale.
pub fn forget_member(world: &World, e: Entity) {
    world.res_mut::<Household>().members.retain(|m| *m != e);
    let mut hood = world.res_mut::<Neighborhood>();
    for h in hood.households.iter_mut() {
        h.members.retain(|m| *m != e);
    }
    let gone: Vec<Household> = hood.households.iter().filter(|h| h.members.is_empty()).cloned().collect();
    hood.households.retain(|h| !h.members.is_empty());
    let lots = hood.lots.clone();
    drop(hood);
    for h in gone {
        let addr = h.home.map(|l| lots[l].address.clone()).unwrap_or_default();
        log(world, LogKind::Info, None, format!("{} is gone; the house at {addr} is up for sale.", h.name));
    }
}

// ------------------------------------------------------------------------------------------------
// moving between households

/// Which household someone lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hh {
    Played,
    /// Index into [`Neighborhood::households`] (only valid until households are added or removed).
    Neighbor(usize),
}

pub fn household_of(world: &World, e: Entity) -> Option<Hh> {
    if world.res::<Household>().members.contains(&e) {
        return Some(Hh::Played);
    }
    world.res::<Neighborhood>().households.iter().position(|h| h.members.contains(&e)).map(Hh::Neighbor)
}

/// The living members of `e`'s household, `e` included.
pub fn housemates(world: &World, e: Entity) -> Vec<Entity> {
    let members = match household_of(world, e) {
        Some(Hh::Played) => world.res::<Household>().members.clone(),
        Some(Hh::Neighbor(i)) => world.res::<Neighborhood>().households[i].members.clone(),
        None => return Vec::new(),
    };
    members.into_iter().filter(|&m| world.contains(m) && !world.has::<Dead>(m)).collect()
}

/// Takes `e` out of their household with their share of its funds. A neighbor household left
/// with nobody moves off the street and its house goes up for sale.
fn take_out(world: &World, e: Entity) -> i64 {
    let n = housemates(world, e).len().max(1) as i64;
    match household_of(world, e) {
        Some(Hh::Played) => {
            let mut hh = world.res_mut::<Household>();
            let share = hh.funds / n;
            hh.funds -= share;
            hh.members.retain(|m| *m != e);
            share
        }
        Some(Hh::Neighbor(i)) => {
            let mut hood = world.res_mut::<Neighborhood>();
            let share = hood.households[i].funds / n;
            hood.households[i].funds -= share;
            hood.households[i].members.retain(|m| *m != e);
            if n <= 1 {
                let h = hood.households.remove(i);
                let addr = h.home.map(|l| hood.lots[l].address.clone()).unwrap_or_default();
                drop(hood);
                log(world, LogKind::Info, None, format!("{} is no more; the house at {addr} is up for sale.", h.name));
            }
            share
        }
        None => 0,
    }
}

/// Puts `e` where their (new) household lives: joining the played household brings them onto
/// the lot; leaving it sends them off down the street unless they're already home.
fn settle_in(world: &mut World, e: Entity) {
    let tick = now(world);
    match household_of(world, e) {
        Some(Hh::Played) => {
            world.insert_one(e, Member);
            if world.remove::<Visitor>(e).is_some() {
                reset(world, e);
            }
            if world.has::<Offlot>(e) && !world.has::<AtWork>(e) {
                world.remove::<Offlot>(e);
                reset(world, e);
                freshen(world, e);
                let pos = arrival_spot(world, 0);
                place_at(world, e, pos);
            }
        }
        Some(Hh::Neighbor(_)) => {
            world.remove::<Member>(e);
            if world.has::<Offlot>(e) {
                return;
            }
            if world.has::<AtWork>(e) {
                let visit_at = next_visit(world, e, 1.0, 3.0);
                world.insert_one(e, Offlot { visit_at });
            } else if home_of(world, e) == Some(active_lot(world)) {
                world.remove::<Visitor>(e);
            } else if let Some(mut v) = world.get_mut::<Visitor>(e) {
                v.leave_at = v.leave_at.min(tick + 30);
            } else {
                world.insert_one(e, Visitor { leave_at: tick + 30, leaving: false });
            }
        }
        None => {}
    }
    world.flush();
}

/// `mover` joins `host`'s household (bringing their share of their old household's funds).
pub fn move_in_with(world: &mut World, mover: Entity, host: Entity) -> Result<(), String> {
    let from = household_of(world, mover).ok_or("they don't live anywhere")?;
    let to = household_of(world, host).ok_or("they don't live anywhere")?;
    let (nm, nh) = (name_of(world, mover), name_of(world, host));
    if from == to {
        return Err(format!("{nm} and {nh} already live together"));
    }
    if housemates(world, host).len() >= crate::family::MAX_HOUSEHOLD {
        return Err(format!("{nh}'s house is full"));
    }
    if from == Hh::Played && housemates(world, mover).len() <= 1 {
        return Err(format!("{nm} is the last one home"));
    }
    let share = take_out(world, mover);
    let name = match household_of(world, host) {
        Some(Hh::Played) => {
            let mut hh = world.res_mut::<Household>();
            hh.members.push(mover);
            hh.funds += share;
            hh.name.clone()
        }
        Some(Hh::Neighbor(i)) => {
            let mut hood = world.res_mut::<Neighborhood>();
            hood.households[i].members.push(mover);
            hood.households[i].funds += share;
            hood.households[i].name.clone()
        }
        None => unreachable!("the host's household can't vanish"),
    };
    settle_in(world, mover);
    log(world, LogKind::Life, Some(mover), format!("{nm} moved in with {nh} ({name}), bringing ${share} and too many mugs."));
    Ok(())
}

/// `e` leaves their household for a vacant lot (furnished first; on an empty plot they build a
/// studio), with their share of the funds.
pub fn move_out(world: &mut World, e: Entity) -> Result<LotId, String> {
    if housemates(world, e).len() <= 1 {
        return Err("they already live alone".into());
    }
    let active = active_lot(world);
    let open: Vec<LotId> = vacant_lots(world).into_iter().filter(|&l| l != active).collect();
    let furnished: Vec<LotId> = open.iter().copied().filter(|&l| build_value(world, l) > 0).collect();
    let lot = *furnished.first().or(open.first()).ok_or("there's nowhere on the street to move to")?;
    if build_value(world, lot) == 0 {
        let parked = build_parked(world, plan_for(1));
        let mut hood = world.res_mut::<Neighborhood>();
        hood.lots[lot].value = parked.build_value();
        hood.lots[lot].parked = Some(parked);
    }
    let share = take_out(world, e);
    let (first, last) = world.get::<Peon>(e).map(|p| (p.first.clone(), p.last.clone())).unwrap_or_default();
    let mut name = format!("The {last} Household");
    let taken = world.res::<Household>().name == name || world.res::<Neighborhood>().households.iter().any(|h| h.name == name);
    if taken {
        name = format!("{first} {last}'s Place");
    }
    push_household(world, Household { name, funds: share, members: vec![e], unpaid: 0, unpaid_since: None, home: Some(lot) });
    settle_in(world, e);
    let addr = world.res::<Neighborhood>().lots[lot].address.clone();
    log(world, LogKind::Life, Some(e), format!("{first} packed three boxes and a lamp and moved out to {addr}."));
    Ok(lot)
}

// ------------------------------------------------------------------------------------------------
// old saves

/// A neighbor from before lots were playable: a peon with an address and no house.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OldTownie {
    pub address: String,
    pub leave_at: u64,
    pub leaving: bool,
}

/// The neighborhood as old saves stored it (only names and addresses).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OldNeighborhood {
    pub name: String,
}

/// An old-style townie: entity, what the save said, and whether they were away (vs. visiting).
pub type OldTownieEntry = (Entity, OldTownie, bool);

/// Gives old-style townies real homes: each address becomes a household with a bungalow, and the
/// rest of the street is filled in.
pub fn migrate(world: &mut World, name: Option<String>, townies: Vec<OldTownieEntry>) {
    let mut hood = Neighborhood::default();
    if let Some(n) = name.filter(|n| !n.is_empty()) {
        hood.name = n;
    }
    world.insert_resource(hood);
    let mut by_address: Vec<(String, Vec<OldTownieEntry>)> = Vec::new();
    for t in townies {
        match by_address.iter_mut().find(|(a, _)| *a == t.1.address) {
            Some((_, v)) => v.push(t),
            None => by_address.push((t.1.address.clone(), vec![t])),
        }
    }
    for (address, people) in by_address {
        let members: Vec<Entity> = people.iter().map(|(e, _, _)| *e).collect();
        for (e, t, away) in people {
            if away {
                let visit_at = world.remove::<AtWork>(e).map(|a| a.until).unwrap_or(u64::MAX);
                world.insert_one(e, Offlot { visit_at });
            } else {
                world.insert_one(e, Visitor { leave_at: t.leave_at, leaving: t.leaving });
            }
        }
        let last = world.get::<Peon>(members[0]).map(|p| p.last.clone()).unwrap_or_default();
        let lot = add_lot(world, address, None, LotKind::Residential, 2500, crate::starter::build_bungalow);
        push_household(world, Household { name: format!("The {last} Household"), funds: 8000, members, unpaid: 0, unpaid_since: None, home: Some(lot) });
    }
    add_rest_of_street(world);
}
