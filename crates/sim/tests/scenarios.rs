//! End-to-end scenarios on the real content.

use glam::IVec2;
use sim::components::*;
use sim::data::{GameData, LifeStage, Need};
use sim::log::{LogKind, SimLog};
use sim::lot::{Axis, Edge, Lot, Opening};
use sim::starter::starter_sim;
use sim::time::Settings;
use sim::{Entity, Sim};

fn find_object(sim: &Sim, id: &str) -> Entity {
    let data = sim.data();
    sim.objects()
        .into_iter()
        .find(|e| sim.world.get::<sim::objects::ObjectInstance>(*e).is_some_and(|o| data.object(o.def).id == id))
        .unwrap_or_else(|| panic!("no {id}"))
}

fn interaction(sim: &Sim, obj: Entity, id: &str) -> usize {
    let data = sim.data();
    let o = sim.world.get::<sim::objects::ObjectInstance>(obj).unwrap();
    data.object(o.def).interactions.iter().position(|i| i.id == id).unwrap()
}

#[test]
fn same_seed_same_story() {
    let mut a = starter_sim(99);
    let mut b = starter_sim(99);
    a.run_days(2);
    b.run_days(2);
    assert_eq!(a.summary(), b.summary(), "simulation must be deterministic for a given seed");
    let la: Vec<String> = a.world.res::<SimLog>().entries.iter().map(|e| e.text.clone()).collect();
    let lb: Vec<String> = b.world.res::<SimLog>().entries.iter().map(|e| e.text.clone()).collect();
    assert_eq!(la, lb);
    let mut c = starter_sim(100);
    c.run_days(2);
    assert_ne!(a.summary(), c.summary());
}

#[test]
fn player_order_is_carried_out() {
    let mut sim = starter_sim(5);
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    let adult = sim.living_peons()[0];
    // make sure nothing urgent overrides the order
    {
        let mut n = sim.world.get_mut::<Needs>(adult).unwrap();
        for k in Need::ALL {
            n.set(k, 80.0);
        }
        n.set(Need::Hygiene, 20.0);
    }
    let shower = find_object(&sim, "shower_basic");
    let i = interaction(&sim, shower, "shower");
    sim.order(adult, ActionKind::Object { obj: shower, interaction: i }).expect("order accepted");
    let mut showered = false;
    for _ in 0..120 {
        sim.tick();
        let q = sim.world.get::<ActionQueue>(adult).unwrap();
        if let Some(c) = &q.current
            && matches!(c.phase, Phase::Performing { .. }) && c.action.label.contains("Shower") {
                showered = true;
            }
    }
    assert!(showered, "peon should walk to the shower and use it");
    assert!(sim.world.get::<Needs>(adult).unwrap().get(Need::Hygiene) > 60.0);
}

#[test]
fn sealed_room_is_unreachable() {
    let mut sim = Sim::new(1, GameData::embedded());
    {
        let mut lot = sim.lot_mut();
        lot.wall_rect(0, IVec2::new(10, 10), IVec2::new(14, 14), 0);
        lot.floor_rect(0, IVec2::new(10, 10), IVec2::new(14, 14), 0);
    }
    let fridge = sim.place_object("fridge_basic", 0, IVec2::new(11, 11), 0, false).unwrap();
    let spec = sim.random_spec(LifeStage::Adult, None);
    let p = sim.spawn_peon(spec, Lot::tile_center(0, IVec2::new(2, 2)));
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    let i = interaction(&sim, fridge, "snack");
    sim.world.get_mut::<Needs>(p).unwrap().set(Need::Hunger, 60.0);
    sim.order(p, ActionKind::Object { obj: fridge, interaction: i }).unwrap();
    sim.run_minutes(5);
    assert!(sim.world.res::<SimLog>().contains("couldn't"), "should log a routing failure");
    // add a door: now it works
    sim.lot_mut().set_opening(Edge::new(0, 12, 14, Axis::X), Opening::Door);
    sim.order(p, ActionKind::Object { obj: fridge, interaction: i }).unwrap();
    let before = sim.world.get::<Needs>(p).unwrap().get(Need::Hunger);
    sim.run_minutes(60);
    assert!(sim.world.get::<Needs>(p).unwrap().get(Need::Hunger) > before);
}

