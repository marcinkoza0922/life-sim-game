//! Action queue execution: route → walk → perform → finish, plus player orders and refusals.

use glam::{IVec2, Vec3};
use hearth_ecs::{Entity, World};

use crate::autonomy::{self, free_slots, interaction_check, tag_multiplier};
use crate::components::*;
use crate::data::{DurationDef, GameData, LifeStage, Need, Outcome, Pose};
use crate::economy::Household;
use crate::log::LogKind;
use crate::lot::Lot;
use crate::nav::{NavField, NavNode, waypoints};
use crate::objects::ObjectInstance;
use crate::rng::SimRng;
use crate::sim::{add_moodlet, data, fill, log, name_of, now, present_peons, say, set_bubble};
use crate::social::{Relationships, accept_chance, social_allowed};

/// Monotonic action id source.
#[derive(Default)]
pub struct NextActionId(pub u64);

fn next_id(world: &World) -> u64 {
    let mut n = world.res_mut::<NextActionId>();
    n.0 += 1;
    n.0
}

pub fn walk_speed(stage: LifeStage) -> f32 {
    // meters per game minute (1 tick)
    match stage {
        LifeStage::Infant => 0.35,
        LifeStage::Toddler => 0.6,
        LifeStage::Child => 1.15,
        LifeStage::Elder => 0.9,
        _ => 1.3,
    }
}

pub fn label_for(world: &World, data: &GameData, kind: &ActionKind) -> String {
    match kind {
        ActionKind::Object { obj, interaction } => world
            .get::<ObjectInstance>(*obj)
            .map(|o| data.object(o.def).interactions[*interaction].name.clone())
            .unwrap_or_else(|| "???".into()),
        ActionKind::Social { target, social } => format!("{} {}", data.socials[*social].name, name_of(world, *target)),
        ActionKind::Respond { initiator, .. } => format!("Talking with {}", name_of(world, *initiator)),
        ActionKind::GoTo { .. } => "Go Here".into(),
        ActionKind::Walk { .. } => "Walk".into(),
        ActionKind::GoToWork => "Go to Work".into(),
        ActionKind::GoToSchool => "Catch the School Bus".into(),
        ActionKind::PassOut => "Pass Out".into(),
        ActionKind::Idle { .. } => "Idle".into(),
    }
}

/// Need gain per hour the current action provides (for skipping decay).
pub fn active_rates(world: &World, data: &GameData, aq: &ActionQueue) -> [f32; Need::COUNT] {
    let mut out = [0.0; Need::COUNT];
    let Some(cur) = &aq.current else { return out };
    if !matches!(cur.phase, Phase::Performing { .. }) {
        return out;
    }
    match &cur.action.kind {
        ActionKind::Object { obj, interaction } => {
            if let Some(o) = world.get::<ObjectInstance>(*obj) {
                for (n, r) in &data.object(o.def).interactions[*interaction].rates {
                    out[n.idx()] = *r;
                }
            }
        }
        ActionKind::Social { social, .. } => {
            for (n, r) in &data.socials[*social].rates {
                out[n.idx()] = *r;
            }
        }
        ActionKind::Respond { social, .. } => {
            for (n, r) in &data.socials[*social].target_rates {
                out[n.idx()] = *r;
            }
        }
        ActionKind::PassOut => out[Need::Energy.idx()] = 30.0,
        _ => {}
    }
    out
}

pub fn push_front(world: &World, e: Entity, kind: ActionKind, source: Source, label: String) -> u64 {
    let id = next_id(world);
    if let Some(mut q) = world.get_mut::<ActionQueue>(e) {
        q.queue.push_front(QueuedAction { id, kind, source, label });
    }
    id
}

pub fn push_back(world: &World, e: Entity, kind: ActionKind, source: Source, label: String) -> u64 {
    let id = next_id(world);
    if let Some(mut q) = world.get_mut::<ActionQueue>(e) {
        q.queue.push_back(QueuedAction { id, kind, source, label });
    }
    id
}

fn release_slot(world: &World, obj: Entity, e: Entity) {
    if let Some(mut o) = world.get_mut::<ObjectInstance>(obj) {
        for s in o.slot_users.iter_mut() {
            if *s == Some(e) {
                *s = None;
            }
        }
    }
}

/// Immediately stops the current action without outcomes, releasing any reservations.
pub fn abort_current(world: &World, e: Entity) {
    let cur = world.get_mut::<ActionQueue>(e).and_then(|mut q| q.current.take());
    if let Some(cur) = cur {
        cleanup(world, e, &cur);
    }
}

fn cleanup(world: &World, e: Entity, cur: &CurrentAction) {
    match &cur.action.kind {
        ActionKind::Object { obj, .. } => {
            release_slot(world, *obj, e);
            // step off whatever we were lying/sitting on
            if let Phase::Performing { slot: Some(s), .. } = cur.phase {
                stand_up(world, e, *obj, s);
            }
            if let Some((seat, s)) = cur.seat {
                release_slot(world, seat, e);
                if matches!(cur.phase, Phase::Performing { .. }) {
                    stand_up(world, e, seat, s);
                }
            }
        }
        ActionKind::Social { target, .. } => {
            let responding = world.get::<ActionQueue>(*target).is_some_and(|q| {
                q.current.as_ref().is_some_and(|c| matches!(c.action.kind, ActionKind::Respond { initiator, .. } if initiator == e))
            });
            if responding {
                world.get_mut::<ActionQueue>(*target).unwrap().current = None;
                end_posture(world, *target);
            }
        }
        _ => {}
    }
    end_posture(world, e);
}

fn end_posture(world: &World, e: Entity) {
    if let Some(mut p) = world.get_mut::<Posture>(e) {
        *p = Posture::default();
    }
    if let Some(mut b) = world.get_mut::<Brain>(e) {
        b.next_think = 0;
    }
}

