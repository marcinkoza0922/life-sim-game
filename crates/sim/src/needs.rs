//! Need decay, mood, rooms/environment, and what happens when needs bottom out.

use hearth_ecs::{Entity, With, Without, World};

use crate::actions;
use crate::components::*;
use crate::data::{LifeStage, Need, Pose};
use crate::log::LogKind;
use crate::lot::Lot;
use crate::objects::ObjectInstance;
use crate::sim::{add_moodlet, data, fill, log, name_of, now, set_bubble};
use crate::time::Clock;

/// Base decay in points per game hour.
fn base_decay(n: Need) -> f32 {
    match n {
        Need::Hunger => 5.5,
        Need::Energy => 4.0,
        Need::Bladder => 8.5,
        Need::Hygiene => 3.5,
        Need::Social => 3.0,
        Need::Fun => 4.5,
        Need::Comfort => 6.0,
        Need::Environment => 0.0,
        // personality needs: slow, gentle
        _ => 2.2,
    }
}

fn stage_mult(s: LifeStage, n: Need) -> f32 {
    use LifeStage::*;
    use Need::*;
    match (s, n) {
        (Infant, Hunger | Energy) => 1.6,
        (Infant, Bladder) => 1.4,
        (Infant, Social | Fun) => 1.3,
        (Toddler, Hunger | Energy | Fun) => 1.35,
        (Toddler, Bladder) => 1.3,
        (Child, Fun) => 1.3,
        (Child, Hunger) => 1.15,
        (Teen, Hunger) => 1.25,
        (Teen, Social) => 1.2,
        (Elder, Energy) => 1.2,
        (Elder, Bladder) => 1.3,
        (Elder, Fun) => 0.85,
        _ => 1.0,
    }
}

pub fn update_rooms(world: &mut World) {
    let tick = now(world);
    let dirty = world.res::<Lot>().rooms_dirty;
    if dirty {
        world.res_mut::<Lot>().recompute_rooms();
    }
    // re-score rooms every 30 minutes or after changes
    if !(dirty || tick.is_multiple_of(30)) {
        return;
    }
    let data = data(world);
    let mut lot = world.res_mut::<Lot>();
    for r in lot.rooms.iter_mut() {
        r.beauty = 0.0;
        r.dirt = 0.0;
    }
    // sorted so float sums don't depend on storage order (saves restore entities in a different layout)
    let mut objs: Vec<(Entity, ObjectInstance)> = world.query::<(Entity, &ObjectInstance)>().iter().map(|(e, o)| (e, o.clone())).collect();
    objs.sort_by_key(|(e, _)| *e);
    for (_, o) in &objs {
        let def = data.object(o.def);
        let t = o.tile;
        if let Some(&rid) = lot.room_of.get(&(o.level, t.x, t.y)) {
            let r = &mut lot.rooms[rid as usize];
            r.beauty += def.beauty - if o.broken { 2.0 } else { 0.0 };
            r.dirt += o.dirt;
        }
    }
    for r in lot.rooms.iter_mut() {
        r.score = if !r.indoor {
            60.0
        } else {
            let size = (r.tiles as f32).sqrt().max(1.0);
            (45.0 + 12.0 * r.beauty / size - 25.0 * r.dirt + size.min(6.0) * 2.0).clamp(0.0, 100.0)
        };
    }
}

pub fn decay_needs(world: &mut World) {
    let data = data(world);
    let lot = world.res::<Lot>();
    let mut q = world.query_filtered::<(Entity, &mut Needs, &Age, &Personality, &Posture, &Transform, &ActionQueue), (Without<Dead>, Without<AtWork>)>();
    for (_e, needs, age, pers, posture, tf, aq) in q.iter() {
        let boosted = actions::active_rates(world, &data, aq);
        let sleeping = posture.pose == Pose::Lie && boosted[Need::Energy.idx()] > 0.0;
        for n in Need::ALL {
            if !needs.is_active(n) {
                continue;
            }
            if n == Need::Environment {
                let target = lot.room_at(tf.level(), tf.tile()).map(|r| r.score).unwrap_or(55.0);
                let cur = needs.get(n);
                let step = (target - cur).clamp(-20.0 / 60.0, 20.0 / 60.0);
                needs.add(n, step);
                continue;
            }
            if boosted[n.idx()] > 0.0 {
                continue;
            }
            let mut rate = base_decay(n) * stage_mult(age.stage, n);
            for t in &pers.traits {
                if let Some(m) = data.trait_def(t).and_then(|d| d.decay.get(&n)) {
                    rate *= m;
                }
            }
            match n {
                Need::Comfort if matches!(posture.pose, Pose::Sit | Pose::Lie) => rate = 0.0,
                Need::Hunger | Need::Bladder if sleeping => rate *= 0.5,
                Need::Social | Need::Fun if sleeping => rate *= 0.2,
                Need::Energy if posture.walking => rate *= 1.2,
                _ => {}
            }
            needs.add(n, -rate / 60.0);
        }
    }
}

