//! Accidental deaths: electrocution, vending machines, laughter, hubris, and the Reaper's terms.

use glam::IVec2;
use sim::career::{PendingChoice, PendingChoices};
use sim::components::*;
use sim::data::{CareerEffect, GameData, LifeStage, Need, Outcome, Skill};
use sim::log::SimLog;
use sim::lot::Lot;
use sim::objects::ObjectInstance;
use sim::starter::starter_sim;
use sim::time::Settings;
use sim::{Entity, Sim};

fn find_object(sim: &Sim, id: &str) -> Entity {
    let data = sim.data();
    sim.objects()
        .into_iter()
        .find(|e| sim.world.get::<ObjectInstance>(*e).is_some_and(|o| data.object(o.def).id == id))
        .unwrap_or_else(|| panic!("no {id}"))
}

fn interaction(sim: &Sim, obj: Entity, id: &str) -> usize {
    let data = sim.data();
    let o = sim.world.get::<ObjectInstance>(obj).unwrap();
    data.object(o.def).interactions.iter().position(|i| i.id == id).unwrap()
}

fn lone_adult(seed: u64) -> (Sim, Entity) {
    let mut sim = Sim::new(seed, GameData::embedded());
    sim.world.res_mut::<Settings>().aging = false;
    let spec = sim.random_spec(LifeStage::Adult, None);
    let p = sim.spawn_peon(spec, Lot::tile_center(0, IVec2::new(5, 5)));
    (sim, p)
}

#[test]
fn skill_makes_accidents_rarer_and_clumsiness_commoner() {
    let (sim, p) = lone_adult(1);
    sim.world.get_mut::<Personality>(p).unwrap().traits.retain(|t| t != "clumsy");
    let chance = |sim: &Sim| sim::life::accident_chance(&sim.world, p, 0.2, Some(Skill::Handiness));
    sim.world.get_mut::<Skills>(p).unwrap().v = Default::default();
    assert!((chance(&sim) - 0.2).abs() < 1e-6);
    sim.world.get_mut::<Skills>(p).unwrap().add(Skill::Handiness, 10.0);
    assert!((chance(&sim) - 0.04).abs() < 1e-6, "a master is 80% safer");
    sim.world.get_mut::<Personality>(p).unwrap().traits.push("clumsy".into());
    assert!((chance(&sim) - 0.08).abs() < 1e-6, "clumsy doubles it");
    assert_eq!(sim::life::accident_chance(&sim.world, p, 0.2, None), 0.4, "no skill helps without one");
}

#[test]
fn a_fatal_mishap_kills_unless_the_reaper_is_talked_round() {
    for seed in 0..20 {
        let (mut sim, p) = lone_adult(seed);
        sim::life::mishap(&sim.world, p, "{name} touched the thing.", Some("singed"), 1.0, "electrocution");
        sim.world.flush();
        let log = sim.world.res::<SimLog>();
        assert!(log.contains("touched the thing"));
        let mood = sim.world.get::<Mood>(p).unwrap();
        assert!(mood.moodlets.iter().any(|m| m.id == "singed"));
        match sim.world.get::<Dead>(p) {
            Some(d) => {
                assert_eq!(d.cause, "electrocution");
                assert!(log.contains("electrocuted"));
            }
            None => {
                assert!(log.contains("The Reaper came for"));
                assert!(mood.moodlets.iter().any(|m| m.id == "cheated_death"));
            }
        }
    }
}

#[test]
fn charisma_talks_the_reaper_round_more_often() {
    let reprieves = |charisma: f32| {
        (0..200)
            .filter(|&seed| {
                let (mut sim, p) = lone_adult(seed);
                sim.world.get_mut::<Skills>(p).unwrap().v = Default::default();
                sim.world.get_mut::<Skills>(p).unwrap().add(Skill::Charisma, charisma);
                sim::life::die(&mut sim.world, p, "vending machine");
                !sim.world.has::<Dead>(p)
            })
            .count()
    };
    let (dull, smooth) = (reprieves(0.0), reprieves(10.0));
    assert!(dull > 5 && dull < 45, "about 10% win at rock-paper-scissors: {dull}");
    assert!(smooth > 100, "a silver tongue gets about 55%: {smooth}");
}

