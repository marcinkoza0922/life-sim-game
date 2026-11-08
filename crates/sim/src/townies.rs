//! The neighborhood: townies who live on the other lots on the street. They're simulated
//! off-lot (as [`AtWork`] without a job) and drop by now and then: they ring the bell, chat,
//! raid the fridge, and go home when they're tired, it's late, or nature calls.

use hearth_ecs::{Entity, With, World};
use serde::{Deserialize, Serialize};

use crate::actions;
use crate::components::*;
use crate::data::{LifeStage, Need};
use crate::log::LogKind;
use crate::lot::Lot;
use crate::nav::NavNode;
use crate::rng::SimRng;
use crate::sim::{log, name_of, now, random_spec, set_bubble, spawn_peon_as};
use crate::time::{Clock, TICKS_PER_DAY};

/// A neighbor who lives elsewhere on the street.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Townie {
    pub address: String,
    /// When the current visit ends.
    pub leave_at: u64,
    /// Walking back to the street.
    pub leaving: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LotInfo {
    pub address: String,
    pub residents: String,
}

/// The street the household lives on.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Neighborhood {
    pub name: String,
    pub home: String,
    pub lots: Vec<LotInfo>,
}

impl Default for Neighborhood {
    fn default() -> Self {
        Neighborhood { name: "Pleasant Gulch".into(), home: "12 Sycamore Lane".into(), lots: Vec::new() }
    }
}

const STREETS: [&str; 4] = ["Sycamore Lane", "Gravy Boat Court", "Mild Peril Avenue", "Ennui Street"];

/// All townies, sorted.
pub fn townies(world: &World) -> Vec<(Entity, Townie)> {
    let mut v: Vec<(Entity, Townie)> = world.query::<(Entity, &Townie)>().iter().map(|(e, t)| (e, t.clone())).collect();
    v.sort_by_key(|(e, _)| *e);
    v
}

fn next_visit(world: &World, earliest_days: f32, latest_days: f32) -> u64 {
    let clock = *world.res::<Clock>();
    let mut rng = world.res_mut::<SimRng>();
    let delay = (rng.range_f32(earliest_days, latest_days) * TICKS_PER_DAY as f32) as u64;
    let t = clock.tick + delay;
    // visits happen in the daytime: snap into 9:00–19:00
    let mod_ = t % TICKS_PER_DAY;
    if !(9 * 60..19 * 60).contains(&mod_) {
        let day = t / TICKS_PER_DAY + if mod_ >= 19 * 60 { 1 } else { 0 };
        day * TICKS_PER_DAY + 9 * 60 + rng.below(8 * 60) as u64
    } else {
        t
    }
}

/// Moves `n` townies into the neighborhood (idempotent-ish: adds to whoever is there).
pub fn populate(world: &mut World, n: usize) {
    let mut lots: Vec<LotInfo> = world.res::<Neighborhood>().lots.clone();
    for i in 0..n {
        let stage = {
            let mut rng = world.res_mut::<SimRng>();
            *rng.pick(&[LifeStage::YoungAdult, LifeStage::YoungAdult, LifeStage::Adult, LifeStage::Adult, LifeStage::Elder]).unwrap()
        };
        let mut spec = random_spec(world, stage, None);
        spec.career = None;
        let number = 3 + 2 * (lots.len() as u32 + i as u32);
        let street = STREETS[(lots.len() + i) % STREETS.len()];
        let address = format!("{number} {street}");
        let e = spawn_peon_as(world, spec, glam::Vec3::ZERO, false);
        let until = next_visit(world, 0.1, 2.0);
        world.insert(e, (Townie { address: address.clone(), leave_at: 0, leaving: false }, AtWork { until, event_at: u64::MAX, event_done: true, school: false }));
        lots.push(LotInfo { address, residents: crate::sim::name_of(world, e) + " " + &world.get::<Peon>(e).unwrap().last });
    }
    world.res_mut::<Neighborhood>().lots = lots;
}

/// Ask a townie to come over now.
pub fn invite(world: &mut World, e: Entity) {
    let tick = now(world);
    if let Some(mut a) = world.get_mut::<AtWork>(e)
        && world.has::<Townie>(e)
    {
        a.until = tick;
    }
}

