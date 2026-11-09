//! Ghosts: the dead get a grave marker on the lot and may haunt it at night. Hauntings are
//! mostly harmless pranks: dirtying things, the odd breakage, and spooking the living.
//! Selling the grave lets the ghost move on.

use std::sync::Arc;

use glam::{IVec2, Vec3};
use hearth_ecs::{Entity, With, World};

use crate::components::*;
use crate::data::Need;
use crate::economy::Household;
use crate::log::LogKind;
use crate::lot::{LEVEL_HEIGHT, Lot};
use crate::neighborhood::{LotId, Neighborhood, active_lot, park_active, unpark};
use crate::objects::ObjectInstance;
use crate::rng::SimRng;
use crate::sim::{Data, add_moodlet, data, log, name_of, now, place_object, present_peons, set_bubble};
use crate::time::{Clock, Settings};

pub const GRAVE_OBJECT: &str = "tombstone";
/// Ghost drift speed, meters per minute (they don't bother with doors).
const DRIFT_SPEED: f32 = 0.5;

/// Puts a grave marker in the yard of the dead peon's home and turns them into a (dormant) ghost.
/// If home is another lot than the live one, the grave goes into that parked lot and the ghost
/// waits there (off-lot) until the lot is next live.
pub fn bury(world: &mut World, e: Entity, home: Option<LotId>) {
    match home {
        Some(lot) if lot != active_lot(world) => bury_away(world, e, lot),
        _ => bury_here(world, e),
    }
}

fn bury_here(world: &mut World, e: Entity) {
    let Some(tile) = grave_spot(world) else {
        log(world, LogKind::Info, Some(e), format!("There was nowhere to put {}'s tombstone, so the urn went on the fridge.", name_of(world, e)));
        return;
    };
    match place_object(world, GRAVE_OBJECT, 0, tile, 0, false) {
        Ok(grave) => {
            world.insert_one(e, Ghost { grave, visible_until: 0, drift_to: None, next_prank: 0 });
            world.remove::<Visitor>(e);
            world.remove::<Offlot>(e);
            let pos = world.res::<Lot>().ground_center(0, tile);
            if let Some(mut tf) = world.get_mut::<Transform>(e) {
                tf.pos = pos;
                tf.prev_pos = pos;
            }
        }
        Err(why) => log::warn!("couldn't place grave: {why}"),
    }
}

/// Buries `e` on parked `lot`: the lot is made live in a scratch world, the grave placed there, and
/// the lot parked again with its objects' real-world handles (plus a fresh one for the grave).
fn bury_away(world: &mut World, e: Entity, lot: LotId) {
    let Some(parked) = world.res_mut::<Neighborhood>().lots[lot].parked.take() else { return };
    let mut scratch = World::new();
    scratch.insert_resource(Data(Arc::clone(&world.res::<Data>().0)));
    scratch.insert_resource(Lot::new(1, 1));
    scratch.insert_resource(Household::default());
    let map = unpark(&mut scratch, parked);
    scratch.res_mut::<Lot>().recompute_rooms();
    let grave = grave_spot(&scratch).ok_or_else(|| "no room in the yard".to_string()).and_then(|t| place_object(&mut scratch, GRAVE_OBJECT, 0, t, 0, false));
    let mut parked = park_active(&mut scratch);
    // a handle no live entity will ever have, for the ghost to find its grave by
    let handle = world.spawn_empty();
    world.despawn(handle);
    for o in &mut parked.objects {
        o.id = match o.id {
            Some(s) if grave.as_ref().ok() == Some(&s) => Some(handle),
            Some(s) => map.iter().find(|(_, new)| *new == s).map(|(old, _)| *old),
            None => None,
        };
    }
    let address = {
        let mut hood = world.res_mut::<Neighborhood>();
        hood.lots[lot].value = parked.build_value();
        hood.lots[lot].parked = Some(parked);
        hood.lots[lot].address.clone()
    };
    let name = name_of(world, e);
    match grave {
        Ok(_) => {
            world.insert_one(e, Ghost { grave: handle, visible_until: 0, drift_to: None, next_prank: 0 });
            world.remove::<Visitor>(e);
            world.insert_one(e, Offlot { visit_at: u64::MAX });
            log(world, LogKind::Life, Some(e), format!("{name} was laid to rest in the yard at {address}."));
        }
        Err(why) => {
            log::warn!("couldn't place grave at {address}: {why}");
            log(world, LogKind::Info, Some(e), format!("There was nowhere to put {name}'s tombstone, so the urn went on a shelf at {address}."));
        }
    }
}

