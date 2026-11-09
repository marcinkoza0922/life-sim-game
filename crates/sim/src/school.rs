//! School: children and teens take the bus on weekdays, come home drained, and bring back grades
//! that depend on mood and whether they did their homework. Report cards go out on Fryday.

use hearth_ecs::{Entity, With, Without, World};

use crate::actions;
use crate::components::*;
use crate::data::{LifeStage, Need, Skill};
use crate::log::LogKind;
use crate::lot::Lot;
use crate::rng::SimRng;
use crate::sim::{add_moodlet, log, name_of, now};
use crate::time::Clock;

pub const SCHOOL_START_HOUR: u64 = 8;
pub const SCHOOL_HOURS: u64 = 6;
/// Moonday–Fryday.
pub const SCHOOL_DAYS: [u8; 5] = [0, 1, 2, 3, 4];
/// Catches the bus this many minutes before the bell.
const BUS_LEAD: u64 = 45;

/// Sends students to the bus, and marks truants who never made it.
pub fn schedule_school(world: &mut World) {
    let clock = *world.res::<Clock>();
    if !SCHOOL_DAYS.contains(&clock.weekday()) {
        return;
    }
    let mod_ = clock.minute_of_day();
    let start = SCHOOL_START_HOUR * 60;
    let students: Vec<(Entity, Student)> = world
        .query_filtered::<(Entity, &Student), (Without<Dead>, Without<AtWork>)>()
        .iter()
        .map(|(e, s)| (e, s.clone()))
        .collect();
    let mut students = students;
    students.sort_by_key(|(e, _)| *e);
    for (e, st) in students {
        if mod_ == start - BUS_LEAD && st.last_school_day != Some(clock.day()) {
            world.get_mut::<Student>(e).unwrap().last_school_day = Some(clock.day());
            actions::abort_current(world, e);
            actions::push_front(world, e, ActionKind::GoToSchool, Source::System, "Catch the School Bus".into());
        }
        if mod_ == start + 60 && st.last_school_day == Some(clock.day()) {
            let mut q = world.get_mut::<ActionQueue>(e).unwrap();
            let pending = q.queue.iter().any(|a| matches!(a.kind, ActionKind::GoToSchool))
                || q.current.as_ref().is_some_and(|c| matches!(c.action.kind, ActionKind::GoToSchool));
            if pending {
                q.queue.retain(|a| !matches!(a.kind, ActionKind::GoToSchool));
                if q.current.as_ref().is_some_and(|c| matches!(c.action.kind, ActionKind::GoToSchool)) {
                    q.current.as_mut().unwrap().cancel = true;
                }
                drop(q);
                world.get_mut::<Student>(e).unwrap().grade -= 12.0;
                log(world, LogKind::Warning, Some(e), format!("{} missed the school bus. The truant officer has been notified and is sharpening a pencil.", name_of(world, e)));
            }
        }
    }
}

/// Called when a student reaches the bus stop.
pub fn begin_school(world: &World, e: Entity) {
    let clock = *world.res::<Clock>();
    let tick = now(world);
    let end = (SCHOOL_START_HOUR + SCHOOL_HOURS) * 60;
    let until = tick + end.saturating_sub(clock.minute_of_day()).max(60);
    world.defer_insert(e, (AtWork { until, event_at: u64::MAX, event_done: true, school: true },));
    // free school breakfast: nobody learns long division on an empty stomach
    if let Some(mut n) = world.get_mut::<Needs>(e) {
        let h = n.get(Need::Hunger).max(55.0);
        n.set(Need::Hunger, h);
        let b = n.get(Need::Bladder).max(50.0);
        n.set(Need::Bladder, b);
    }
    log(world, LogKind::Info, Some(e), format!("{} got on the school bus.", name_of(world, e)));
}

/// Brings students home when the bell rings and grades the day.
pub fn run_school(world: &mut World) {
    let tick = now(world);
    let done: Vec<Entity> = world
        .query_filtered::<(Entity, &AtWork), With<Student>>()
        .iter()
        .filter(|(_, a)| a.school && tick >= a.until)
        .map(|(e, _)| e)
        .collect();
    let mut done = done;
    done.sort();
    for e in done {
        end_school_day(world, e);
    }
}

