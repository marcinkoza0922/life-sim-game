//! School, pregnancy/genetics, adoption, ghosts and crowding on the real content.

use glam::{IVec2, Vec3};
use sim::components::*;
use sim::data::{GameData, LifeStage, Need};
use sim::log::SimLog;
use sim::lot::Lot;
use sim::objects::ObjectInstance;
use sim::starter::starter_sim;
use sim::time::Settings;
use sim::{Entity, Sim};

fn of_stage(sim: &Sim, stage: LifeStage) -> Vec<Entity> {
    sim.living_peons().into_iter().filter(|e| sim.world.get::<Age>(*e).unwrap().stage == stage).collect()
}

fn social(sim: &Sim, id: &str) -> usize {
    sim.data().socials.iter().position(|s| s.id == id).unwrap()
}

#[test]
fn kids_go_to_school_and_get_graded() {
    let mut sim = starter_sim(4);
    sim.world.res_mut::<Settings>().aging = false;
    let kid = of_stage(&sim, LifeStage::Child)[0];
    assert!(sim.world.has::<Student>(kid), "starter child is enrolled");
    // day 0 is a Moonday: watch the bus come and go
    let mut was_at_school = false;
    for _ in 0..(16 * 60) {
        sim.tick();
        was_at_school |= sim.world.get::<AtWork>(kid).is_some_and(|a| a.school);
    }
    assert!(was_at_school, "child should have been at school");
    assert!(!sim.world.has::<AtWork>(kid), "home by 16:00");
    let log = sim.world.res::<SimLog>();
    assert!(log.contains("school bus"), "{:?}", log.entries.iter().map(|e| &e.text).collect::<Vec<_>>());
    drop(log);
    // a week of school produces a report card
    sim.run_days(5);
    assert!(sim.world.res::<SimLog>().contains("report card"));
    let s = sim.world.get::<Student>(kid).unwrap();
    assert!(s.grade != 0.0);
}

#[test]
fn homework_is_done_autonomously_sometimes() {
    let mut sim = starter_sim(12);
    sim.world.res_mut::<Settings>().aging = false;
    let kid = of_stage(&sim, LifeStage::Child)[0];
    let mut done = 0;
    let mut prev = false;
    for _ in 0..(5 * 1440) {
        sim.tick();
        let now = sim.world.get::<Student>(kid).unwrap().homework_done;
        if now && !prev {
            done += 1;
        }
        prev = now;
    }
    assert!(done >= 1, "a child should do homework at least once in five days");
}

#[test]
fn teens_graduate_and_toddlers_enroll() {
    let mut sim = starter_sim(6);
    let kid = of_stage(&sim, LifeStage::Child)[0];
    sim::life::grow_up(&mut sim.world, kid);
    assert!(sim.world.has::<Student>(kid), "teens stay enrolled");
    sim.world.get_mut::<Student>(kid).unwrap().grade = 80.0;
    sim::life::grow_up(&mut sim.world, kid);
    assert!(!sim.world.has::<Student>(kid));
    assert!(sim.world.res::<SimLog>().contains("graduated with honors"));
    let baby = of_stage(&sim, LifeStage::Infant)[0];
    sim::life::grow_up(&mut sim.world, baby);
    assert!(!sim.world.has::<Student>(baby));
    sim::life::grow_up(&mut sim.world, baby);
    assert!(sim.world.has::<Student>(baby), "children start school");
}

