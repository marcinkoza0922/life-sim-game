//! Utility AI. Objects and other peons *advertise* need gains; peons score them against their
//! needs, personality and distance, then pick among the best with a little randomness.

use hearth_ecs::{Entity, World};

use crate::components::*;
use crate::data::{GameData, InteractionDef, LifeStage, Need, SocialDef};
use crate::economy::Household;
use crate::lot::Lot;
use crate::nav::{NavField, NavNode};
use crate::objects::ObjectInstance;
use crate::rng::SimRng;
use crate::social::{accept_chance_with, social_allowed, social_allowed_with};
use crate::time::{Clock, FreeWill, Settings};

/// How badly a need value wants raising (0 at full, ~3.4 at empty).
pub fn urgency(v: f32) -> f32 {
    let u = ((100.0 - v) / 100.0).clamp(0.0, 1.0);
    u * u * 3.0 + u * 0.4
}

/// Silly/personality needs never dominate.
pub fn need_weight(n: Need) -> f32 {
    if n.is_core() { 1.0 } else { 0.35 }
}

/// Product of trait and quirk multipliers for the given tags.
pub fn tag_multiplier(data: &GameData, pers: &Personality, tags: &[String]) -> f32 {
    let mut m = 1.0;
    for tag in tags {
        for t in &pers.traits {
            if let Some(x) = data.trait_def(t).and_then(|d| d.tags.get(tag)) {
                m *= x;
            }
        }
        if let Some(x) = data.quirk(&pers.quirk).and_then(|q| q.tags.get(tag)) {
            m *= x;
        }
        let a = &pers.axes;
        let axis = |v: u8| 0.7 + v as f32 * 0.06; // 0.7 .. 1.3
        m *= match tag.as_str() {
            "neat" | "chore" => axis(a.neat),
            "social" => axis(a.outgoing),
            "exercise" => axis(a.active),
            "lazy" => axis(10 - a.active),
            "fun" | "playful" => axis(a.playful),
            "nice" | "care" => axis(a.nice),
            "mean" => axis(10 - a.nice),
            _ => 1.0,
        };
    }
    m.clamp(0.0, 6.0)
}

/// Can `peon` use `interaction` on this object right now? (Ignores slot availability.)
pub fn interaction_check(
    data: &GameData,
    inter: &InteractionDef,
    inst: &ObjectInstance,
    stage: LifeStage,
    pers: &Personality,
    funds: i64,
) -> Result<(), String> {
    if !inter.allowed_stage(stage) {
        return Err(format!("not for {}s", stage.label().to_lowercase()));
    }
    if let Some(t) = &inter.requires_trait
        && !pers.has(t) {
            return Err(format!("requires {}", data.trait_def(t).map(|d| d.name.as_str()).unwrap_or(t)));
        }
    if inst.broken && !inter.when_broken {
        return Err("it's broken".into());
    }
    if inter.only_broken && !inst.broken {
        return Err("nothing to fix".into());
    }
    if inst.dirt < inter.min_dirt {
        return Err("already clean".into());
    }
    if inter.cost > funds {
        return Err("can't afford".into());
    }
    Ok(())
}

/// Guests don't sleep in their hosts' beds (infants and toddlers can still use a crib).
pub fn not_a_guest_bed(world: &World, e: Entity, inter: &InteractionDef, stage: LifeStage) -> Result<(), String> {
    if stage > LifeStage::Toddler && inter.tags.iter().any(|t| t == "sleep") && crate::neighborhood::is_guest(world, e) {
        return Err("not their bed".into());
    }
    Ok(())
}

pub fn free_slots(def: &crate::data::ObjectDef, inter: &InteractionDef, inst: &ObjectInstance, me: Entity) -> Vec<usize> {
    (0..def.slots.len())
        .filter(|i| inter.slots.is_empty() || inter.slots.contains(i))
        .filter(|&i| inst.slot_users.get(i).copied().flatten().is_none_or(|u| u == me))
        .collect()
}

/// The closest (by walking) free seat slot to perform `obj`'s interaction from (see `SeatDef`), with
/// the nav node to approach it from.
pub fn nearest_seat(world: &World, data: &GameData, e: Entity, obj: Entity, inst: &ObjectInstance, sd: &crate::data::SeatDef, field: &NavField) -> Option<(Entity, usize, NavNode)> {
    nearest_seat_by(world, data, e, obj, inst, sd, field).map(|(_, se, s, n)| (se, s, n))
}

