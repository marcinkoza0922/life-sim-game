//! Family growth: trying for a baby, pregnancy, birth (with genetics) and adoption.
//! Either partner may end up carrying; the sim doesn't model sex, only partnership.

use glam::Vec3;
use hearth_ecs::{Entity, Without, World};

use crate::actions;
use crate::components::*;
use crate::data::{LifeStage, Need};
use crate::economy::Household;
use crate::genetics;
use crate::log::LogKind;
use crate::lot::Lot;
use crate::rng::SimRng;
use crate::sim::{add_moodlet, data, log, name_of, now, random_spec, set_bubble, spawn_peon};
use crate::social::Relationships;
use crate::time::{Settings, TICKS_PER_DAY};

/// Household size limit (births and adoptions are refused beyond it).
pub const MAX_HOUSEHOLD: usize = 8;
/// Pregnancy length in days at lifespan scale 1.0.
pub const PREGNANCY_DAYS: f32 = 2.0;
pub const ADOPTION_FEE: i64 = 500;

pub fn set_parent(world: &mut World, parent: Entity, child: Entity) {
    if let Some(mut f) = world.get_mut::<Family>(child)
        && !f.parents.contains(&parent)
    {
        f.parents.push(parent);
    }
    if let Some(mut f) = world.get_mut::<Family>(parent)
        && !f.children.contains(&child)
    {
        f.children.push(child);
    }
    world.res_mut::<Relationships>().add(parent, child, 50.0, 0.0);
}

pub fn marry(world: &mut World, a: Entity, b: Entity) {
    for (x, y) in [(a, b), (b, a)] {
        if let Some(mut f) = world.get_mut::<Family>(x) {
            f.spouse = Some(y);
        }
    }
    let mut rels = world.res_mut::<Relationships>();
    let r = rels.get_mut(a, b);
    r.friendship = r.friendship.max(60.0);
    r.romance = r.romance.max(60.0);
    r.partners = true;
    r.married = true;
}

fn household_size(world: &World) -> usize {
    world.res::<Household>().members.len()
}

fn can_carry(world: &World, e: Entity) -> bool {
    !world.has::<Pregnant>(e)
        && !world.has::<Dead>(e)
        && world.get::<Age>(e).is_some_and(|a| matches!(a.stage, LifeStage::YoungAdult | LifeStage::Adult))
}

/// Outcome of a completed "Try for Baby".
pub fn try_for_baby(world: &mut World, a: Entity, b: Entity) {
    let (na, nb) = (name_of(world, a), name_of(world, b));
    if world.has::<Pregnant>(a) || world.has::<Pregnant>(b) {
        log(world, LogKind::Funny, Some(a), format!("{na} and {nb} tried for a baby. There is already one on the way. Ambitious."));
        return;
    }
    if household_size(world) >= MAX_HOUSEHOLD {
        log(world, LogKind::Info, Some(a), format!("{na} and {nb} would love another baby, but the house is full. The house said so."));
        return;
    }
    let carriers: Vec<Entity> = [a, b].into_iter().filter(|e| can_carry(world, *e)).collect();
    let lucky = world.res_mut::<SimRng>().chance(0.6);
    if carriers.is_empty() || !lucky {
        log(world, LogKind::Social, Some(a), format!("{na} and {nb} tried for a baby. No luck this time. They seem fine with practicing."));
        return;
    }
    let carrier = *world.res_mut::<SimRng>().pick(&carriers).unwrap();
    let other = if carrier == a { b } else { a };
    let scale = world.res::<Settings>().lifespan_scale;
    let tick = now(world);
    let due = tick + (PREGNANCY_DAYS * scale * TICKS_PER_DAY as f32) as u64;
    world.insert_one(carrier, Pregnant { other, since: tick, due });
    add_moodlet(world, carrier, "expecting");
    add_moodlet(world, other, "expecting");
    let (nc, no) = (name_of(world, carrier), name_of(world, other));
    log(world, LogKind::Life, Some(carrier), format!("{nc} is expecting a baby with {no}! A congratulatory casserole has appeared from nowhere."));
}

/// Pregnancy symptoms and births.
pub fn pregnancies(world: &mut World) {
    let tick = now(world);
    let list: Vec<(Entity, Pregnant)> = world
        .query_filtered::<(Entity, &Pregnant), Without<Dead>>()
        .iter()
        .map(|(e, p)| (e, p.clone()))
        .collect();
    let mut list = list;
    list.sort_by_key(|(e, _)| *e);
    for (e, p) in list {
        // eating for two, sleeping for three
        if let Some(mut n) = world.get_mut::<Needs>(e) {
            n.add(Need::Hunger, -0.02);
            n.add(Need::Energy, -0.015);
        }
        if tick.is_multiple_of(6 * 60) && tick > p.since {
            const CRAVINGS: [&str; 4] = ["pickle", "ice cream", "pickle+ice cream", "crayons?"];
            let i = world.res_mut::<SimRng>().below(CRAVINGS.len());
            set_bubble(world, e, Some(CRAVINGS[i].into()), None, 20);
        }
        // babies wait until their carrier is home
        if tick >= p.due && !is_away(world, e) {
            give_birth(world, e, p.other);
        }
    }
    // the dead stop being pregnant
    let dead: Vec<Entity> = world.query_filtered::<Entity, hearth_ecs::With<Dead>>().iter().filter(|e| world.has::<Pregnant>(*e)).collect();
    for e in dead {
        world.remove::<Pregnant>(e);
    }
}