fn end_school_day(world: &mut World, e: Entity) {
    world.remove::<AtWork>(e);
    let home = { let lot = world.res::<Lot>(); lot.ground_center(0, actions::exit_tile(&lot)) };
    if let Some(mut tf) = world.get_mut::<Transform>(e) {
        tf.pos = home;
        tf.prev_pos = tf.pos;
    }
    {
        let mut n = world.get_mut::<Needs>(e).unwrap();
        // lunch was mystery meat, but it was lunch
        let h = n.get(Need::Hunger).max(45.0);
        n.set(Need::Hunger, h);
        for (need, d) in [(Need::Hunger, -10.0), (Need::Energy, -15.0), (Need::Fun, -25.0), (Need::Hygiene, -8.0)] {
            let v = n.get(need);
            n.set(need, (v + d).max(v.min(5.0)));
        }
        n.add(Need::Social, 30.0);
        let b = n.get(Need::Bladder).max(55.0);
        n.set(Need::Bladder, b);
    }
    let mood = world.get::<Mood>(e).map(|m| m.value).unwrap_or(0.0);
    let luck = world.res_mut::<SimRng>().range_f32(-4.0, 4.0);
    let (homework, letter_before) = {
        let s = world.get::<Student>(e).unwrap();
        (s.homework_done, s.letter())
    };
    let delta = 2.0 + mood / 12.0 + if homework { 7.0 } else { -6.0 } + luck;
    {
        let mut s = world.get_mut::<Student>(e).unwrap();
        s.grade = (s.grade + delta).clamp(-100.0, 100.0);
        s.homework_done = false;
    }
    world.get_mut::<Skills>(e).unwrap().add(Skill::Logic, 0.15);
    let name = name_of(world, e);
    if !homework && world.res_mut::<SimRng>().chance(0.25) {
        const EXCUSES: [&str; 4] = [
            "claimed the dog ate their homework. They do not have a dog.",
            "handed in a drawing of a horse instead of homework. Partial credit for the horse.",
            "said their homework 'was stolen by a cryptid'. The teacher has seen this before.",
            "turned in their homework on a napkin. It was mostly ketchup.",
        ];
        let i = world.res_mut::<SimRng>().below(EXCUSES.len());
        log(world, LogKind::Funny, Some(e), format!("{name} {}", EXCUSES[i]));
    }
    let letter = world.get::<Student>(e).unwrap().letter();
    let clock = *world.res::<Clock>();
    if clock.weekday() == 4 {
        log(world, LogKind::Life, Some(e), format!("{name} brought home a report card: {letter}."));
        match letter {
            "A" => add_moodlet(world, e, "honor_roll"),
            "D" | "F" => add_moodlet(world, e, "bad_report_card"),
            _ => {}
        }
    } else if letter != letter_before {
        log(world, LogKind::Info, Some(e), format!("{name}'s grade is now {letter}."));
    }
}

/// Homework counts once per school day.
pub fn homework_done(world: &World, e: Entity) {
    if let Some(mut s) = world.get_mut::<Student>(e)
        && !s.homework_done
    {
        s.homework_done = true;
        s.grade = (s.grade + 2.0).min(100.0);
    }
}

/// Enrollment changes at stage transitions.
pub fn on_stage_change(world: &mut World, e: Entity, stage: LifeStage) {
    match stage {
        LifeStage::Child => {
            world.insert_one(e, Student::new());
            log(world, LogKind::Life, Some(e), format!("{} started school and already has a favorite crayon.", name_of(world, e)));
        }
        LifeStage::YoungAdult => {
            if let Some(s) = world.remove::<Student>(e) {
                let letter = s.letter();
                let name = name_of(world, e);
                let line = match letter {
                    "A" => format!("{name} graduated with honors (an A)! The valedictorian speech was mostly about snacks."),
                    "F" => format!("{name} graduated... technically. With an F. The diploma is printed on a paper plate."),
                    l => format!("{name} graduated with a {l} average."),
                };
                log(world, LogKind::Life, Some(e), line);
                if letter == "A" {
                    world.get_mut::<Skills>(e).unwrap().add(Skill::Logic, 1.0);
                    add_moodlet(world, e, "honor_roll");
                }
            }
        }
        _ => {}
    }
}