/// [`nearest_seat`] with its walking distance.
fn nearest_seat_by(world: &World, data: &GameData, e: Entity, obj: Entity, inst: &ObjectInstance, sd: &crate::data::SeatDef, field: &NavField) -> Option<(f32, Entity, usize, NavNode)> {
    let target = inst.center(data.object(inst.def));
    let cos_cone = sd.cone.to_radians().cos();
    let mut best: Option<(f32, Entity, usize, NavNode)> = None;
    for (se, si) in world.query::<(Entity, &ObjectInstance)>().iter() {
        if se == obj || si.level != inst.level {
            continue;
        }
        let def = data.object(si.def);
        let seat_slots = def.interactions.iter().filter(|i| i.pose == crate::data::Pose::Sit && i.tags.iter().any(|t| t == "seating"));
        for i in seat_slots {
            for s in free_slots(def, i, si, e) {
                let at = si.slot_use_pos(def, s);
                let to = glam::Vec2::new(target.x - at.x, target.z - at.z);
                let yaw = si.slot_yaw(def, s);
                let fwd = glam::Vec2::new(yaw.sin(), yaw.cos());
                if to.length() > sd.range || to.normalize_or_zero().dot(fwd) < cos_cone {
                    continue;
                }
                let node = NavNode::new(si.level, si.slot_stand_tile(def, s));
                if let Some(d) = field.distance(node)
                    && best.is_none_or(|b| d < b.0)
                {
                    best = Some((d, se, s, node));
                }
            }
        }
    }
    best
}

/// Is there a reachable free seat to perform `inter` on `obj` from (see `SeatDef`)?
fn seat_free(world: &World, data: &GameData, e: Entity, obj: Entity, inst: &ObjectInstance, inter: &InteractionDef) -> bool {
    let Some(sd) = &inter.seat else { return false };
    let Some(tf) = world.get::<Transform>(e).map(|t| *t) else { return false };
    let field = NavField::flood(&world.res::<Lot>(), NavNode::new(tf.level(), tf.tile()), 400.0);
    nearest_seat(world, data, e, obj, inst, sd, &field).is_some()
}

fn score_gains(needs: &Needs, gains: impl Iterator<Item = (Need, f32)>) -> f32 {
    let mut s = 0.0;
    for (n, ad) in gains {
        if !needs.is_active(n) {
            continue;
        }
        let v = needs.get(n);
        let w = urgency(v) * need_weight(n);
        // you can't eat more than you're hungry: gains are capped by the deficit
        s += if ad > 0.0 { ad.min(100.0 - v + 5.0) * w } else { ad * 0.15 * (1.0 + w) };
    }
    s
}

fn time_of_day_mult(clock: &Clock, pers: &Personality, needs: &Needs, tags: &[String]) -> f32 {
    if tags.iter().any(|t| t == "sleep") {
        let night = if pers.has("night_owl") { clock.hour() >= 1 && clock.hour() < 9 } else { clock.is_night() };
        if night {
            1.4
        } else if needs.get(Need::Energy) < 25.0 {
            1.0
        } else {
            0.4
        }
    } else {
        1.0
    }
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub kind: ActionKind,
    pub label: String,
    pub score: f32,
    /// Needs this candidate advertises a meaningful gain for.
    pub helps: Vec<Need>,
}

/// Every option the peon could pick autonomously, scored and sorted (best first).
pub fn candidates(world: &World, e: Entity, data: &GameData) -> Vec<Candidate> {
    candidates_with_navigation(world, e, data).0
}