#[test]
fn couples_can_have_babies_with_inherited_genes() {
    let mut sim = starter_sim(21);
    sim.world.res_mut::<Settings>().aging = false;
    let adults = of_stage(&sim, LifeStage::Adult);
    let (a, b) = (adults[0], adults[1]);
    // a day off for both, so they're actually home together
    for e in [a, b] {
        sim.world.remove::<Job>(e);
    }
    let before = sim.peons();
    let i = social(&sim, "try_for_baby");
    let mut pregnant = None;
    for _ in 0..24 {
        for e in [a, b] {
            let mut n = sim.world.get_mut::<Needs>(e).unwrap();
            for k in Need::ALL {
                n.set(k, 90.0);
            }
        }
        sim.order(a, ActionKind::Social { target: b, social: i }).ok();
        sim.run_minutes(60);
        pregnant = [a, b].into_iter().find(|e| sim.world.has::<Pregnant>(*e));
        if pregnant.is_some() {
            break;
        }
    }
    let carrier = pregnant.expect("one partner should become pregnant within a day of trying");
    assert!(sim.world.res::<SimLog>().contains("expecting"));
    let due = sim.world.get::<Pregnant>(carrier).unwrap().due;
    let now = sim.clock().tick;
    sim.run_minutes(due - now + 600);
    assert!(!sim.world.has::<Pregnant>(carrier));
    let baby = sim
        .living_peons()
        .into_iter()
        .find(|e| !before.contains(e))
        .expect("a baby was born");
    let age = sim.world.get::<Age>(baby).unwrap();
    assert_eq!(age.stage, LifeStage::Infant);
    let fam = sim.world.get::<Family>(baby).unwrap();
    assert!(fam.parents.contains(&a) && fam.parents.contains(&b));
    // genes come from the parents
    let (ga, gb, gc) = (
        sim.world.get::<Genes>(a).unwrap().clone(),
        sim.world.get::<Genes>(b).unwrap().clone(),
        sim.world.get::<Genes>(baby).unwrap().clone(),
    );
    let lo = ga.height.min(gb.height) - 0.05;
    let hi = ga.height.max(gb.height) + 0.05;
    assert!(gc.height >= lo && gc.height <= hi, "{} not within {lo}..{hi}", gc.height);
    let lo = ga.skin[0].min(gb.skin[0]) - 0.03;
    let hi = ga.skin[0].max(gb.skin[0]) + 0.03;
    assert!(gc.skin[0] >= lo && gc.skin[0] <= hi);
    assert_eq!(sim.world.get::<Appearance>(baby).unwrap().skin, gc.skin);
}

#[test]
fn adoption_costs_money_and_adds_a_kid() {
    let mut sim = starter_sim(2);
    let funds = sim.world.res::<sim::economy::Household>().funds;
    let kid = sim.adopt(LifeStage::Toddler).expect("adopt");
    assert_eq!(sim.world.get::<Age>(kid).unwrap().stage, LifeStage::Toddler);
    assert_eq!(sim.world.res::<sim::economy::Household>().funds, funds - sim::family::ADOPTION_FEE);
    assert!(!sim.world.get::<Family>(kid).unwrap().parents.is_empty());
    assert!(sim.adopt(LifeStage::Adult).is_err());
    while sim.world.res::<sim::economy::Household>().members.len() < sim::family::MAX_HOUSEHOLD {
        sim.adopt(LifeStage::Infant).unwrap();
    }
    assert!(sim.adopt(LifeStage::Infant).is_err(), "household cap");
}

#[test]
fn the_dead_get_a_grave_and_haunt_at_night() {
    let mut sim = starter_sim(31);
    sim.world.res_mut::<Settings>().aging = false;
    let elder = of_stage(&sim, LifeStage::Adult)[0];
    sim::life::die(&mut sim.world, elder, "old age");
    assert!(sim.world.has::<Dead>(elder));
    let ghost = sim.world.get::<Ghost>(elder).expect("ghost").clone();
    let grave = sim.world.get::<ObjectInstance>(ghost.grave).expect("grave object");
    assert_eq!(sim.data().object(grave.def).id, "tombstone");
    drop(grave);
    // sooner or later over a few nights, they show up and cause mischief
    let mut seen = false;
    for _ in 0..(6 * 1440) {
        sim.tick();
        seen |= !sim::ghosts::visible_ghosts(&sim.world).is_empty();
    }
    assert!(seen, "ghost should manifest within six nights");
    assert!(sim.world.res::<SimLog>().contains("ghost"));
    // selling the grave lays them to rest
    sim.sell_object(ghost.grave);
    sim.run_minutes(2);
    assert!(!sim.world.has::<Ghost>(elder));
    assert!(sim.world.res::<SimLog>().contains("moved on"));
}