fn stand_up(world: &World, e: Entity, obj: Entity, slot: usize) {
    let data = data(world);
    let Some(o) = world.get::<ObjectInstance>(obj) else {
        // sold out from under them: drop to the floor where they are
        if let Some(mut tf) = world.get_mut::<Transform>(e) {
            tf.pos.y = tf.level() as f32 * crate::lot::LEVEL_HEIGHT;
        }
        return;
    };
    let def = data.object(o.def);
    let t = o.slot_stand_tile(def, slot);
    let level = o.level;
    drop(o);
    if let Some(mut tf) = world.get_mut::<Transform>(e) {
        let lot = world.res::<Lot>();
        if lot.walkable(level, t) {
            tf.pos = lot.ground_center(level, t);
        } else {
            tf.pos.y = level as f32 * crate::lot::LEVEL_HEIGHT;
        }
    }
}

// ------------------------------------------------------------------------------------------------
// player commands

pub fn player_order(world: &mut World, e: Entity, kind: ActionKind) -> Result<(), String> {
    let data = data(world);
    if world.has::<Dead>(e) {
        return Err("They are dead. They are not taking requests.".into());
    }
    let stage = world.get::<Age>(e).ok_or("not a peon")?.stage;
    let pers = world.get::<Personality>(e).unwrap().clone();
    let funds = world.res::<Household>().funds;
    // validity
    let (tags, ads): (Vec<String>, Vec<(Need, f32)>) = match &kind {
        ActionKind::Object { obj, interaction } => {
            let inst = world.get::<ObjectInstance>(*obj).ok_or("that object is gone")?;
            let def = data.object(inst.def);
            let inter = def.interactions.get(*interaction).ok_or("no such interaction")?;
            interaction_check(&data, inter, &inst, stage, &pers, funds)?;
            autonomy::not_a_guest_bed(world, e, inter, stage)?;
            (inter.tags.clone(), crate::data::Need::ALL.iter().map(|n| (*n, inter.ad(*n))).collect())
        }
        ActionKind::Social { target, social } => {
            let sd = &data.socials[*social];
            social_allowed(world, sd, e, *target).map_err(String::from)?;
            if is_away(world, *target) || world.has::<Dead>(*target) {
                return Err("they're not here".into());
            }
            (sd.tags.clone(), sd.rates.iter().map(|(n, r)| (*n, r * sd.minutes as f32 / 60.0)).collect())
        }
        ActionKind::GoTo { .. } => (Vec::new(), Vec::new()),
        _ => return Err("can't order that directly".into()),
    };
    if !stage.can_walk() && !matches!(kind, ActionKind::Object { .. }) {
        return Err("Babies do not take orders. Babies give them.".into());
    }

    // refusal
    let needs = world.get::<Needs>(e).unwrap().clone();
    let mood = world.get::<Mood>(e).unwrap().value;
    let (lowest, lv) = needs.lowest_core();
    let helps_lowest = ads.iter().any(|(n, a)| *n == lowest && *a > 5.0);
    let clash = tag_multiplier(&data, &pers, &tags);
    let label = label_for(world, &data, &kind);
    let refuse_reason = {
        let mut rng = world.res_mut::<SimRng>();
        if lv < 10.0 && !helps_lowest && rng.chance(0.6) {
            Some(format!("{} refuses to {}. Their {} is a bit more pressing.", name_of(world, e), label.to_lowercase(), lowest.label().to_lowercase()))
        } else if clash < 0.35 && rng.chance((0.45 - mood / 400.0).clamp(0.1, 0.7)) {
            Some(format!("{} looked at you, looked at the idea of '{}', and said no.", name_of(world, e), label))
        } else {
            None
        }
    };
    if let Some(r) = refuse_reason {
        add_moodlet(world, e, "refused_order");
        set_bubble(world, e, Some("no".into()), Some("Nope!".into()), 20);
        log(world, LogKind::Funny, Some(e), r.clone());
        return Err(r);
    }
    push_back(world, e, kind, Source::Player, label);
    if let Some(mut q) = world.get_mut::<ActionQueue>(e)
        && let Some(c) = q.current.as_mut()
            && c.action.source == Source::Autonomy {
                c.cancel = true;
            }
    Ok(())
}

pub fn cancel(world: &mut World, e: Entity, id: u64) {
    if let Some(mut q) = world.get_mut::<ActionQueue>(e) {
        if let Some(c) = q.current.as_mut()
            && c.action.id == id {
                c.cancel = true;
                return;
            }
        q.queue.retain(|a| a.id != id);
    }
}

// ------------------------------------------------------------------------------------------------
// the system

pub fn run_actions(world: &mut World) {
    let data = data(world);
    let tick = now(world);
    for e in present_peons(world) {
        // an earlier peon's action may have changed us (e.g. pulled into a conversation)
        if !world.contains(e) || world.has::<Dead>(e) || is_away(world, e) {
            continue;
        }
        step(world, e, &data, tick);
    }
}