#[test]
fn unskilled_repairs_of_live_wiring_shock_people() {
    let mut sim = starter_sim(12);
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    sim.world.res_mut::<Settings>().aging = false;
    let adult = sim.living_peons().into_iter().find(|e| sim.world.get::<Age>(*e).unwrap().stage.is_adult()).unwrap();
    let stove = find_object(&sim, "stove_basic");
    let repair = interaction(&sim, stove, "repair");
    let mut shocks = 0;
    for _ in 0..40 {
        sim.world.get_mut::<ObjectInstance>(stove).unwrap().broken = true;
        sim.world.get_mut::<Skills>(adult).unwrap().v = Default::default();
        {
            let mut n = sim.world.get_mut::<Needs>(adult).unwrap();
            for k in Need::ALL {
                n.set(k, 90.0);
            }
        }
        sim.order(adult, ActionKind::Object { obj: stove, interaction: repair }).unwrap();
        for _ in 0..120 {
            sim.tick();
            if !sim.world.get::<ObjectInstance>(stove).unwrap().broken {
                break;
            }
        }
        assert!(!sim.world.get::<ObjectInstance>(stove).unwrap().broken, "the repair finishes");
        if sim.world.get::<Mood>(adult).unwrap().moodlets.iter().any(|m| m.id == "singed") || sim.world.has::<Dead>(adult) {
            shocks += 1;
            if sim.world.has::<Dead>(adult) {
                assert_eq!(sim.world.get::<Dead>(adult).unwrap().cause, "electrocution");
                return;
            }
            sim.world.get_mut::<Mood>(adult).unwrap().moodlets.retain(|m| m.id != "singed");
        }
    }
    assert!((2..=20).contains(&shocks), "about one unskilled repair in five shocks: {shocks}");
}

#[test]
fn dying_at_work_ends_the_shift_and_clears_its_decision() {
    let mut sim = starter_sim(21);
    sim.world.res_mut::<Settings>().aging = false;
    sim.world.res_mut::<Settings>().auto_career_choices = false;
    let adult = sim.living_peons().into_iter().find(|e| sim.world.get::<Job>(*e).is_some()).expect("someone has a job");
    sim::career::begin_shift(&sim.world, adult);
    sim.world.flush();
    assert!(sim.world.has::<AtWork>(adult));
    sim.world.res_mut::<PendingChoices>().items.push(PendingChoice {
        peon: adult,
        career: sim.world.get::<Job>(adult).unwrap().career.clone(),
        event: 0,
        text: "test".into(),
        choices: vec!["a".into()],
        deadline: u64::MAX,
    });
    // the Reaper may be talked round; keep trying until he isn't
    for _ in 0..50 {
        sim::life::die(&mut sim.world, adult, "hubris");
        if sim.world.has::<Dead>(adult) {
            break;
        }
        sim.world.get_mut::<Mood>(adult).unwrap().moodlets.retain(|m| m.id != "cheated_death");
    }
    assert!(sim.world.has::<Dead>(adult));
    assert!(!sim.world.has::<AtWork>(adult), "no longer at work");
    assert!(sim.world.res::<PendingChoices>().items.is_empty(), "nobody left to decide");
    assert!(sim.world.has::<Ghost>(adult), "buried at home");
    assert!(sim.world.res::<SimLog>().contains("shark tank"));
    sim.run_days(1);
}

#[test]
fn the_content_has_each_kind_of_accident() {
    let data = GameData::embedded();
    let causes: Vec<&str> = data
        .objects
        .iter()
        .flat_map(|o| &o.interactions)
        .flat_map(|i| &i.outcomes)
        .filter_map(|o| match o {
            Outcome::Accident { cause, .. } => Some(cause.as_str()),
            _ => None,
        })
        .collect();
    assert!(causes.contains(&"electrocution"));
    assert!(causes.contains(&"vending machine"));
    let villain = data.career("supervillain").unwrap();
    assert!(villain.events.iter().flat_map(|e| &e.choices).flat_map(|c| &c.failure).any(|e| matches!(e, CareerEffect::Accident(_, c) if c == "hubris")));
    // anything that can break can be fixed
    for o in &data.objects {
        assert!(o.break_chance == 0.0 || o.interactions.iter().any(|i| i.repairs), "{} breaks for good", o.id);
    }
}

#[test]
fn the_cheerful_can_laugh_too_hard() {
    let mut sim = starter_sim(7);
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    sim.world.res_mut::<Settings>().aging = false;
    let kid = sim.living_peons().into_iter().find(|e| sim.world.get::<Age>(*e).unwrap().stage == LifeStage::Child).unwrap();
    {
        let mut p = sim.world.get_mut::<Personality>(kid).unwrap();
        p.traits.retain(|t| t != "hothead" && t != "gloomy");
        p.traits.push("cheerful".into());
    }
    sim.world.remove::<Student>(kid);
    let toys = find_object(&sim, "toy_box");
    let play = interaction(&sim, toys, "play");
    let mut fits = 0;
    for _ in 0..(4 * 1440) {
        {
            let mut n = sim.world.get_mut::<Needs>(kid).unwrap();
            for k in Need::ALL {
                n.set(k, 100.0);
            }
        }
        if sim.world.get::<ActionQueue>(kid).unwrap().current.is_none() {
            let _ = sim.order(kid, ActionKind::Object { obj: toys, interaction: play });
        }
        sim.tick();
        if sim.world.has::<Dead>(kid) {
            assert_eq!(sim.world.get::<Dead>(kid).unwrap().cause, "laughter");
            return;
        }
        let mut mood = sim.world.get_mut::<Mood>(kid).unwrap();
        if mood.moodlets.iter().any(|m| m.id == "laughing_fit") {
            fits += 1;
            mood.moodlets.retain(|m| m.id != "laughing_fit");
        }
    }
    assert!(fits >= 1, "four days of nonstop fun at full Fun should bring on a laughing fit");
}