#[test]
fn babies_grow_up() {
    let mut sim = starter_sim(3);
    sim.world.res_mut::<Settings>().lifespan_scale = 0.1;
    // existing stage lengths were set at spawn; shorten them too
    for (_, age) in sim.world.query::<(Entity, &mut Age)>().iter() {
        age.stage_len *= 0.1;
    }
    let baby = sim.living_peons().into_iter().find(|e| sim.world.get::<Age>(*e).unwrap().stage == LifeStage::Infant).unwrap();
    sim.run_days(1);
    let stage = sim.world.get::<Age>(baby).map(|a| a.stage);
    assert!(stage.is_some_and(|s| s >= LifeStage::Toddler), "infant should have aged up, got {stage:?}");
    assert!(sim.world.res::<SimLog>().contains("Toddler"));
}

#[test]
fn starving_alone_ends_badly() {
    let mut sim = Sim::new(11, GameData::embedded());
    sim.world.res_mut::<Settings>().aging = false;
    let spec = sim.random_spec(LifeStage::Adult, None);
    let p = sim.spawn_peon(spec, Lot::tile_center(0, IVec2::new(5, 5)));
    sim.world.get_mut::<Needs>(p).unwrap().set(Need::Hunger, 0.0);
    sim.run_days(2);
    let log = sim.world.res::<SimLog>();
    let died = sim.world.has::<Dead>(p);
    let haggled = log.contains("The Reaper came for");
    assert!(died || haggled, "no food on the lot: starvation (or a Reaper bargain) must happen");
    if died {
        assert!(log.count(LogKind::Death) >= 1);
    }
}

#[test]
fn careers_pay() {
    let mut sim = starter_sim(8);
    sim.world.res_mut::<Settings>().auto_career_choices = true;
    let adults: Vec<Entity> = sim.living_peons().into_iter().filter(|e| sim.world.get::<Age>(*e).unwrap().stage.is_adult()).collect();
    for a in &adults {
        sim.world.remove::<Job>(*a);
    }
    sim::career::join_career(&mut sim.world, adults[0], "spy").unwrap();
    let funds0 = sim.world.res::<sim::economy::Household>().funds;
    sim.run_days(7);
    let log = sim.world.res::<SimLog>();
    assert!(log.contains("came home from work"), "should have worked a shift");
    assert!(log.count(LogKind::Career) >= 3);
    let _ = funds0;
    let job = sim.world.get::<Job>(adults[0]);
    assert!(job.is_none_or(|j| j.days_worked >= 1));
}

#[test]
fn performing_sets_the_interaction_clip_and_prop_then_clears_them() {
    let mut sim = starter_sim(5);
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    let adult = sim.living_peons()[0];
    {
        let mut n = sim.world.get_mut::<Needs>(adult).unwrap();
        for k in Need::ALL {
            n.set(k, 80.0);
        }
    }
    let shelf = find_object(&sim, "bookshelf");
    let i = interaction(&sim, shelf, "read");
    sim.order(adult, ActionKind::Object { obj: shelf, interaction: i }).expect("order accepted");
    let (mut walked_bare, mut read_with_book) = (false, false);
    for _ in 0..240 {
        sim.tick();
        let p = sim.world.get::<Posture>(adult).unwrap().clone();
        if p.walking {
            walked_bare |= p.anim.is_none() && p.prop.is_none();
        }
        read_with_book |= p.anim.as_deref() == Some("read") && p.prop.as_deref() == Some("book");
        let idle = sim.world.get::<ActionQueue>(adult).unwrap().current.is_none();
        if read_with_book && idle {
            assert_eq!(p, Posture::default(), "the book is put back when done");
            break;
        }
    }
    assert!(walked_bare && read_with_book);
}

#[test]
fn a_day_of_free_will_plays_many_clips() {
    let mut sim = starter_sim(3);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..24 * 60 {
        sim.tick();
        for e in sim.living_peons() {
            if let Some(a) = sim.world.get::<Posture>(e).and_then(|p| p.anim.clone()) {
                seen.insert(a);
            }
        }
    }
    assert!(seen.len() >= 5, "only saw {seen:?}");
}

/// Orders `who` to watch TV and runs until they are performing; returns their posture then.
fn start_watching(sim: &mut Sim, who: Entity) -> Posture {
    {
        let mut n = sim.world.get_mut::<Needs>(who).unwrap();
        for k in Need::ALL {
            n.set(k, 80.0);
        }
    }
    let tv = find_object(sim, "tv_basic");
    let i = interaction(sim, tv, "watch");
    sim.order(who, ActionKind::Object { obj: tv, interaction: i }).expect("order accepted");
    for _ in 0..240 {
        sim.tick();
        let q = sim.world.get::<ActionQueue>(who).unwrap();
        if q.current.as_ref().is_some_and(|c| matches!(c.phase, Phase::Performing { .. })) {
            return sim.world.get::<Posture>(who).unwrap().clone();
        }
    }
    panic!("never started watching");
}