fn step(world: &mut World, e: Entity, data: &GameData, tick: u64) {
    // pick something to do
    let has_current = world.get::<ActionQueue>(e).unwrap().current.is_some();
    if !has_current {
        let next = world.get_mut::<ActionQueue>(e).unwrap().queue.pop_front();
        let action = match next {
            Some(a) => Some(a),
            None => {
                let due = world.get::<Brain>(e).unwrap().next_think <= tick;
                if !due {
                    None
                } else {
                    let choice = autonomy::choose(world, e, data);
                    let mut brain = world.get_mut::<Brain>(e).unwrap();
                    brain.next_think = tick + 5;
                    drop(brain);
                    match choice {
                        Some((kind, label)) => Some(QueuedAction { id: next_id(world), kind, source: Source::Autonomy, label }),
                        None if world.get::<Brain>(e).unwrap().free_will != Some(crate::time::FreeWill::Off)
                            && world.res::<crate::time::Settings>().free_will != crate::time::FreeWill::Off =>
                        {
                            let minutes = 10 + world.res_mut::<SimRng>().below(20) as u32;
                            Some(QueuedAction { id: next_id(world), kind: ActionKind::Idle { minutes }, source: Source::Autonomy, label: "Idle".into() })
                        }
                        None => None,
                    }
                }
            }
        };
        let Some(action) = action else { return };
        world.get_mut::<ActionQueue>(e).unwrap().current =
            Some(CurrentAction { action, phase: Phase::Start, started: tick, cancel: false, seat: None });
    }

    let mut cur = world.get_mut::<ActionQueue>(e).unwrap().current.take().unwrap();

    // interruptions
    if !cur.cancel && cur.action.source == Source::Autonomy {
        let player_waiting = world.get::<ActionQueue>(e).unwrap().queue.front().is_some_and(|a| a.source == Source::Player);
        let critical = {
            // Only physical needs interrupt, and only if the current action helps none of the
            // critical ones (otherwise multiple low needs make peons thrash between actions).
            let crit = world.get::<Needs>(e).unwrap().critical_physical(10.0);
            let rates = {
                let tmp = ActionQueue { current: Some(cur.clone()), queue: Default::default() };
                active_rates(world, data, &tmp)
            };
            // babies can't fix their own needs, so interrupting them just causes thrashing
            let can_self_help = world.get::<Age>(e).is_some_and(|a| a.stage > LifeStage::Toddler);
            let unfixable = world.get::<Brain>(e).and_then(|b| b.unfixable);
            let fixable: Vec<Need> = crit.into_iter().filter(|n| Some(*n) != unfixable).collect();
            // repairing the broken toilet *is* how you fix a critical bladder
            let is_repair = match &cur.action.kind {
                ActionKind::Object { obj, interaction } => world
                    .get::<ObjectInstance>(*obj)
                    .is_some_and(|o| data.object(o.def).interactions[*interaction].repairs),
                _ => false,
            };
            can_self_help
                && !is_repair
                && !fixable.is_empty()
                && fixable.iter().all(|n| rates[n.idx()] <= 0.0)
                && matches!(cur.phase, Phase::Performing { .. })
                && !matches!(cur.action.kind, ActionKind::Respond { .. })
        };
        // a critical need nothing could fix earlier (the fridge was taken): check back now and then
        let fixable_again = !critical
            && tick.is_multiple_of(15)
            && matches!(cur.phase, Phase::Performing { .. })
            && world.get::<Brain>(e).and_then(|b| b.unfixable).is_some_and(|n| {
                world.get::<Needs>(e).is_some_and(|nd| nd.get(n) < 15.0) && autonomy::candidates(world, e, data).iter().any(|c| c.helps.contains(&n))
            });
        if player_waiting || critical || fixable_again {
            cur.cancel = true;
        }
    }

    let result = match cur.phase.clone() {
        Phase::Start => match begin(world, e, data, &mut cur) {
            Ok(()) => StepResult::Continue,
            Err(why) => StepResult::Failed(why),
        },
        Phase::Walking { .. } if cur.cancel => StepResult::Done(false),
        Phase::Walking { mut path, mut idx, slot, retries, mut blocked } => {
            let stage = world.get::<Age>(e).unwrap().stage;
            // crowding: wait for, route around, or sidestep other peons
            let ignore = match &cur.action.kind {
                ActionKind::Social { target, .. } => Some(*target),
                _ => None,
            };
            let mut wait = false;
            let speed = walk_speed(stage);
            match crowd_ahead(world, e, &path, idx, ignore, speed) {
                Some(Crowd::Standing(other)) => {
                    blocked = blocked.saturating_add(1);
                    if (blocked == 2 || blocked == 8)
                        && let Some(p) = reroute_around(world, e, &path, idx, other) {
                            path = p;
                            idx = 0;
                        }
                    // after a while, just squeeze past ("excuse me, sorry, excuse me")
                    wait = blocked < 15 && crowd_ahead(world, e, &path, idx, ignore, speed).is_some();
                    if blocked == 6 {
                        set_bubble(world, e, Some("...".into()), Some("'Scuse me".into()), 5);
                    }
                }
                Some(Crowd::Oncoming) => {
                    sidestep(world, e, &mut path, idx);
                    blocked = 0;
                }
                None => blocked = 0,
            }
            let arrived = if wait {
                false
            } else {
                let mut tf = world.get_mut::<Transform>(e).unwrap();
                advance_along(&mut tf, &path, &mut idx, walk_speed(stage))
            };
            if let Some(mut p) = world.get_mut::<Posture>(e) {
                p.walking = !arrived && !wait;
                p.pose = Pose::Stand;
                p.anim = None;
                p.prop = None;
            }
            if arrived {
                match arrive(world, e, data, &mut cur, slot, retries) {
                    Ok(true) => StepResult::Continue,
                    Ok(false) => StepResult::Done(true),
                    Err(why) => StepResult::Failed(why),
                }
            } else {
                cur.phase = Phase::Walking { path, idx, slot, retries, blocked };
                StepResult::Continue
            }
        }
        Phase::Performing { slot, elapsed } => {
            if cur.cancel {
                StepResult::Done(false)
            } else {
                match perform(world, e, data, &cur, slot, elapsed + 1) {
                    Ok(true) => StepResult::Done(true),
                    Ok(false) => {
                        cur.phase = Phase::Performing { slot, elapsed: elapsed + 1 };
                        StepResult::Continue
                    }
                    Err(why) => StepResult::Failed(why),
                }
            }
        }
    };

    if world.res::<crate::time::Settings>().trace_actions {
        let ev = match &result {
            StepResult::Continue if matches!(cur.phase, Phase::Performing { elapsed: 0, .. }) => Some("begin".to_string()),
            StepResult::Continue => None,
            StepResult::Done(true) => Some("done".into()),
            StepResult::Done(false) => Some(format!("cancelled{}", if cur.cancel { "" } else { "?" })),
            StepResult::Failed(w) => Some(format!("FAILED: {w}")),
        };
        if let Some(ev) = ev {
            let n = world.get::<Needs>(e).unwrap();
            let ns: Vec<String> = n.active_needs().map(|k| format!("{}{:.0}", &k.label()[..3], n.get(k))).collect();
            let t = format!("{} [{:?}] {} — {} | {}", name_of(world, e), cur.action.source, cur.action.label, ev, ns.join(" "));
            log(world, LogKind::Debug, Some(e), t);
        }
    }
    match result {
        StepResult::Continue => world.get_mut::<ActionQueue>(e).unwrap().current = Some(cur),
        StepResult::Done(completed) => finish(world, e, data, &cur, completed),
        StepResult::Failed(why) => {
            cleanup(world, e, &cur);
            if cur.action.source == Source::Player {
                set_bubble(world, e, Some("?".into()), Some("Hmm?".into()), 15);
                log(world, LogKind::Info, Some(e), format!("{} couldn't {}: {}.", name_of(world, e), cur.action.label.to_lowercase(), why));
            }
            if let Some(mut b) = world.get_mut::<Brain>(e) {
                b.next_think = tick + 2;
            }
        }
    }
}

