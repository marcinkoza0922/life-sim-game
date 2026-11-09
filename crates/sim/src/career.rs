//! Rabbit-hole careers: scheduling, shifts, comedic choice events, pay and promotions.

use hearth_ecs::{Entity, With, Without, World};

use crate::actions::{self, exit_tile};
use crate::components::*;
use crate::data::{CareerEffect, GameData, Need, Skill};
use crate::log::LogKind;
use crate::lot::Lot;
use crate::rng::SimRng;
use crate::sim::{add_moodlet, data, fill, log, name_of, now};
use crate::speech::redact;
use crate::time::{Clock, Settings};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PendingChoice {
    pub peon: Entity,
    pub career: String,
    pub event: usize,
    pub text: String,
    pub choices: Vec<String>,
    pub deadline: u64,
}

/// Career events waiting for the player to choose.
#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingChoices {
    pub items: Vec<PendingChoice>,
}

pub fn schedule_work(world: &mut World) {
    let data = data(world);
    let clock = *world.res::<Clock>();
    let mod_ = clock.minute_of_day();
    let jobs: Vec<(Entity, Job)> = world
        .query_filtered::<(Entity, &Job), (Without<Dead>, Without<AtWork>, Without<Offlot>, Without<Visitor>)>()
        .iter()
        .map(|(e, j)| (e, j.clone()))
        .collect();
    let mut jobs = jobs;
    jobs.sort_by_key(|(e, _)| *e);
    for (e, job) in jobs {
        let Some(c) = data.career(&job.career) else { continue };
        let lvl = &c.levels[job.level];
        if !lvl.days.contains(&clock.weekday()) {
            continue;
        }
        let start = lvl.start_hour as u64 * 60;
        let leave_at = start.saturating_sub(45);
        if mod_ == leave_at && job.last_shift_day != Some(clock.day()) {
            world.get_mut::<Job>(e).unwrap().last_shift_day = Some(clock.day());
            actions::abort_current(world, e);
            actions::push_front(world, e, ActionKind::GoToWork, Source::System, "Go to Work".into());
        }
        // missed the carpool?
        if mod_ == start + 60 && job.last_shift_day == Some(clock.day()) {
            let mut q = world.get_mut::<ActionQueue>(e).unwrap();
            let pending = q.queue.iter().any(|a| matches!(a.kind, ActionKind::GoToWork))
                || q.current.as_ref().is_some_and(|c| matches!(c.action.kind, ActionKind::GoToWork));
            if pending {
                q.queue.retain(|a| !matches!(a.kind, ActionKind::GoToWork));
                drop(q);
                world.get_mut::<Job>(e).unwrap().performance -= 15.0;
                log(world, LogKind::Career, Some(e), format!("{} missed work. The boss has been informed. The boss is disappointed.", name_of(world, e)));
            }
        }
    }
}

pub fn begin_shift(world: &World, e: Entity) {
    let data = data(world);
    let tick = now(world);
    let Some(job) = world.get::<Job>(e).map(|j| j.clone()) else { return };
    let Some(c) = data.career(&job.career) else { return };
    let lvl = &c.levels[job.level];
    let clock = *world.res::<Clock>();
    let start = lvl.start_hour as u64 * 60;
    // shift ends `hours` after the scheduled start (late arrivals still leave on time)
    let end_mod = start + lvl.hours as u64 * 60;
    let until = if clock.minute_of_day() <= start { tick + (start - clock.minute_of_day()) + lvl.hours as u64 * 60 } else { tick + end_mod.saturating_sub(clock.minute_of_day()).max(60) };
    let event_at = tick + 30 + world.res_mut::<SimRng>().below(((until - tick).max(61) - 60) as usize) as u64;
    world.defer_insert(e, (AtWork { until, event_at, event_done: false, school: false },));
    log(world, LogKind::Career, Some(e), format!("{} left for work ({}).", name_of(world, e), lvl.title));
}

pub fn run_shifts(world: &mut World) {
    let data = data(world);
    let tick = now(world);
    let at_work: Vec<(Entity, AtWork)> = world
        .query_filtered::<(Entity, &AtWork), With<Job>>()
        .iter()
        .map(|(e, a)| (e, a.clone()))
        .collect();
    let mut at_work = at_work;
    at_work.sort_by_key(|(e, _)| *e);
    for (e, aw) in at_work {
        // someone who died earlier this tick (an accident at work, flushed by another shift's end)
        if aw.school || !world.has::<AtWork>(e) {
            continue;
        }
        if !aw.event_done && tick >= aw.event_at {
            world.get_mut::<AtWork>(e).unwrap().event_done = true;
            fire_event(world, e, &data, aw.until);
        }
        if tick >= aw.until {
            end_shift(world, e, &data);
        }
    }
}