fn candidates_with_navigation(world: &World, e: Entity, data: &GameData) -> (Vec<Candidate>, NavField) {
    #[cfg(feature = "profiling")]
    let _span = crate::profiling::Span::new(1);
    let clock = *world.res::<Clock>();
    let funds = world.res::<Household>().funds;
    let needs = world.get::<Needs>(e).unwrap().clone();
    let pers = world.get::<Personality>(e).unwrap().clone();
    let stage = world.get::<Age>(e).unwrap().stage;
    let mood = world.get::<Mood>(e).map(|m| m.value).unwrap_or(0.0);
    let tf = *world.get::<Transform>(e).unwrap();
    let lot = world.res::<Lot>();
    let field = NavField::flood(&lot, NavNode::new(tf.level(), tf.tile()), 200.0);
    let recent = world.get::<Brain>(e).map(|b| b.recent.clone()).unwrap_or_default();
    // Repeating the same activity gets boring, unless it fixes something urgent.
    let boredom = |label: &str, helps: &[Need]| -> f32 {
        if helps.iter().any(|n| needs.get(*n) < 30.0) {
            return 1.0;
        }
        let n = recent.iter().filter(|(l, t)| l == label && clock.tick.saturating_sub(*t) < 240).count();
        0.45f32.powi(n as i32)
    };
    let mut out = Vec::new();
    let homework_due = world.get::<Student>(e).is_some_and(|s| !s.homework_done);
    let grade = world.get::<Student>(e).map(|s| s.grade).unwrap_or(0.0);
    let grieving = world.get::<Mood>(e).is_some_and(|m| m.moodlets.iter().any(|x| x.id == "grieving"));

    // objects
    for (oe, inst) in world.query::<(Entity, &ObjectInstance)>().iter() {
        let def = data.object(inst.def);
        for (ii, inter) in def.interactions.iter().enumerate() {
            if !inter.autonomous || interaction_check(data, inter, inst, stage, &pers, funds).is_err() || not_a_guest_bed(world, e, inter, stage).is_err() {
                continue;
            }
            if inter.has_tag("homework") && !homework_due {
                continue;
            }
            // walking distance to the nearest free spot: one of its own slots or a seat nearby
            let seat = inter.seat.as_ref().and_then(|sd| nearest_seat_by(world, data, e, oe, inst, sd, &field)).map_or(f32::INFINITY, |b| b.0);
            let dist = free_slots(def, inter, inst, e)
                .into_iter()
                .filter_map(|s| field.distance(NavNode::new(inst.level, inst.slot_stand_tile(def, s))))
                .fold(seat, f32::min);
            if !dist.is_finite() {
                continue;
            }
            let mut helps = Vec::new();
            let gains: Vec<(Need, f32)> = Need::ALL
                .into_iter()
                .map(|n| (n, inter.ad(n)))
                .filter(|(_, a)| *a != 0.0)
                .inspect(|(n, a)| {
                    if *a > 5.0 {
                        helps.push(*n)
                    }
                })
                .collect();
            let mut s = score_gains(&needs, gains.into_iter());
            s *= tag_multiplier(data, &pers, &inter.tags);
            s *= time_of_day_mult(&clock, &pers, &needs, &inter.tags);
            if inter.has_tag("mean") || inter.has_tag("mischief") {
                s *= 1.0 - mood.min(0.0) / 150.0;
            }
            if inst.dirt > 0.5 && !inter.cleans {
                s *= if pers.has("neat") { 0.5 } else { 0.85 };
            }
            if inter.has_tag("homework") {
                // school-night guilt, scaled by conscientiousness; stronger after school
                let after_school = (15..21).contains(&clock.hour());
                let slipping = if grade < 0.0 { 4.0 } else { 0.0 };
                s = s.max(0.0)
                    + (9.0 + pers.axes.neat as f32 * 0.6 + slipping)
                        * if after_school { 1.4 } else { 0.6 }
                        * tag_multiplier(data, &pers, &inter.tags).max(0.4);
            }
            if inter.has_tag("grief") {
                s = if grieving { s.max(0.0) + 8.0 } else { s * 0.3 };
            }
            if inter.cleans {
                // chores matter more when things are bad
                s += 6.0 * inst.dirt * tag_multiplier(data, &pers, &inter.tags);
            }
            if inter.repairs {
                // fixing something is worth what it unlocks (a working toilet is worth a lot)
                let unlocked = def
                    .interactions
                    .iter()
                    .filter(|i| !i.repairs && i.allowed_stage(stage))
                    .map(|i| score_gains(&needs, Need::ALL.into_iter().map(|n| (n, i.ad(n))).filter(|(_, a)| *a > 0.0)))
                    .fold(0.0, f32::max);
                s = s.max(0.0) + 4.0 + unlocked * 0.8 * tag_multiplier(data, &pers, &inter.tags).max(0.5);
                for n in Need::ALL {
                    if def.interactions.iter().any(|i| !i.repairs && i.ad(n) > 5.0) {
                        helps.push(n);
                    }
                }
            }
            s /= 1.0 + dist / 25.0;
            let label = format!("{} ({})", inter.name, def.name);
            s *= boredom(&label, &helps);
            if s > 0.01 {
                out.push(Candidate {
                    kind: ActionKind::Object { obj: oe, interaction: ii },
                    label,
                    score: s,
                    helps,
                });
            }
        }
    }

    // This lookup is valid only during the read-only scoring pass.
    let commitments = crate::romance::Commitments::new(world);
    // Only score socials that have an eligible target; reuse initiator-only work
    // across subsequent targets without paying for unused options in small homes.
    let mut social_scores = vec![None; data.socials.len()];
    // other peons
    for (te, _tage, ttf, tneeds, taq, tpeon) in world
        .query_filtered::<(Entity, &Age, &Transform, &Needs, &ActionQueue, &Peon), (hearth_ecs::Without<Dead>, hearth_ecs::Without<AtWork>, hearth_ecs::Without<Offlot>)>()
        .iter()
    {
        if te == e {
            continue;
        }
        // don't bother sleeping or already-engaged peons
        let busy = taq.current.as_ref().is_some_and(|c| {
            matches!(c.action.kind, ActionKind::Respond { .. } | ActionKind::PassOut | ActionKind::GoToWork | ActionKind::GoToSchool)
                || c.action.source == Source::Player
        });
        if busy {
            continue;
        }
        // sleeping peons can still be cared for (diapers wait for no one)
        let asleep = taq.current.as_ref().is_some_and(|c| is_sleeping(world, data, c));
        let Some(dist) = adjacent_distance(&lot, &field, ttf) else { continue };
        for (si, sd) in data.socials.iter().enumerate() {
            if !sd.autonomous || (asleep && !sd.care) || social_allowed_with(world, sd, e, te, Some(&commitments)).is_err() {
                continue;
            }
            let (base, helps, multiplier) = social_scores[si].get_or_insert_with(|| {
                let mut helps = Vec::new();
                let base = if sd.care { 0.0 } else {
                    for (n, r) in &sd.rates {
                        if *r * sd.minutes as f32 / 60.0 > 5.0 { helps.push(*n); }
                    }
                    score_gains(&needs, sd.rates.iter().map(|(n, r)| (*n, (r * sd.minutes as f32 / 60.0).clamp(-100.0, 100.0))))
                };
                (base, helps, tag_multiplier(data, &pers, &sd.tags))
            });
            let mut s = if sd.care {
                // empathy: driven by the target's needs
                let g: f32 = sd
                    .target_rates
                    .iter()
                    .map(|(n, r)| {
                        let ad = r * sd.minutes as f32 / 60.0;
                        if ad > 0.0 && tneeds.is_active(*n) { ad.min(100.0) * urgency(tneeds.get(*n)) * 1.4 } else { 0.0 }
                    })
                    .sum();
                g * (0.6 + pers.axes.nice as f32 * 0.06)
            } else {
                *base
            };
            s *= *multiplier;
            // the spoken-for mostly keep their eyes to themselves, especially in front of their partner
            if sd.romantic
                && let Some(p) = commitments.get(e)
                && p != te
            {
                s *= if pers.has("romantic") { 0.25 } else { 0.08 };
                if !crate::components::is_away(world, p) && !world.has::<crate::components::Dead>(p) {
                    s *= 0.3;
                }
            }
            if sd.tags.iter().any(|t| t == "mean") {
                s *= (1.0 - mood / 100.0).clamp(0.2, 2.0) * 0.6;
            }
            // expected value of acceptance
            s *= 0.4 + 0.6 * accept_chance_with(world, sd, e, te, Some(&commitments));
            s /= 1.0 + dist / 25.0;
            let label = format!("{} {}", sd.name, tpeon.first);
            s *= boredom(&label, helps);
            if s > 0.01 {
                out.push(Candidate {
                    kind: ActionKind::Social { target: te, social: si },
                    label,
                    score: s,
                    helps: helps.clone(),
                });
            }
        }
    }
    // ties are broken by label and target so the choice never depends on ECS storage order
    let key = |k: &ActionKind| match k {
        ActionKind::Object { obj, interaction } => (obj.to_bits(), *interaction),
        ActionKind::Social { target, social } => (target.to_bits(), *social),
        _ => (0, 0),
    };
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.label.cmp(&b.label)).then_with(|| key(&a.kind).cmp(&key(&b.kind))));
    (out, field)
}