enum StepResult {
    Continue,
    Done(bool),
    Failed(String),
}

enum Crowd {
    /// Someone standing still in the way.
    Standing(Entity),
    /// Someone walking toward us.
    Oncoming,
}

/// How close (sideways) someone may be to our line of travel before they're "in the way".
const PERSONAL_SPACE: f32 = 0.55;

/// Is another peon in front of us, within this tick's stride (plus a margin)?
fn crowd_ahead(world: &World, e: Entity, path: &[Vec3], idx: usize, ignore: Option<Entity>, speed: f32) -> Option<Crowd> {
    let tf = *world.get::<Transform>(e)?;
    let next = *path.get(idx)?;
    let dir = next - tf.pos;
    let dir = Vec3::new(dir.x, 0.0, dir.z);
    if dir.length() < 1e-3 {
        return None;
    }
    let dir = dir.normalize();
    let goal = *path.last()?;
    let mut found: Option<(f32, Entity, Crowd)> = None;
    for (o, otf, posture) in world
        .query_filtered::<(Entity, &Transform, &Posture), (hearth_ecs::Without<Dead>, hearth_ecs::Without<AtWork>, hearth_ecs::Without<Offlot>)>()
        .iter()
    {
        if o == e || Some(o) == ignore || (otf.pos.y - tf.pos.y).abs() > 1.0 {
            continue;
        }
        let to = otf.pos - tf.pos;
        let to = Vec3::new(to.x, 0.0, to.z);
        let along = to.dot(dir);
        let lateral = (to - dir * along).length();
        let reach = if posture.walking { speed * 2.0 + 0.6 } else { speed + 0.6 };
        if along <= 0.05 || along > reach || lateral > PERSONAL_SPACE {
            continue;
        }
        // nobody gets out of the way of someone standing on the spot we're walking to
        if (otf.pos - goal).length() < 0.4 && !posture.walking {
            continue;
        }
        let hit = if posture.walking {
            let theirs = Vec3::new(otf.yaw.sin(), 0.0, otf.yaw.cos());
            if theirs.dot(dir) >= -0.3 {
                continue;
            }
            Crowd::Oncoming
        } else {
            Crowd::Standing(o)
        };
        // nearest first; entity order breaks ties so storage order never matters
        if found.as_ref().is_none_or(|f| along < f.0 || (along == f.0 && o < f.1)) {
            found = Some((along, o, hit));
        }
    }
    found.map(|f| f.2)
}

/// Replans the rest of the path treating `other`'s tile as blocked. Accepts small detours only.
fn reroute_around(world: &World, e: Entity, path: &[Vec3], idx: usize, other: Entity) -> Option<Vec<Vec3>> {
    let tf = *world.get::<Transform>(e)?;
    let otf = *world.get::<Transform>(other)?;
    let goal = *path.last()?;
    let goal_node = NavNode::new(Lot::level_of(goal), Lot::tile_of(goal));
    let avoid = NavNode::new(otf.level(), otf.tile());
    if avoid == goal_node || avoid == NavNode::new(tf.level(), tf.tile()) {
        return None;
    }
    let remaining: f32 = std::iter::once(tf.pos).chain(path[idx..].iter().copied()).collect::<Vec<_>>().windows(2).map(|w| (w[1] - w[0]).length()).sum();
    let lot = world.res::<Lot>();
    let field = NavField::flood_avoiding(&lot, NavNode::new(tf.level(), tf.tile()), remaining + 8.0, &[avoid]);
    let d = field.distance(goal_node)?;
    if d > remaining + 6.0 {
        return None;
    }
    let nodes = field.path_to(goal_node)?;
    let mut wp = waypoints(&lot, &nodes);
    if nodes.len() > 1 {
        wp.remove(0);
    }
    // keep the exact final position (slot approach points may be off-center)
    if let Some(last) = wp.last_mut() {
        *last = goal;
    }
    Some(wp)
}