/// An empty, flat outdoor tile with a walkable tile in front of it, as close to the street as possible.
fn grave_spot(world: &World) -> Option<IVec2> {
    let lot = world.res::<Lot>();
    let street = crate::actions::exit_tile(&lot);
    let want = street + IVec2::new(-5, -3);
    let mut best: Option<(i32, IVec2)> = None;
    for x in 1..lot.width - 1 {
        for z in 1..lot.depth - 2 {
            let t = IVec2::new(x, z);
            let front = t + IVec2::Y;
            let outdoors = !lot.has_floor(0, t) && lot.room_at(0, t).is_none_or(|r| !r.indoor);
            if !outdoors || !lot.walkable(0, t) || !lot.walkable(0, front) || !lot.terrain.tile_flat(t) || t == street || front == street {
                continue;
            }
            // keep the street-side path clear
            if (t.x - street.x).abs() <= 1 {
                continue;
            }
            let d = (t - want).abs().element_sum();
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, t));
            }
        }
    }
    best.map(|b| b.1)
}

/// Night-time hauntings.
pub fn haunt(world: &mut World) {
    if !world.res::<Settings>().ghosts {
        return;
    }
    let tick = now(world);
    let clock = *world.res::<Clock>();
    let ghosts: Vec<(Entity, Ghost)> =
        world.query_filtered::<(Entity, &Ghost), (With<Dead>, hearth_ecs::Without<Offlot>)>().iter().map(|(e, g)| (e, g.clone())).collect();
    let mut ghosts = ghosts;
    ghosts.sort_by_key(|(e, _)| *e);
    for (e, g) in ghosts {
        // the grave is gone: rest in peace
        if !world.has::<ObjectInstance>(g.grave) {
            world.remove::<Ghost>(e);
            log(world, LogKind::Life, Some(e), format!("{}'s ghost has finally moved on. They left a note: 'Thanks for nothing.'", name_of(world, e)));
            continue;
        }
        let visible = g.visible_until > tick;
        if !visible {
            // ~once a night on average, between 22:00 and 6:00
            if clock.is_night() && world.res_mut::<SimRng>().chance(1.0 / 480.0) {
                manifest(world, e, &g, tick);
            }
            continue;
        }
        drift(world, e, tick);
        if tick >= g.next_prank {
            prank(world, e);
            let wait = 20 + world.res_mut::<SimRng>().below(30) as u64;
            world.get_mut::<Ghost>(e).unwrap().next_prank = tick + wait;
        }
        if !clock.is_night() {
            // sunrise: back to the grave
            world.get_mut::<Ghost>(e).unwrap().visible_until = tick;
        }
    }
}

fn manifest(world: &mut World, e: Entity, g: &Ghost, tick: u64) {
    let Some(pos) = world.get::<ObjectInstance>(g.grave).map(|o| o.center(data(world).object(o.def))) else { return };
    let mins = 60 + world.res_mut::<SimRng>().below(90) as u64;
    {
        let mut gh = world.get_mut::<Ghost>(e).unwrap();
        gh.visible_until = tick + mins;
        gh.drift_to = None;
        gh.next_prank = tick + 15;
    }
    if let Some(mut tf) = world.get_mut::<Transform>(e) {
        tf.pos = pos + Vec3::Y * 0.3;
        tf.prev_pos = tf.pos;
    }
    set_bubble(world, e, Some("boo".into()), Some("Wooooo".into()), 20);
    log(world, LogKind::Funny, Some(e), format!("The ghost of {} rose from the grave, mostly to complain about the lawn.", name_of(world, e)));
}