pub(crate) fn is_sleeping(world: &World, data: &GameData, c: &CurrentAction) -> bool {
    match &c.action.kind {
        ActionKind::Object { obj, interaction } => world
            .get::<ObjectInstance>(*obj)
            .map(|o| data.object(o.def).interactions[*interaction].has_tag("sleep"))
            .unwrap_or(false),
        ActionKind::PassOut => true,
        _ => false,
    }
}

/// Path distance to some tile next to the target.
fn adjacent_distance(lot: &Lot, field: &NavField, target: &Transform) -> Option<f32> {
    let t = target.tile();
    let l = target.level();
    let mut best = None::<f32>;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let n = NavNode::new(l, t + glam::IVec2::new(dx, dz));
            if (dx, dz) == (0, 0) && !lot.walkable(l, n.tile) {
                continue;
            }
            if let Some(d) = field.distance(n) {
                best = Some(best.map_or(d, |b: f32| b.min(d)));
            }
        }
    }
    best
}

/// Picks what to do next. Records the top options on the [`Brain`] for debugging.
pub fn choose(world: &World, e: Entity, data: &GameData) -> Option<(ActionKind, String)> {
    choose_with_navigation(world, e, data).0
}

pub(crate) fn choose_with_navigation(world: &World, e: Entity, data: &GameData) -> (Option<(ActionKind, String)>, Option<NavField>) {
    let fw = world
        .get::<Brain>(e)
        .and_then(|b| b.free_will)
        .unwrap_or_else(|| world.res::<Settings>().free_will);
    if fw == FreeWill::Off {
        return (None, None);
    }
    let needs = world.get::<Needs>(e).unwrap().clone();
    let (lowest, lowest_v) = needs.most_urgent();
    if fw == FreeWill::Low && needs.lowest_core().1 >= 35.0 {
        return (None, None);
    }
    let (mut cands, field) = candidates_with_navigation(world, e, data);
    let reusable = field.is_complete().then_some(field);
    if fw == FreeWill::Low {
        cands.retain(|c| c.helps.iter().any(|n| needs.get(*n) < 40.0));
    }
    if let Some(mut b) = world.get_mut::<Brain>(e) {
        b.last_scores = cands.iter().take(12).map(|c| ScoredOption { label: c.label.clone(), score: c.score }).collect();
        b.last_think_tick = world.res::<Clock>().tick;
    }
    if cands.is_empty() {
        return (None, None);
    }
    // When a core need is critical, prefer something that actually helps it.
    let critical = lowest_v < 15.0;
    let mut unfixable = None;
    if critical {
        if let Some(c) = cands.iter().find(|c| c.helps.contains(&lowest)) {
            if let Some(mut b) = world.get_mut::<Brain>(e) {
                b.unfixable = None;
            }
            return (Some((c.kind.clone(), c.label.clone())), reusable);
        }
        // nothing reachable helps; don't let the critical-need interrupt thrash every action
        unfixable = Some(lowest);
    }
    if let Some(mut b) = world.get_mut::<Brain>(e) {
        b.unfixable = unfixable;
    }
    let top: Vec<&Candidate> = cands.iter().take(3).filter(|c| c.score >= cands[0].score * 0.5).collect();
    let weights: Vec<f32> = top.iter().map(|c| c.score * c.score).collect();
    let pick = world.res_mut::<SimRng>().weighted(&weights).unwrap_or(0);
    (Some((top[pick].kind.clone(), top[pick].label.clone())), reusable)
}