fn fire_event(world: &World, e: Entity, data: &GameData, deadline: u64) {
    let job = world.get::<Job>(e).unwrap().clone();
    let c = data.career(&job.career).unwrap();
    let options: Vec<usize> = (0..c.events.len()).filter(|&i| c.events[i].min_level <= job.level).collect();
    let Some(&ev_i) = world.res_mut::<SimRng>().pick(&options) else { return };
    let ev = &c.events[ev_i];
    let mut text = fill(world, e, &ev.text);
    if c.redact {
        text = redact(&mut world.res_mut::<SimRng>(), &text, 0.3);
    }
    let auto = world.res::<Settings>().auto_career_choices;
    if auto {
        let choice = best_choice(world, e, data, &job.career, ev_i);
        log(world, LogKind::Career, Some(e), format!("At work: {}", text));
        apply_choice(world, e, data, &job.career, ev_i, choice);
    } else {
        world.res_mut::<PendingChoices>().items.push(PendingChoice {
            peon: e,
            career: job.career.clone(),
            event: ev_i,
            text,
            choices: ev.choices.iter().map(|c| c.label.clone()).collect(),
            deadline,
        });
    }
}

/// Probability of passing a skill check.
pub fn check_chance(skill: f32, difficulty: f32, mood: f32) -> f32 {
    (0.35 + (skill - difficulty) * 0.15 + mood * 0.002).clamp(0.05, 0.95)
}

fn best_choice(world: &World, e: Entity, data: &GameData, career: &str, ev: usize) -> usize {
    let c = data.career(career).unwrap();
    let skills = world.get::<Skills>(e).unwrap().clone();
    let mood = world.get::<Mood>(e).unwrap().value;
    let perf = |effs: &[CareerEffect]| -> f32 {
        effs.iter()
            .map(|x| match x {
                CareerEffect::Performance(p) => *p,
                CareerEffect::Money(m) => *m as f32 / 50.0,
                CareerEffect::Fired => -100.0,
                CareerEffect::Accident(p, _) => -200.0 * p,
                _ => 0.0,
            })
            .sum()
    };
    let ev = &c.events[ev];
    (0..ev.choices.len())
        .max_by(|&a, &b| {
            let val = |i: usize| {
                let ch = &ev.choices[i];
                match ch.check {
                    Some((s, d)) => {
                        let p = check_chance(skills.get(s), d, mood);
                        p * perf(&ch.success) + (1.0 - p) * perf(&ch.failure)
                    }
                    None => perf(&ch.success),
                }
            };
            val(a).total_cmp(&val(b))
        })
        .unwrap_or(0)
}

fn apply_choice(world: &World, e: Entity, data: &GameData, career: &str, ev: usize, choice: usize) {
    let c = data.career(career).unwrap();
    let Some(ch) = c.events[ev].choices.get(choice) else { return };
    let success = match ch.check {
        Some((s, d)) => {
            let sk = world.get::<Skills>(e).unwrap().get(s);
            let mood = world.get::<Mood>(e).unwrap().value;
            world.res_mut::<SimRng>().chance(check_chance(sk, d, mood))
        }
        None => true,
    };
    let effects = if success { &ch.success } else { &ch.failure };
    let name = name_of(world, e);
    log(world, LogKind::Career, Some(e), format!("{} chose \"{}\" — {}.", name, ch.label, if success { "it worked" } else { "it did not work" }));
    for eff in effects {
        match eff {
            CareerEffect::Performance(p) => {
                if let Some(mut j) = world.get_mut::<Job>(e) {
                    j.performance = (j.performance + p).clamp(-100.0, 150.0);
                }
            }
            CareerEffect::Money(m) => crate::economy::credit(world, e, *m),
            CareerEffect::Moodlet(id) => add_moodlet(world, e, id),
            CareerEffect::Skill(s, v) => {
                world.get_mut::<Skills>(e).unwrap().add(*s, *v);
            }
            CareerEffect::Log(t) => {
                let t = fill(world, e, t);
                log(world, LogKind::Funny, Some(e), t);
            }
            CareerEffect::Accident(p, cause) => {
                if world.res_mut::<SimRng>().chance(*p) {
                    let cause = cause.clone();
                    world.defer(move |w| crate::life::die(w, e, &cause));
                }
            }
            CareerEffect::Fired => {
                world.defer_remove::<Job>(e);
                add_moodlet(world, e, "fired");
                log(world, LogKind::Career, Some(e), format!("{} was fired from the {} career.", name, c.name));
            }
        }
    }
}