/// Keep to the right when passing someone: shift our position and the next couple of waypoints
/// sideways, staying well inside each tile so walls are never crossed.
fn sidestep(world: &World, e: Entity, path: &mut [Vec3], idx: usize) {
    let Some(next) = path.get(idx).copied() else { return };
    let Some(mut tf) = world.get_mut::<Transform>(e) else { return };
    let dir = Vec3::new(next.x - tf.pos.x, 0.0, next.z - tf.pos.z);
    if dir.length() < 1e-3 {
        return;
    }
    let right = Vec3::new(-dir.z, 0.0, dir.x).normalize() * 0.3;
    let shift = |p: Vec3| {
        let t = Lot::tile_of(p);
        let q = p + right;
        Vec3::new(q.x.clamp(t.x as f32 + 0.2, t.x as f32 + 0.8), p.y, q.z.clamp(t.y as f32 + 0.2, t.y as f32 + 0.8))
    };
    tf.pos = shift(tf.pos);
    let last = path.len().saturating_sub(1);
    let y = tf.pos.y;
    let end = (idx + 3).min(last).max(idx);
    for wp in &mut path[idx..end] {
        // leave stair waypoints and the final destination alone
        if (wp.y - y).abs() < 0.05 {
            *wp = shift(*wp);
        }
    }
}

/// Moves along waypoints; returns true when the end is reached.
fn advance_along(tf: &mut Transform, path: &[Vec3], idx: &mut usize, speed: f32) -> bool {
    let mut budget = speed;
    while *idx < path.len() {
        let target = path[*idx];
        let d = target - tf.pos;
        let len = d.length();
        if len > 1e-4 {
            tf.yaw = d.x.atan2(d.z);
        }
        if len <= budget {
            tf.pos = target;
            budget -= len;
            *idx += 1;
        } else {
            tf.pos += d / len * budget;
            return false;
        }
    }
    true
}

fn path_to(world: &World, e: Entity, goal: NavNode) -> Option<Vec<Vec3>> {
    let tf = *world.get::<Transform>(e)?;
    let lot = world.res::<Lot>();
    let field = NavField::flood(&lot, NavNode::new(tf.level(), tf.tile()), 400.0);
    let nodes = field.path_to(goal)?;
    let mut wp = waypoints(&lot, &nodes);
    // start from the current exact position, not the tile center
    if !wp.is_empty() && nodes.len() > 1 {
        wp.remove(0);
    }
    Some(wp)
}

fn begin(world: &World, e: Entity, data: &GameData, cur: &mut CurrentAction) -> Result<(), String> {
    match cur.action.kind.clone() {
        ActionKind::Object { obj, interaction } => {
            let stage = world.get::<Age>(e).unwrap().stage;
            let pers = world.get::<Personality>(e).unwrap().clone();
            let funds = world.res::<Household>().funds;
            let tf = *world.get::<Transform>(e).unwrap();
            let field = NavField::flood(&world.res::<Lot>(), NavNode::new(tf.level(), tf.tile()), 400.0);
            let seat = {
                let inst = world.get::<ObjectInstance>(obj).ok_or("it's gone")?;
                let def = data.object(inst.def);
                let inter = &def.interactions[interaction];
                interaction_check(data, inter, &inst, stage, &pers, funds)?;
                autonomy::not_a_guest_bed(world, e, inter, stage)?;
                inter.seat.as_ref().and_then(|sd| autonomy::nearest_seat(world, data, e, obj, &inst, sd, &field))
            };
            if let Some((seat, slot, node)) = seat {
                let nodes = field.path_to(node).unwrap();
                let mut path = waypoints(&world.res::<Lot>(), &nodes);
                if nodes.len() > 1 {
                    path.remove(0);
                }
                world.get_mut::<ObjectInstance>(seat).unwrap().slot_users[slot] = Some(e);
                cur.seat = Some((seat, slot));
                cur.phase = Phase::Walking { path, idx: 0, slot: None, retries: 0, blocked: 0 };
                return Ok(());
            }
            let mut inst = world.get_mut::<ObjectInstance>(obj).ok_or("it's gone")?;
            let def = data.object(inst.def);
            let inter = &def.interactions[interaction];
            let slots = free_slots(def, inter, &inst, e);
            if slots.is_empty() {
                return Err("someone else is using it".into());
            }
            let lot = world.res::<Lot>();
            let best = slots
                .iter()
                .filter_map(|&s| {
                    let n = NavNode::new(inst.level, inst.slot_stand_tile(def, s));
                    field.distance(n).map(|d| (d, s, n))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, slot, node)) = best else { return Err("can't get there".into()) };
            let nodes = field.path_to(node).unwrap();
            let mut path = waypoints(&lot, &nodes);
            if nodes.len() > 1 {
                path.remove(0);
            }
            inst.slot_users[slot] = Some(e);
            cur.phase = Phase::Walking { path, idx: 0, slot: Some(slot), retries: 0, blocked: 0 };
            Ok(())
        }
        ActionKind::Social { target, .. } => {
            if !world.contains(target) || is_away(world, target) || world.has::<Dead>(target) {
                return Err("they're not around".into());
            }
            let path = path_next_to(world, e, target).ok_or("can't reach them")?;
            cur.phase = Phase::Walking { path, idx: 0, slot: None, retries: 0, blocked: 0 };
            Ok(())
        }
        ActionKind::Respond { .. } | ActionKind::PassOut => {
            cur.phase = Phase::Performing { slot: None, elapsed: 0 };
            if matches!(cur.action.kind, ActionKind::PassOut) {
                world.get_mut::<Posture>(e).unwrap().pose = Pose::Lie;
                set_bubble(world, e, Some("Zzz".into()), None, 240);
            }
            Ok(())
        }
        ActionKind::GoTo { level, tile } => {
            let path = path_to(world, e, NavNode::new(level, tile)).ok_or("can't get there")?;
            cur.phase = Phase::Walking { path, idx: 0, slot: None, retries: 0, blocked: 0 };
            Ok(())
        }
        ActionKind::Walk { path } => {
            cur.phase = Phase::Walking { path: path.clone(), idx: 0, slot: None, retries: 0, blocked: 0 };
            Ok(())
        }
        ActionKind::GoToWork | ActionKind::GoToSchool => {
            let exit = exit_tile(&world.res::<Lot>());
            let path = path_to(world, e, NavNode::new(0, exit)).unwrap_or_default();
            cur.phase = Phase::Walking { path, idx: 0, slot: None, retries: 0, blocked: 0 };
            let icon = if matches!(cur.action.kind, ActionKind::GoToWork) { "work" } else { "school" };
            set_bubble(world, e, Some(icon.into()), None, 30);
            Ok(())
        }
        ActionKind::Idle { .. } => {
            let tf = *world.get::<Transform>(e).unwrap();
            let lot = world.res::<Lot>();
            let field = NavField::flood(&lot, NavNode::new(tf.level(), tf.tile()), 6.0);
            let opts = field.tiles_within(5.0);
            drop(lot);
            let wander = world.res_mut::<SimRng>().chance(0.6);
            let goal = if wander { world.res_mut::<SimRng>().pick(&opts).copied() } else { None };
            let path = goal.and_then(|g| path_to(world, e, g)).unwrap_or_default();
            cur.phase = Phase::Walking { path, idx: 0, slot: None, retries: 0, blocked: 0 };
            Ok(())
        }
    }
}