/// Options for the player's pie menu, including unavailable ones with the reason.
pub fn menu_options(world: &World, peon: Entity, target: Entity) -> Vec<(String, ActionKind, Result<(), String>)> {
    let data = crate::sim::data(world);
    let mut out = Vec::new();
    let Some(stage) = world.get::<Age>(peon).map(|a| a.stage) else { return out };
    let pers = world.get::<Personality>(peon).unwrap().clone();
    let funds = world.res::<Household>().funds;
    if let Some(inst) = world.get::<ObjectInstance>(target) {
        let def = data.object(inst.def);
        for (i, inter) in def.interactions.iter().enumerate() {
            // hide trait-locked options from peons without the trait, and repair-only when not broken
            if inter.requires_trait.as_ref().is_some_and(|t| !pers.has(t)) || (inter.only_broken && !inst.broken) {
                continue;
            }
            let mut ok = interaction_check(&data, inter, &inst, stage, &pers, funds).and_then(|()| not_a_guest_bed(world, peon, inter, stage));
            if ok.is_ok() && free_slots(def, inter, &inst, peon).is_empty() && !seat_free(world, &data, peon, target, &inst, inter) {
                ok = Err("in use".into());
            }
            let label = if inter.cost > 0 { format!("{} (${})", inter.name, inter.cost) } else { inter.name.clone() };
            out.push((label, ActionKind::Object { obj: target, interaction: i }, ok));
        }
    } else if world.has::<Peon>(target) && target != peon {
        for (i, sd) in data.socials.iter().enumerate() {
            if sd.requires_trait.as_ref().is_some_and(|t| !pers.has(t)) {
                continue;
            }
            let ok = social_allowed(world, sd, peon, target).map_err(String::from);
            if ok.is_err() && is_hidden_social(sd, world, peon, target) {
                continue;
            }
            out.push((sd.name.clone(), ActionKind::Social { target, social: i }, ok));
        }
    }
    out
}

fn is_hidden_social(sd: &SocialDef, world: &World, peon: Entity, target: Entity) -> bool {
    // don't show baby-care options for adults, or wedding plans for strangers
    world.get::<Age>(target).is_some_and(|a| !sd.target_ok(a.stage)) || crate::social::social_pair_ok(world, sd, peon, target).is_err()
}