#[test]
fn peons_route_around_someone_standing_in_a_corridor() {
    let mut sim = Sim::new(5, GameData::embedded());
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    sim.world.res_mut::<Settings>().aging = false;
    let a = sim.random_spec(LifeStage::Adult, None);
    let a = sim.spawn_peon(a, Lot::tile_center(0, IVec2::new(2, 10)));
    let b = sim.random_spec(LifeStage::Adult, None);
    let b = sim.spawn_peon(b, Lot::tile_center(0, IVec2::new(6, 10)));
    for e in [a, b] {
        let mut n = sim.world.get_mut::<Needs>(e).unwrap();
        for k in Need::ALL {
            n.set(k, 95.0);
        }
    }
    sim.order(a, ActionKind::GoTo { level: 0, tile: IVec2::new(10, 10) }).unwrap();
    let mut min_dist = f32::INFINITY;
    for _ in 0..30 {
        sim.tick();
        let pa = sim.world.get::<Transform>(a).unwrap().pos;
        let pb = sim.world.get::<Transform>(b).unwrap().pos;
        min_dist = min_dist.min((pa - pb).length());
    }
    let end = sim.world.get::<Transform>(a).unwrap().pos;
    assert!((end - Lot::tile_center(0, IVec2::new(10, 10))).length() < 0.1, "arrived: {end}");
    assert!(min_dist > 0.6, "walked around B instead of through them (closest {min_dist})");
}

#[test]
fn oncoming_walkers_pass_each_other() {
    let mut sim = Sim::new(5, GameData::embedded());
    sim.world.res_mut::<Settings>().free_will = sim::time::FreeWill::Off;
    sim.world.res_mut::<Settings>().aging = false;
    let a = sim.random_spec(LifeStage::Adult, None);
    let a = sim.spawn_peon(a, Lot::tile_center(0, IVec2::new(2, 10)));
    let b = sim.random_spec(LifeStage::Adult, None);
    let b = sim.spawn_peon(b, Lot::tile_center(0, IVec2::new(12, 10)));
    sim.order(a, ActionKind::GoTo { level: 0, tile: IVec2::new(12, 10) }).unwrap();
    sim.order(b, ActionKind::GoTo { level: 0, tile: IVec2::new(2, 10) }).unwrap();
    let mut min_dist = f32::INFINITY;
    for _ in 0..30 {
        sim.tick();
        let pa = sim.world.get::<Transform>(a).unwrap().pos;
        let pb = sim.world.get::<Transform>(b).unwrap().pos;
        min_dist = min_dist.min((pa - pb).length());
    }
    assert!((sim.world.get::<Transform>(a).unwrap().pos - Lot::tile_center(0, IVec2::new(12, 10))).length() < 0.1);
    assert!((sim.world.get::<Transform>(b).unwrap().pos - Lot::tile_center(0, IVec2::new(2, 10))).length() < 0.1);
    assert!(min_dist > 0.15, "kept some distance while passing ({min_dist})");
    let _ = Vec3::ZERO;
}

#[test]
fn townies_visit_and_go_home() {
    let mut sim = starter_sim(41);
    sim::townies::populate(&mut sim.world, 3);
    let townies = sim::townies::townies(&sim.world);
    assert_eq!(townies.len(), 3);
    assert_eq!(sim.world.res::<sim::economy::Household>().members.len(), 4, "townies aren't household members");
    let (t, _) = townies[0];
    assert!(sim.world.has::<AtWork>(t), "townies start at home");
    sim::townies::invite(&mut sim.world, t);
    // invite at a sensible hour
    while sim.clock().hour() != 10 {
        sim.tick();
    }
    sim::townies::invite(&mut sim.world, t);
    sim.run_minutes(2);
    assert!(sim::townies::visitors(&sim.world).contains(&t), "invited townie arrives");
    assert!(sim.world.res::<SimLog>().contains("from"));
    let mut left = false;
    for _ in 0..(8 * 60) {
        sim.tick();
        if sim.world.has::<AtWork>(t) {
            left = true;
            break;
        }
    }
    assert!(left, "visit ends within 8 hours");
    assert!(sim.world.res::<SimLog>().contains("went home"));
    // a save keeps the neighbors
    let text = sim.save_to_string().unwrap();
    let b = Sim::load_from_str(&text, GameData::embedded()).unwrap();
    assert_eq!(sim::townies::townies(&b.world).len(), 3);
    assert_eq!(b.world.res::<sim::townies::Neighborhood>().lots.len(), 3);
}