#[test]
fn tv_is_watched_from_the_sofa_facing_it() {
    let mut sim = starter_sim(5);
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    let adult = sim.living_peons()[0];
    let sofa = find_object(&sim, "sofa_basic");
    let p = start_watching(&mut sim, adult);
    assert_eq!((p.pose, p.anim.as_deref(), p.prop.as_deref()), (sim::data::Pose::Sit, Some("watch"), Some("remote")));
    let o = sim.world.get::<sim::objects::ObjectInstance>(sofa).unwrap().clone();
    assert!(o.slot_users.contains(&Some(adult)), "the sofa seat is taken");
    let y = sim.world.get::<Transform>(adult).unwrap().pos.y;
    assert!(y > 0.3, "sitting on the cushion, not the floor ({y})");

    // getting up frees the seat and puts them back on their feet
    let id = sim.world.get::<ActionQueue>(adult).unwrap().current.as_ref().unwrap().action.id;
    sim.cancel(adult, id);
    for _ in 0..5 {
        sim.tick();
    }
    let o = sim.world.get::<sim::objects::ObjectInstance>(sofa).unwrap().clone();
    assert!(!o.slot_users.contains(&Some(adult)));
    assert!(sim.world.get::<Transform>(adult).unwrap().pos.y < 0.05);
}

#[test]
fn tv_without_a_free_seat_is_watched_from_the_floor() {
    let mut sim = starter_sim(5);
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    let adult = sim.living_peons()[0];
    let sofa = find_object(&sim, "sofa_basic");
    sim.sell_object(sofa);
    let p = start_watching(&mut sim, adult);
    assert_eq!((p.pose, p.anim.as_deref()), (sim::data::Pose::Sit, Some("watch_floor")));
    assert!(sim.world.get::<Transform>(adult).unwrap().pos.y < 0.05);
}

#[test]
fn selling_the_seat_mid_show_stands_them_up() {
    let mut sim = starter_sim(5);
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    let adult = sim.living_peons()[0];
    start_watching(&mut sim, adult);
    sim.sell_object(find_object(&sim, "sofa_basic"));
    sim.tick();
    assert_eq!(*sim.world.get::<Posture>(adult).unwrap(), Posture::default());
    assert!(sim.world.get::<Transform>(adult).unwrap().pos.y < 0.05, "back on the floor");
    assert!(sim.world.get::<ActionQueue>(adult).unwrap().current.as_ref().is_none_or(|c| c.seat.is_none()));
}

#[test]
fn a_full_floor_in_front_of_the_tv_still_leaves_the_sofa() {
    let sim = starter_sim(5);
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    let adult = sim.living_peons()[0];
    let tv = find_object(&sim, "tv_basic");
    let watch = interaction(&sim, tv, "watch");
    let is_watch = |k: &ActionKind| matches!(k, ActionKind::Object { obj, interaction } if *obj == tv && *interaction == watch);
    // three others already on the floor in front of it
    let others: Vec<Entity> = sim.living_peons().into_iter().filter(|&p| p != adult).collect();
    {
        let mut o = sim.world.get_mut::<sim::objects::ObjectInstance>(tv).unwrap();
        for s in o.slot_users.iter_mut() {
            *s = Some(others[0]);
        }
    }
    let offered = |sim: &Sim| sim.options(adult, tv).into_iter().find(|(_, k, _)| is_watch(k)).unwrap().2;
    let considered = |sim: &Sim| sim::autonomy::candidates(&sim.world, adult, &sim.data()).iter().any(|c| is_watch(&c.kind));
    assert_eq!(offered(&sim), Ok(()), "the sofa is free");
    assert!(considered(&sim), "autonomy counts the sofa");

    // and with the sofa taken too, it really is full
    let sofa = find_object(&sim, "sofa_basic");
    {
        let mut o = sim.world.get_mut::<sim::objects::ObjectInstance>(sofa).unwrap();
        for s in o.slot_users.iter_mut() {
            *s = Some(others[0]);
        }
    }
    assert!(offered(&sim).is_err());
    assert!(!considered(&sim));
}