pub fn update_mood(world: &mut World) {
    let tick = now(world);
    let hourly = tick.is_multiple_of(60);
    let mut low: Vec<(Entity, &'static str)> = Vec::new();
    for (e, needs, mood) in world.query_filtered::<(Entity, &Needs, &mut Mood), Without<Dead>>().iter() {
        mood.moodlets.retain(|m| m.until > tick);
        let mut sum = 0.0;
        let mut wsum = 0.0;
        for n in needs.active_needs() {
            let w = match n {
                Need::Environment => 0.5,
                n if n.is_core() => 1.0,
                _ => 0.3,
            };
            // low needs hurt more than full needs help
            let v = needs.get(n) - 50.0;
            sum += if v < 0.0 { v * 1.5 } else { v } * w;
            wsum += w;
        }
        let base = if wsum > 0.0 { sum / wsum } else { 0.0 };
        let ml: f32 = mood.moodlets.iter().map(|m| m.value).sum();
        mood.value = (base + ml).clamp(-100.0, 100.0);
        if hourly {
            for (n, id) in [(Need::Hunger, "starving"), (Need::Energy, "exhausted"), (Need::Hygiene, "smelly"), (Need::Social, "lonely")] {
                if needs.get(n) < 15.0 {
                    low.push((e, id));
                }
            }
        }
    }
    for (e, id) in low {
        add_moodlet(world, e, id);
    }
}

/// Off-screen nanny who looks after babies when no teen/adult is home. Costs money, saves lives.
#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Nanny {
    pub present: bool,
    pub name: String,
}

fn nanny(world: &mut World) {
    let tick = now(world);
    if !tick.is_multiple_of(15) {
        return;
    }
    let mut babies = Vec::new();
    let mut grownups = 0;
    for (e, age) in world.query_filtered::<(Entity, &Age), (Without<Dead>, Without<AtWork>)>().iter() {
        if age.stage <= LifeStage::Toddler {
            babies.push(e);
        } else if age.stage >= LifeStage::Teen {
            grownups += 1;
        }
    }
    let needed = !babies.is_empty() && grownups == 0;
    let present = world.res::<Nanny>().present;
    if needed && !present {
        const NAMES: [&str; 6] = ["Gertrude", "Mrs. Pennywhistle", "Big Sheila", "Bartholomew Sr.", "Nanny Nine-Fingers", "Doreen"];
        let n = NAMES[world.res_mut::<crate::rng::SimRng>().below(NAMES.len())].to_string();
        log(world, LogKind::Funny, None, format!("{n} the nanny arrived, looked at the kitchen, and sighed audibly."));
        *world.res_mut::<Nanny>() = Nanny { present: true, name: n };
    } else if !needed && present {
        let n = world.res::<Nanny>().name.clone();
        log(world, LogKind::Money, None, format!("{n} the nanny left, taking her fee and a few cookies."));
        world.res_mut::<Nanny>().present = false;
    }
    if needed {
        // $20/hour, billed every 15 minutes
        world.res_mut::<crate::economy::Household>().funds -= 5;
        for b in babies {
            let mut n = world.get_mut::<Needs>(b).unwrap();
            for (need, floor) in [(Need::Hunger, 55.0), (Need::Hygiene, 50.0), (Need::Bladder, 40.0), (Need::Social, 45.0)] {
                if n.get(need) < floor - 20.0 {
                    n.set(need, floor);
                }
            }
        }
    }
}