pub fn resolve_pending(world: &mut World, index: usize, choice: usize) {
    let data = data(world);
    let item = {
        let mut p = world.res_mut::<PendingChoices>();
        if index >= p.items.len() {
            return;
        }
        p.items.remove(index)
    };
    if world.contains(item.peon) {
        apply_choice(world, item.peon, &data, &item.career, item.event, choice);
    }
    world.flush();
}

fn end_shift(world: &mut World, e: Entity, data: &GameData) {
    // resolve an unanswered event automatically
    let pending: Vec<usize> = world.res::<PendingChoices>().items.iter().enumerate().filter(|(_, p)| p.peon == e).map(|(i, _)| i).collect();
    for i in pending.into_iter().rev() {
        let (career, ev) = {
            let p = &world.res::<PendingChoices>().items[i];
            (p.career.clone(), p.event)
        };
        let ch = best_choice(world, e, data, &career, ev);
        world.res_mut::<PendingChoices>().items.remove(i);
        apply_choice(world, e, data, &career, ev, ch);
    }
    world.flush();
    world.remove::<AtWork>(e);
    if world.has::<Dead>(e) {
        return;
    }
    // come home (to wherever the household is; nowhere visible if they've been parked off-lot)
    if !world.has::<Offlot>(e) {
        let home = { let lot = world.res::<Lot>(); lot.ground_center(0, exit_tile(&lot)) };
        if let Some(mut tf) = world.get_mut::<Transform>(e) {
            tf.pos = home;
            tf.prev_pos = tf.pos;
        }
    }
    {
        // a shift is draining, but the commute home never takes anyone below 5
        let mut n = world.get_mut::<Needs>(e).unwrap();
        for (need, d) in [(Need::Hunger, -25.0), (Need::Energy, -25.0), (Need::Fun, -20.0), (Need::Hygiene, -15.0)] {
            let v = n.get(need);
            n.set(need, (v + d).max(v.min(5.0)));
        }
        n.add(Need::Social, 20.0);
        let b = n.get(Need::Bladder).max(55.0);
        n.set(Need::Bladder, b);
        n.add(Need::Comfort, -10.0);
    }
    let Some(job) = world.get::<Job>(e).map(|j| j.clone()) else { return };
    let c = data.career(&job.career).unwrap();
    let lvl = &c.levels[job.level];
    let pay = lvl.pay * lvl.hours as i64;
    crate::economy::credit(world, e, pay);
    let mood = world.get::<Mood>(e).unwrap().value;
    let gain = 6.0 + mood / 8.0;
    let skills = world.get::<Skills>(e).unwrap().clone();
    let mut promoted = None;
    {
        let mut j = world.get_mut::<Job>(e).unwrap();
        j.days_worked += 1;
        j.performance = (j.performance + gain).clamp(-100.0, 150.0);
        if j.performance >= 100.0 && j.level + 1 < c.levels.len() {
            let next = &c.levels[j.level + 1];
            if next.skills.iter().all(|(s, v)| skills.get(*s) >= *v) {
                j.level += 1;
                j.performance = 25.0;
                promoted = Some(next.title.clone());
            }
        }
    }
    let name = name_of(world, e);
    log(world, LogKind::Money, Some(e), format!("{} came home from work with ${}.", name, pay));
    if let Some(t) = promoted {
        add_moodlet(world, e, "promoted");
        log(world, LogKind::Career, Some(e), format!("{} was promoted to {}!", name, t));
    }
    if world.get::<Job>(e).unwrap().performance <= -100.0 {
        world.remove::<Job>(e);
        add_moodlet(world, e, "fired");
        log(world, LogKind::Career, Some(e), format!("{} was fired for consistently poor performance.", name));
    }
    let _ = Skill::ALL;
}

/// Gives a peon a job at level 0 (player choice from UI).
pub fn join_career(world: &mut World, e: Entity, career: &str) -> Result<(), String> {
    let data = data(world);
    let c = data.career(career).ok_or("no such career")?;
    let stage = world.get::<Age>(e).ok_or("not a peon")?.stage;
    if stage < c.min_stage {
        return Err("too young for that job".into());
    }
    world.insert_one(e, Job { career: career.into(), level: 0, performance: 0.0, days_worked: 0, last_shift_day: None });
    log(world, LogKind::Career, Some(e), format!("{} joined the {} career as {}.", name_of(world, e), c.name, c.levels[0].title));
    Ok(())
}