fn drift(world: &mut World, e: Entity, _tick: u64) {
    let (lw, ld) = {
        let lot = world.res::<Lot>();
        (lot.width, lot.depth)
    };
    let Some(tf) = world.get::<Transform>(e).map(|t| *t) else { return };
    let target = world.get::<Ghost>(e).and_then(|g| g.drift_to);
    let reached = target.is_none_or(|t| (t - tf.pos).length() < 0.2);
    if reached {
        // float toward something interesting: a living peon (usually) or a random spot
        let peons = present_peons(world);
        let pick = if !peons.is_empty() && world.res_mut::<SimRng>().chance(0.6) {
            let i = world.res_mut::<SimRng>().below(peons.len());
            world.get::<Transform>(peons[i]).map(|t| t.pos)
        } else {
            None
        };
        let to = pick.unwrap_or_else(|| {
            let mut rng = world.res_mut::<SimRng>();
            let x = rng.range_f32(1.0, lw as f32 - 1.0);
            let z = rng.range_f32(1.0, ld as f32 - 1.0);
            Vec3::new(x, tf.pos.y, z)
        });
        // ghosts float a little off the floor, but stick to the level they're haunting
        let level = Lot::level_of(to);
        let to = Vec3::new(to.x, level as f32 * LEVEL_HEIGHT + 0.3, to.z);
        world.get_mut::<Ghost>(e).unwrap().drift_to = Some(to);
        return;
    }
    let to = target.unwrap();
    let d = to - tf.pos;
    let step = d.length().min(DRIFT_SPEED);
    if let Some(mut t) = world.get_mut::<Transform>(e)
        && d.length() > 1e-4
    {
        t.pos += d.normalize() * step;
        t.yaw = d.x.atan2(d.z);
    }
}

fn prank(world: &mut World, ghost: Entity) {
    let data = data(world);
    let Some(gp) = world.get::<Transform>(ghost).map(|t| t.pos) else { return };
    let name = name_of(world, ghost);
    // spook the living nearby
    let mut spooked = Vec::new();
    for p in present_peons(world) {
        let Some(pp) = world.get::<Transform>(p).map(|t| t.pos) else { continue };
        if (pp - gp).length() < 4.0 && (pp.y - gp.y).abs() < 1.5 {
            spooked.push(p);
        }
    }
    for p in &spooked {
        let asleep = world
            .get::<ActionQueue>(*p)
            .and_then(|q| q.current.as_ref().map(|c| crate::autonomy::is_sleeping(world, &data, c)))
            .unwrap_or(false);
        if asleep {
            continue;
        }
        add_moodlet(world, *p, "saw_ghost");
        set_bubble(world, *p, Some("!!".into()), Some("AAAH!".into()), 10);
        let brave = world.get::<Personality>(*p).is_some_and(|x| x.has("gremlin") || x.has("cheerful"));
        if let Some(mut n) = world.get_mut::<Needs>(*p) {
            if brave {
                n.add(Need::Fun, 15.0);
            } else {
                n.add(Need::Bladder, -10.0);
                if n.is_active(Need::Vigilance) {
                    n.add(Need::Vigilance, -20.0);
                }
            }
        }
    }
    if let Some(&victim) = spooked.first() {
        if world.res_mut::<SimRng>().chance(0.5) {
            let line = format!("{}'s ghost said 'boo' to {}. {} did not appreciate it.", name, name_of(world, victim), name_of(world, victim));
            log(world, LogKind::Funny, Some(ghost), line);
        }
        return;
    }
    // otherwise mess with the nearest object
    let mut best: Option<(f32, Entity)> = None;
    for (oe, o) in world.query::<(Entity, &ObjectInstance)>().iter() {
        let def = data.object(o.def);
        if def.id == GRAVE_OBJECT || def.interactions.is_empty() {
            continue;
        }
        let d = (o.center(def) - gp).length();
        if d < 3.0 && best.is_none_or(|b| d < b.0 || (d == b.0 && oe < b.1)) {
            best = Some((d, oe));
        }
    }
    let Some((_, oe)) = best else { return };
    let breaks = world.res_mut::<SimRng>().chance(0.08);
    let oname = {
        let mut o = world.get_mut::<ObjectInstance>(oe).unwrap();
        o.dirt = (o.dirt + 0.3).min(1.0);
        if breaks {
            o.broken = true;
        }
        data.object(o.def).name.clone()
    };
    let line = if breaks {
        format!("{name}'s ghost broke the {oname}. Spectral vandalism is not covered by insurance.")
    } else {
        format!("{name}'s ghost smeared ectoplasm all over the {oname}.")
    };
    log(world, LogKind::Funny, Some(ghost), line);
}

/// Ghosts currently visible on the lot (for rendering).
pub fn visible_ghosts(world: &World) -> Vec<Entity> {
    let tick = now(world);
    let mut v: Vec<Entity> = world.query::<(Entity, &Ghost)>().iter().filter(|(_, g)| g.visible_until > tick).map(|(e, _)| e).collect();
    v.sort();
    v
}