/// Accidents, passing out, starvation.
pub fn consequences(world: &mut World) {
    nanny(world);
    let tick = now(world);
    let mut peons: Vec<Entity> = world.query_filtered::<Entity, (With<Needs>, Without<Dead>, Without<AtWork>)>().iter().collect();
    peons.sort();
    for e in peons {
        let (bladder, energy, hunger, stage) = {
            let n = world.get::<Needs>(e).unwrap();
            let a = world.get::<Age>(e).unwrap();
            (n.get(Need::Bladder), n.get(Need::Energy), n.get(Need::Hunger), a.stage)
        };
        let small = stage <= LifeStage::Toddler;

        if bladder <= 0.0 {
            {
                let mut n = world.get_mut::<Needs>(e).unwrap();
                n.set(Need::Bladder, 100.0);
                n.add(Need::Hygiene, if small { -30.0 } else { -50.0 });
            }
            if small {
                set_bubble(world, e, Some("diaper".into()), None, 60);
            } else {
                add_moodlet(world, e, "peed_self");
                let t = fill(world, e, "{name} couldn't hold it any longer. There is now a puddle. Nobody make eye contact.");
                log(world, LogKind::Funny, Some(e), t);
                dirty_room_of(world, e, 0.4);
            }
        }

        if energy <= 0.0 && !small {
            let passing = world
                .get::<ActionQueue>(e)
                .and_then(|q| q.current.as_ref().map(|c| matches!(c.action.kind, ActionKind::PassOut)))
                .unwrap_or(false);
            if !passing {
                actions::abort_current(world, e);
                actions::push_front(world, e, ActionKind::PassOut, Source::System, "Pass Out".into());
                add_moodlet(world, e, "slept_badly");
                let t = fill(world, e, "{name} fell asleep standing up, then horizontally.");
                log(world, LogKind::Funny, Some(e), t);
            }
        }

        if hunger <= 0.0 {
            let mins = {
                let mut n = world.get_mut::<Needs>(e).unwrap();
                n.starving_minutes += 1;
                n.starving_minutes
            };
            if mins == 60 {
                let t = format!("{} is starving! Feed them, you monster.", name_of(world, e));
                log(world, LogKind::Warning, Some(e), t);
            }
            if small && mins >= 6 * 60 {
                let t = format!(
                    "A social worker arrived, looked at the fridge, looked at {}, and left with {} under one arm.",
                    "the household",
                    name_of(world, e)
                );
                log(world, LogKind::Life, Some(e), t);
                crate::life::remove_from_lot(world, e);
            } else if !small && mins >= 20 * 60 {
                crate::life::die(world, e, "starvation");
            }
        } else if let Some(mut n) = world.get_mut::<Needs>(e) {
            n.starving_minutes = 0;
        }

        // babies cry when something is wrong, and crying wakes the grown-ups
        if small && tick.is_multiple_of(20) {
            let (low_need, v) = world.get::<Needs>(e).unwrap().lowest_core();
            if v < 30.0 {
                set_bubble(world, e, Some(format!("cry:{}", low_need.label())), Some("WAAAH!".into()), 15);
                wake_caretakers(world, e);
            }
        }
    }
}

fn dirty_room_of(world: &World, e: Entity, amount: f32) {
    let Some(tf) = world.get::<Transform>(e).map(|t| *t) else { return };
    let lot = world.res::<Lot>();
    let Some(room) = lot.room_of.get(&(tf.level(), tf.tile().x, tf.tile().y)).copied() else { return };
    // dirty the nearest object in the same room (the closest thing to a puddle we can track)
    let mut best: Option<(f32, Entity)> = None;
    for (oe, o) in world.query::<(Entity, &ObjectInstance)>().iter() {
        if lot.room_of.get(&(o.level, o.tile.x, o.tile.y)) == Some(&room) {
            let d = (o.tile.as_vec2() - tf.tile().as_vec2()).length();
            if best.is_none_or(|b| d < b.0 || (d == b.0 && oe < b.1)) {
                best = Some((d, oe));
            }
        }
    }
    drop(lot);
    if let Some((_, oe)) = best
        && let Some(mut o) = world.get_mut::<ObjectInstance>(oe) {
            o.dirt = (o.dirt + amount).min(1.0);
        }
    let _ = world.res::<Clock>();
}

/// A crying baby may wake one sleeping teen/adult on the lot.
fn wake_caretakers(world: &mut World, baby: Entity) {
    let data = data(world);
    let mut sleepers: Vec<Entity> = Vec::new();
    for (e, age, q) in world.query_filtered::<(Entity, &Age, &ActionQueue), (Without<Dead>, Without<AtWork>)>().iter() {
        if e != baby && age.stage >= LifeStage::Teen
            && let Some(c) = &q.current
                && crate::autonomy::is_sleeping(world, &data, c) {
                    sleepers.push(e);
                }
    }
    sleepers.sort();
    if sleepers.is_empty() || !world.res_mut::<crate::rng::SimRng>().chance(0.5) {
        return;
    }
    let i = world.res_mut::<crate::rng::SimRng>().below(sleepers.len());
    let who = sleepers[i];
    if let Some(mut q) = world.get_mut::<ActionQueue>(who)
        && let Some(c) = q.current.as_mut() {
            c.cancel = true;
        }
    let t = format!("{} was woken up by {}'s crying. Bleary-eyed parenting commences.", name_of(world, who), name_of(world, baby));
    log(world, LogKind::Info, Some(who), t);
}