/// Where peons leave the lot (the street side).
pub fn exit_tile(lot: &Lot) -> IVec2 {
    IVec2::new(lot.width / 2, lot.depth - 1)
}

fn path_next_to(world: &World, e: Entity, target: Entity) -> Option<Vec<Vec3>> {
    let tf = *world.get::<Transform>(e)?;
    let ttf = *world.get::<Transform>(target)?;
    let lot = world.res::<Lot>();
    let field = NavField::flood(&lot, NavNode::new(tf.level(), tf.tile()), 400.0);
    let tl = ttf.level();
    let tt = ttf.tile();
    let mut best: Option<(f32, NavNode)> = None;
    for dx in -1..=1 {
        for dz in -1..=1 {
            if (dx, dz) == (0, 0) {
                continue;
            }
            let n = NavNode::new(tl, tt + IVec2::new(dx, dz));
            // must be able to step to the target from there (no wall between)
            if !lot.can_step(tl, n.tile, tt) && lot.walkable(tl, tt) {
                continue;
            }
            if let Some(d) = field.distance(n)
                && best.is_none_or(|b| d < b.0) {
                    best = Some((d, n));
                }
        }
    }
    let (_, goal) = best?;
    let nodes = field.path_to(goal)?;
    let mut wp = waypoints(&lot, &nodes);
    if nodes.len() > 1 {
        wp.remove(0);
    }
    Some(wp)
}

fn face(world: &World, a: Entity, b: Entity) {
    let (Some(pa), Some(pb)) = (world.get::<Transform>(a).map(|t| t.pos), world.get::<Transform>(b).map(|t| t.pos)) else { return };
    let d = pb - pa;
    if let Some(mut t) = world.get_mut::<Transform>(a) {
        t.yaw = d.x.atan2(d.z);
    }
}