/// Arrivals and departures.
pub fn visits(world: &mut World) {
    let tick = now(world);
    let hour = world.res::<Clock>().hour();
    let list: Vec<(Entity, Townie, Option<u64>)> = world
        .query_filtered::<(Entity, &Townie, Option<&AtWork>), hearth_ecs::Without<Dead>>()
        .iter()
        .map(|(e, t, a)| (e, t.clone(), a.map(|a| a.until)))
        .collect();
    let mut list = list;
    list.sort_by_key(|(e, _, _)| *e);
    let street = {
        let lot = world.res::<Lot>();
        lot.ground_center(0, actions::exit_tile(&lot))
    };
    for (e, t, away_until) in list {
        match away_until {
            Some(until) if tick >= until => arrive(world, e, &t, street, tick),
            Some(_) => {}
            None => {
                let needs_home = world.get::<Needs>(e).is_some_and(|n| n.critical_physical(15.0).iter().any(|_| true));
                if !t.leaving && (tick >= t.leave_at || needs_home || !(8..22).contains(&hour)) {
                    start_leaving(world, e, needs_home);
                } else if t.leaving {
                    let at_street = world.get::<Transform>(e).is_some_and(|tf| (tf.pos - street).length() < 0.6);
                    let idle = world.get::<ActionQueue>(e).is_some_and(|q| q.current.is_none() && q.queue.is_empty());
                    if at_street || idle || tick > t.leave_at + 120 {
                        go_home(world, e);
                    }
                }
            }
        }
    }
}

fn arrive(world: &mut World, e: Entity, t: &Townie, street: glam::Vec3, tick: u64) {
    world.remove::<AtWork>(e);
    let stay = 120 + world.res_mut::<SimRng>().below(180) as u64;
    {
        let mut tw = world.get_mut::<Townie>(e).unwrap();
        tw.leave_at = tick + stay;
        tw.leaving = false;
    }
    if let Some(mut tf) = world.get_mut::<Transform>(e) {
        tf.pos = street;
        tf.prev_pos = street;
    }
    // they freshened up at home
    if let Some(mut n) = world.get_mut::<Needs>(e) {
        for k in Need::ALL {
            let v = n.get(k).max(70.0);
            n.set(k, v);
        }
        n.starving_minutes = 0;
    }
    if let Some(mut q) = world.get_mut::<ActionQueue>(e) {
        q.current = None;
        q.queue.clear();
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
    let line = LINES[i].replace("{n}", &name_of(world, e)).replace("{a}", &t.address);
    log(world, LogKind::Social, Some(e), line);
}

fn start_leaving(world: &mut World, e: Entity, needs_home: bool) {
    actions::abort_current(world, e);
    if let Some(mut q) = world.get_mut::<ActionQueue>(e) {
        q.queue.clear();
    }
    let exit = actions::exit_tile(&world.res::<Lot>());
    actions::push_front(world, e, ActionKind::GoTo { level: 0, tile: exit }, Source::System, "Go Home".into());
    world.get_mut::<Townie>(e).unwrap().leaving = true;
    let name = name_of(world, e);
    if needs_home {
        log(world, LogKind::Funny, Some(e), format!("{name} suddenly remembered they left the oven on. (They need their own bathroom.)"));
    }
    let _ = NavNode::new(0, exit);
}

fn go_home(world: &mut World, e: Entity) {
    actions::abort_current(world, e);
    if let Some(mut q) = world.get_mut::<ActionQueue>(e) {
        q.queue.clear();
    }
    let until = next_visit(world, 1.0, 3.0);
    world.insert_one(e, AtWork { until, event_at: u64::MAX, event_done: true, school: false });
    world.get_mut::<Townie>(e).unwrap().leaving = false;
    log(world, LogKind::Info, Some(e), format!("{} went home.", name_of(world, e)));
}

/// Townies currently on the lot.
pub fn visitors(world: &World) -> Vec<Entity> {
    let mut v: Vec<Entity> = world.query_filtered::<Entity, (With<Townie>, hearth_ecs::Without<AtWork>, hearth_ecs::Without<Dead>)>().iter().collect();
    v.sort();
    v
}