fn give_birth(world: &mut World, carrier: Entity, other: Entity) {
    world.remove::<Pregnant>(carrier);
    let twins = world.res_mut::<SimRng>().chance(0.06) && household_size(world) + 2 <= MAX_HOUSEHOLD;
    let mut babies = Vec::new();
    for _ in 0..if twins { 2 } else { 1 } {
        if household_size(world) >= MAX_HOUSEHOLD {
            break;
        }
        babies.push(spawn_child(world, carrier, Some(other)));
    }
    let nc = name_of(world, carrier);
    let names: Vec<String> = babies.iter().map(|b| name_of(world, *b)).collect();
    let line = match names.as_slice() {
        [one] => format!("{nc} had a baby: {one}! The baby immediately demanded snacks."),
        [a, b] => format!("TWINS! {nc} had {a} and {b}. Nobody is sleeping for a while."),
        _ => format!("{nc} had a baby, but there was no room in the house. The stork took it back. (This shouldn't happen.)"),
    };
    log(world, LogKind::Life, Some(carrier), line);
    for p in [carrier, other] {
        if world.contains(p) && !world.has::<Dead>(p) {
            add_moodlet(world, p, "new_baby");
        }
    }
    actions::abort_current(world, carrier);
}

/// Spawns a newborn of `a` (and `b`, if still around) next to `a`.
pub fn spawn_child(world: &mut World, a: Entity, b: Option<Entity>) -> Entity {
    spawn_child_as(world, a, b, true)
}

/// Like [`spawn_child`]; `member` = joins the played household (else the caller files them).
pub fn spawn_child_as(world: &mut World, a: Entity, b: Option<Entity>, member: bool) -> Entity {
    let data = data(world);
    let b = b.filter(|b| world.contains(*b));
    let ga = world.get::<Genes>(a).map(|g| g.clone()).unwrap_or_else(|| genetics::genes_from_appearance(&world.get::<Appearance>(a).unwrap()));
    let gb = b.and_then(|b| world.get::<Genes>(b).map(|g| g.clone())).unwrap_or_else(|| ga.clone());
    let pa = world.get::<Personality>(a).unwrap().clone();
    let pb = b.and_then(|b| world.get::<Personality>(b).map(|p| p.clone())).unwrap_or_else(|| pa.clone());
    let last = world.get::<Peon>(a).unwrap().last.clone();
    let mut spec = random_spec(world, LifeStage::Infant, Some(&last));
    let genes = {
        let mut rng = world.res_mut::<SimRng>();
        genetics::inherit(&mut rng, &ga, &gb)
    };
    spec.appearance = genetics::express(&genes, LifeStage::Infant, spec.appearance.shirt, spec.appearance.pants, spec.appearance.hair_style);
    spec.genes = Some(genes);
    if let Some(t) = genetics::inherit_trait(&mut world.res_mut::<SimRng>(), &data, &pa, &pb) {
        spec.traits = vec![t];
    }
    spec.career = None;
    let pos = world.get::<Transform>(a).map(|t| near(world, t.pos)).unwrap_or(Vec3::ZERO);
    let baby = crate::sim::spawn_peon_as(world, spec, pos, member);
    set_parent(world, a, baby);
    if let Some(b) = b {
        set_parent(world, b, baby);
    }
    // siblings start out on good terms (for now)
    let sibs: Vec<Entity> = world.get::<Family>(a).map(|f| f.children.clone()).unwrap_or_default();
    for s in sibs {
        if s != baby && world.contains(s) {
            world.res_mut::<Relationships>().add(s, baby, 25.0, 0.0);
        }
    }
    baby
}

/// A walkable tile center next to `pos` (babies shouldn't spawn inside furniture).
fn near(world: &World, pos: Vec3) -> Vec3 {
    let lot = world.res::<Lot>();
    let l = Lot::level_of(pos);
    let t = Lot::tile_of(pos);
    for r in 0..4 {
        for dx in -r..=r {
            for dz in -r..=r {
                let c = t + glam::IVec2::new(dx, dz);
                if lot.walkable(l, c) {
                    return lot.ground_center(l, c);
                }
            }
        }
    }
    pos
}

/// Adopts a child of the given stage from the Agency of Questionable Storks.
pub fn adopt(world: &mut World, stage: LifeStage) -> Result<Entity, String> {
    if stage > LifeStage::Teen {
        return Err("The agency only places infants, toddlers, children and teens.".into());
    }
    if household_size(world) >= MAX_HOUSEHOLD {
        return Err("The household is full.".into());
    }
    let parents: Vec<Entity> = world
        .res::<Household>()
        .members
        .iter()
        .copied()
        .filter(|m| world.get::<Age>(*m).is_some_and(|a| a.stage.is_adult()))
        .collect();
    if parents.is_empty() {
        return Err("The agency requires at least one adult in the household.".into());
    }
    {
        let mut hh = world.res_mut::<Household>();
        if hh.funds < ADOPTION_FEE {
            return Err(format!("Adoption costs ${ADOPTION_FEE}."));
        }
        hh.funds -= ADOPTION_FEE;
    }
    // parents are the first adult and their spouse, if any
    let first = parents[0];
    let spouse = world.get::<Family>(first).and_then(|f| f.spouse).filter(|s| parents.contains(s));
    let last = world.get::<Peon>(first).unwrap().last.clone();
    let mut spec = random_spec(world, stage, Some(&last));
    spec.career = None;
    let door = { let lot = world.res::<Lot>(); lot.ground_center(0, actions::exit_tile(&lot)) };
    let kid = spawn_peon(world, spec, door);
    set_parent(world, first, kid);
    if let Some(s) = spouse {
        set_parent(world, s, kid);
    }
    for p in [Some(first), spouse].into_iter().flatten() {
        add_moodlet(world, p, "new_baby");
    }
    let name = name_of(world, kid);
    log(world, LogKind::Life, Some(kid), format!("The Agency of Questionable Storks delivered {name} ({}). Some assembly required.", stage.label()));
    Ok(kid)
}