/// Reached the destination. Ok(true) = continue performing, Ok(false) = action complete.
fn arrive(world: &World, e: Entity, data: &GameData, cur: &mut CurrentAction, slot: Option<usize>, retries: u8) -> Result<bool, String> {
    let tick = now(world);
    match cur.action.kind.clone() {
        ActionKind::Object { obj, interaction } => {
            let inter = {
                let inst = world.get::<ObjectInstance>(obj).ok_or("it's gone")?;
                data.object(inst.def).interactions[interaction].clone()
            };
            // performing from a seat (see `SeatDef`) or from one of the object's own slots
            let (spot, slot) = match (cur.seat, slot) {
                (Some((seat, s)), _) => ((seat, s), None),
                (None, Some(s)) => ((obj, s), Some(s)),
                (None, None) => return Err("no slot".into()),
            };
            let (pos, yaw) = {
                let inst = world.get::<ObjectInstance>(spot.0).ok_or("it's gone")?;
                let def = data.object(inst.def);
                (inst.slot_use_pos(def, spot.1), inst.slot_yaw(def, spot.1))
            };
            let (pose, anim) = match (&cur.seat, &inter.seat) {
                (Some(_), Some(sd)) => (Pose::Sit, sd.anim.clone()),
                _ => (inter.pose, inter.anim.clone()),
            };
            if inter.cost > 0 && !crate::economy::debit(world, e, inter.cost) {
                return Err("can't afford it".into());
            }
            {
                let mut tf = world.get_mut::<Transform>(e).unwrap();
                tf.pos = pos;
                tf.yaw = yaw;
            }
            *world.get_mut::<Posture>(e).unwrap() = Posture::performing(pose, anim, inter.prop.clone());
            if let Some(b) = &inter.bubble {
                set_bubble(world, e, Some(b.clone()), None, inter.nominal_minutes().min(60) as u64);
            }
            cur.phase = Phase::Performing { slot, elapsed: 0 };
            Ok(true)
        }
        ActionKind::Social { target, social } => {
            let sd = data.socials[social].clone();
            let (p, tp) = (world.get::<Transform>(e).unwrap().pos, world.get::<Transform>(target).map(|t| t.pos));
            let Some(tp) = tp else { return Err("they left".into()) };
            let close = (p - tp).length() <= 1.9 && (p.y - tp.y).abs() < 1.0;
            if !close {
                if retries >= 3 {
                    return Err("they keep wandering off".into());
                }
                let path = path_next_to(world, e, target).ok_or("can't reach them")?;
                cur.phase = Phase::Walking { path, idx: 0, slot: None, retries: retries + 1, blocked: 0 };
                return Ok(true);
            }
            face(world, e, target);
            // target availability
            let target_busy = world.get::<ActionQueue>(target).is_some_and(|q| {
                q.current.as_ref().is_some_and(|c| {
                    c.action.source == Source::Player
                        || matches!(c.action.kind, ActionKind::Respond { .. } | ActionKind::Social { .. } | ActionKind::GoToWork | ActionKind::GoToSchool | ActionKind::PassOut)
                        || (!sd.care && autonomy::is_sleeping(world, data, c))
                })
            }) || is_away(world, target);
            if target_busy {
                set_bubble(world, e, Some("...".into()), None, 10);
                return Err(format!("{} is busy", name_of(world, target)));
            }
            let p_accept = accept_chance(world, &sd, e, target);
            let accepted = world.res_mut::<SimRng>().chance(p_accept);
            if !accepted {
                world.res_mut::<Relationships>().add(e, target, -3.0, 0.0);
                say(world, target, Some("no"), 10);
                set_bubble(world, e, Some(":(".into()), None, 15);
                if sd.romantic {
                    add_moodlet(world, e, "rejected");
                    let t = format!("{} tried to {} {}. It did not go well.", name_of(world, e), sd.name.to_lowercase(), name_of(world, target));
                    log(world, LogKind::Funny, Some(e), t);
                }
                return Ok(false);
            }
            // pull the target into the conversation
            abort_current(world, target);
            face(world, target, e);
            let id = next_id(world);
            world.get_mut::<ActionQueue>(target).unwrap().current = Some(CurrentAction {
                action: QueuedAction {
                    id,
                    kind: ActionKind::Respond { initiator: e, social, minutes: sd.minutes },
                    source: Source::System,
                    label: format!("{} with {}", sd.name, name_of(world, e)),
                },
                phase: Phase::Performing { slot: None, elapsed: 0 },
                started: tick,
                cancel: false,
                seat: None,
            });
            say(world, e, sd.bubble.as_deref(), sd.minutes.min(20) as u64);
            *world.get_mut::<Posture>(e).unwrap() = Posture::performing(Pose::Stand, sd.anim.clone(), sd.prop.clone());
            if let Some(mut p) = world.get_mut::<Posture>(target) {
                *p = Posture::performing(Pose::Stand, sd.target_anim.clone(), None);
            }
            cur.phase = Phase::Performing { slot: None, elapsed: 0 };
            Ok(true)
        }
        ActionKind::GoToWork => {
            crate::career::begin_shift(world, e);
            Ok(false)
        }
        ActionKind::GoToSchool => {
            crate::school::begin_school(world, e);
            Ok(false)
        }
        ActionKind::Idle { .. } => {
            cur.phase = Phase::Performing { slot: None, elapsed: 0 };
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// One minute of performing. Ok(true) when finished.
fn perform(world: &World, e: Entity, data: &GameData, cur: &CurrentAction, slot: Option<usize>, elapsed: u32) -> Result<bool, String> {
    let tick = now(world);
    match &cur.action.kind {
        ActionKind::Object { obj, interaction } => {
            let inter = {
                let inst = world.get::<ObjectInstance>(*obj).ok_or("it vanished")?;
                let seat_lost = cur.seat.is_some_and(|(seat, s)| {
                    world.get::<ObjectInstance>(seat).is_none_or(|i| i.slot_users.get(s).copied().flatten() != Some(e))
                });
                if seat_lost || slot.is_some_and(|s| inst.slot_users.get(s).copied().flatten() != Some(e)) {
                    return Err("lost their spot".into());
                }
                data.object(inst.def).interactions[*interaction].clone()
            };
            {
                let mut needs = world.get_mut::<Needs>(e).unwrap();
                for (n, r) in &inter.rates {
                    if needs.is_active(*n) {
                        needs.add(*n, r / 60.0);
                    }
                }
            }
            if let Some((s, r)) = inter.skill {
                let leveled = world.get_mut::<Skills>(e).unwrap().add(s, r / 60.0 / 10.0);
                if leveled {
                    let lvl = world.get::<Skills>(e).unwrap().get(s).floor();
                    log(world, LogKind::Info, Some(e), format!("{} reached {:?} level {}!", name_of(world, e), s, lvl));
                }
            }
            Ok(match inter.duration {
                DurationDef::Minutes(m) => elapsed >= m,
                DurationDef::UntilFull(n, max) => world.get::<Needs>(e).unwrap().get(n) >= 99.0 || elapsed >= max,
            })
        }
        ActionKind::Social { target, social } => {
            let sd = &data.socials[*social];
            let responding = world.get::<ActionQueue>(*target).is_some_and(|q| {
                q.current.as_ref().is_some_and(|c| matches!(c.action.kind, ActionKind::Respond { initiator, .. } if initiator == e))
            });
            if !responding {
                return Err("they walked away".into());
            }
            {
                let mut n = world.get_mut::<Needs>(e).unwrap();
                for (k, r) in &sd.rates {
                    if n.is_active(*k) {
                        n.add(*k, r / 60.0);
                    }
                }
            }
            {
                let mut n = world.get_mut::<Needs>(*target).unwrap();
                for (k, r) in &sd.target_rates {
                    if n.is_active(*k) {
                        n.add(*k, r / 60.0);
                    }
                }
            }
            if elapsed.is_multiple_of(8) {
                let who = if (elapsed / 8).is_multiple_of(2) { *target } else { e };
                say(world, who, sd.bubble.as_deref(), 6);
            }
            if sd.tags.iter().any(|t| t == "playful" || t == "social") && elapsed.is_multiple_of(30) {
                world.get_mut::<Skills>(e).unwrap().add(crate::data::Skill::Charisma, 0.05);
            }
            Ok(elapsed >= sd.minutes)
        }
        ActionKind::Respond { initiator, minutes, .. } => {
            let still = world.get::<ActionQueue>(*initiator).is_some_and(|q| {
                q.current.as_ref().is_some_and(|c| matches!(c.action.kind, ActionKind::Social { target, .. } if target == e))
            });
            Ok(!still || elapsed > minutes + 30)
        }
        ActionKind::PassOut => {
            let mut n = world.get_mut::<Needs>(e).unwrap();
            n.add(Need::Energy, 30.0 / 60.0);
            Ok(n.get(Need::Energy) >= 55.0 || elapsed >= 300)
        }
        ActionKind::Idle { minutes } => {
            if elapsed % 12 == 6 && world.res_mut::<SimRng>().chance(0.35) {
                let quirk = world.get::<Personality>(e).unwrap().quirk.clone();
                if let Some(q) = data.quirk(&quirk) {
                    let m = world.res_mut::<SimRng>().pick(&q.mutters).cloned();
                    set_bubble(world, e, m, None, 6);
                }
            }
            let _ = tick;
            Ok(elapsed >= *minutes)
        }
        _ => Ok(true),
    }
}

fn finish(world: &World, e: Entity, data: &GameData, cur: &CurrentAction, completed: bool) {
    if completed {
        let t = now(world);
        let label = match &cur.action.kind {
            ActionKind::Object { obj, interaction } => world.get::<ObjectInstance>(*obj).map(|o| {
                let def = data.object(o.def);
                format!("{} ({})", def.interactions[*interaction].name, def.name)
            }),
            ActionKind::Social { target, social } => Some(format!("{} {}", data.socials[*social].name, name_of(world, *target))),
            _ => None,
        };
        if let (Some(label), Some(mut b)) = (label, world.get_mut::<Brain>(e)) {
            b.recent.retain(|(_, at)| t.saturating_sub(*at) < 240);
            b.recent.push((label, t));
        }
    }
    match &cur.action.kind {
        ActionKind::Object { obj, interaction } if completed => {
            let Some(inst) = world.get::<ObjectInstance>(*obj).map(|i| i.clone()) else {
                cleanup(world, e, cur);
                return;
            };
            let def = data.object(inst.def);
            let inter = &def.interactions[*interaction];
            for out in &inter.outcomes {
                match out {
                    Outcome::Money { chance, min, max } => {
                        let hit = world.res_mut::<SimRng>().chance(*chance);
                        if hit {
                            let amt = world.res_mut::<SimRng>().range_i64(*min, *max);
                            world.res_mut::<Household>().funds += amt;
                            if amt > 0 {
                                log(world, LogKind::Money, Some(e), format!("{} found ${} while doing '{}'.", name_of(world, e), amt, inter.name));
                            } else {
                                log(world, LogKind::Money, Some(e), format!("{} spent ${} on '{}'.", name_of(world, e), -amt, inter.name));
                            }
                        }
                    }
                    Outcome::Moodlet { chance, id } => {
                        if world.res_mut::<SimRng>().chance(*chance) {
                            add_moodlet(world, e, id);
                        }
                    }
                    Outcome::Log { chance, text } => {
                        if world.res_mut::<SimRng>().chance(*chance) {
                            let t = fill(world, e, text);
                            log(world, LogKind::Funny, Some(e), t);
                        }
                    }
                    Outcome::Dirty { amount } => {
                        if let Some(mut o) = world.get_mut::<ObjectInstance>(*obj) {
                            o.dirt = (o.dirt + amount).min(1.0);
                        }
                    }
                    Outcome::Break { chance } => {
                        if world.res_mut::<SimRng>().chance(*chance) {
                            world.get_mut::<ObjectInstance>(*obj).unwrap().broken = true;
                        }
                    }
                }
            }
            let clumsy = world.get::<Personality>(e).is_some_and(|p| p.has("clumsy"));
            let broke = world.res_mut::<SimRng>().chance(def.break_chance * if clumsy { 3.0 } else { 1.0 });
            {
                let mut o = world.get_mut::<ObjectInstance>(*obj).unwrap();
                o.uses += 1;
                o.dirt = (o.dirt + def.dirt_per_use).min(1.0);
                if inter.cleans {
                    o.dirt = 0.0;
                }
                if inter.repairs {
                    o.broken = false;
                }
                if broke && !o.broken && !inter.repairs {
                    o.broken = true;
                }
            }
            if broke && !inter.repairs {
                log(world, LogKind::Funny, Some(e), format!("The {} broke! {} is looking at it accusingly.", def.name, name_of(world, e)));
            }
            if inter.repairs {
                log(world, LogKind::Info, Some(e), format!("{} fixed the {}.", name_of(world, e), def.name));
            }
            if inter.has_tag("homework") {
                crate::school::homework_done(world, e);
            }
        }
        ActionKind::Social { target, social } if completed => {
            let sd = &data.socials[*social];
            let before = world.res::<Relationships>().get(e, *target).label();
            world.res_mut::<Relationships>().add(e, *target, sd.friendship, sd.romance);
            let after = world.res::<Relationships>().get(e, *target).label();
            match sd.id.as_str() {
                "hug" => {
                    add_moodlet(world, e, "hugged");
                    add_moodlet(world, *target, "hugged");
                }
                "kiss" => {
                    add_moodlet(world, e, "kissed");
                    add_moodlet(world, *target, "kissed");
                }
                "insult" => {
                    add_moodlet(world, *target, "insulted");
                    log(world, LogKind::Social, Some(e), format!("{} insulted {}. It was about their shoes.", name_of(world, e), name_of(world, *target)));
                }
                "try_for_baby" => {
                    let (a, b) = (e, *target);
                    world.defer(move |w| crate::family::try_for_baby(w, a, b));
                }
                "chat" | "deep_talk" | "joke"
                    if world.res_mut::<SimRng>().chance(0.3) => {
                        add_moodlet(world, e, "good_chat");
                    }
                _ => {}
            }
            if before != after {
                log(world, LogKind::Social, Some(e), format!("{} and {} are now: {}.", name_of(world, e), name_of(world, *target), after));
            }
        }
        _ => {}
    }
    cleanup(world, e, cur);
}
