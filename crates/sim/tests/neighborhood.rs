//! Multi-lot neighborhoods: neighbors who really live down the street, switching households,
//! moving, outings to community lots, and life going on off the lot.

use sim::components::*;
use sim::data::{GameData, LifeStage};
use sim::economy::Household;
use sim::log::SimLog;
use sim::lot::Lot;
use sim::neighborhood::{self as hood, LotKind, Neighborhood};
use sim::starter::starter_sim;
use sim::{Entity, Sim};

fn street(seed: u64) -> Sim {
    let mut sim = starter_sim(seed);
    sim.populate_neighborhood();
    sim
}

fn run_until_hour(sim: &mut Sim, h: u32) {
    while sim.clock().hour() != h {
        sim.tick();
    }
}

fn lot_of_kind(sim: &Sim, kind: LotKind, vacant: bool) -> Vec<usize> {
    let n = sim.world.res::<Neighborhood>().lots.len();
    (0..n)
        .filter(|&i| sim.world.res::<Neighborhood>().lots[i].kind == kind && hood::residents(&sim.world, i).is_none() == vacant)
        .collect()
}

/// (walls, floors, object count) of the live lot.
fn shape(sim: &Sim) -> (usize, usize, usize) {
    let lot = sim.world.res::<Lot>();
    (lot.walls.len(), lot.floors.len(), sim.objects().len())
}

fn members(sim: &Sim) -> Vec<Entity> {
    let mut m = sim.world.res::<Household>().members.clone();
    m.sort();
    m
}

#[test]
fn the_street_is_populated() {
    let sim = street(3);
    let h = sim.world.res::<Neighborhood>().clone();
    assert_eq!(h.lots.len(), 9, "home, 4 neighbors, a house for sale, 2 plots, a park");
    assert_eq!(h.households.len(), 4);
    assert_eq!(h.active, 0);
    assert!(h.lots[0].parked.is_none() && h.lots[1..].iter().all(|l| l.parked.is_some()));
    assert_eq!(lot_of_kind(&sim, LotKind::Community, true).len(), 1);
    assert_eq!(hood::vacant_lots(&sim.world).len(), 3);
    for n in hood::neighbors(&sim.world) {
        assert!(sim.world.has::<Offlot>(n) && !sim.world.has::<Member>(n));
        let home = hood::home_of(&sim.world, n).expect("neighbors have a home");
        assert!(h.lots[home].parked.as_ref().unwrap().objects.len() > 15, "and it's furnished");
    }
    assert_eq!(members(&sim).len(), 4, "neighbors aren't household members");
}

#[test]
fn neighbors_visit_and_go_home() {
    let mut sim = street(41);
    let t = hood::neighbors(&sim.world).into_iter().find(|e| sim.world.get::<Age>(*e).unwrap().stage >= LifeStage::Teen).unwrap();
    run_until_hour(&mut sim, 10);
    sim.invite(t);
    sim.run_minutes(2);
    assert!(hood::visitors(&sim.world).contains(&t), "invited neighbor arrives");
    assert!(sim.world.res::<SimLog>().contains("from"));
    let mut left = false;
    for _ in 0..(8 * 60) {
        sim.tick();
        if sim.world.has::<Offlot>(t) {
            left = true;
            break;
        }
    }
    assert!(left, "visit ends within 8 hours");
    assert!(sim.world.res::<SimLog>().contains("went home"));
}

#[test]
fn switching_households_and_back_keeps_both_lots() {
    let mut sim = street(5);
    let home_shape = shape(&sim);
    let ours = members(&sim);
    let funds = sim.world.res::<Household>().funds;
    let theirs_lot = sim.world.res::<Neighborhood>().households[1].home.unwrap();
    let theirs = sim.world.res::<Neighborhood>().households[1].members.clone();

    sim.play_household(theirs_lot).unwrap();
    assert_eq!(hood::active_lot(&sim.world), theirs_lot);
    assert_ne!(shape(&sim), home_shape, "their bungalow is live now");
    for &e in &theirs {
        assert!(sim.world.has::<Member>(e) && !sim.world.has::<Offlot>(e));
    }
    for &e in &ours {
        assert!(!sim.world.has::<Member>(e) && sim.world.has::<Offlot>(e), "the old household went off-lot");
    }
    sim.run_minutes(6 * 60);
    let bungalow = shape(&sim);

    sim.play_household(0).unwrap();
    assert_eq!(shape(&sim), home_shape, "home came back as it was");
    assert_eq!(members(&sim), ours);
    assert!(sim.world.res::<Household>().funds >= funds - 200, "funds kept (give or take a bill)");
    let parked = sim.world.res::<Neighborhood>().lots[theirs_lot].parked.clone().unwrap();
    assert_eq!(parked.objects.len(), bungalow.2, "their lot was parked with its furniture");
    sim.run_days(1);
}

#[test]
fn outings_to_the_park() {
    let mut sim = street(9);
    run_until_hour(&mut sim, 10);
    let home_shape = shape(&sim);
    let park = lot_of_kind(&sim, LotKind::Community, true)[0];
    let going = hood::travelers(&sim.world);
    sim.go_out(park).unwrap();
    assert!(!hood::at_home(&sim.world));
    for &e in &going {
        assert!(!sim.world.has::<Offlot>(e), "the family came along");
        assert!(sim.world.get::<Transform>(e).unwrap().pos.z > 28.0, "arriving at the street");
    }
    let saw_locals = (0..4 * 60).any(|_| {
        sim.tick();
        !hood::visitors(&sim.world).is_empty()
    });
    assert!(saw_locals, "neighbors turn up at the park");
    assert!(sim.world.res::<SimLog>().contains("too."));
    sim.go_home().unwrap();
    assert!(hood::at_home(&sim.world));
    assert_eq!(shape(&sim), home_shape);
    assert!(hood::visitors(&sim.world).is_empty(), "nobody followed them home");
}

#[test]
fn moving_house() {
    let mut sim = street(13);
    let plot = *hood::vacant_lots(&sim.world).iter().find(|&&l| hood::build_value(&sim.world, l) == 0).unwrap();
    let funds = sim.world.res::<Household>().funds;
    let sale = hood::price(&sim.world, 0);
    let cost = hood::price(&sim.world, plot);
    sim.move_household(plot).unwrap();
    assert_eq!(sim.world.res::<Household>().home, Some(plot));
    assert_eq!(sim.world.res::<Household>().funds, funds + sale - cost);
    assert!(hood::vacant_lots(&sim.world).contains(&0), "the old house is for sale");
    assert_eq!(sim.objects().len(), 0, "an empty plot to build on");
    sim.place_object("fridge_basic", 0, glam::IVec2::new(20, 20), 0, true).unwrap();
    sim.run_minutes(60);
}

#[test]
fn a_new_household_moves_in_next_door() {
    let mut sim = street(17);
    let old = members(&sim);
    let house = *hood::vacant_lots(&sim.world).iter().find(|&&l| hood::build_value(&sim.world, l) > 0).unwrap();
    sim.found_household(house, "The Newcomers".into(), 20_000).unwrap();
    for i in 0..2 {
        let spec = sim.random_spec(LifeStage::YoungAdult, Some("Newcomer"));
        let pos = hood::move_in_spot(&sim.world, i);
        sim.spawn_peon(spec, pos);
    }
    assert_eq!(members(&sim).len(), 2);
    assert_eq!(hood::active_lot(&sim.world), house);
    assert_eq!(sim.world.res::<Neighborhood>().households.len(), 5, "the old family are neighbors now");
    for e in old {
        assert!(sim.world.has::<Offlot>(e));
    }
    sim.run_days(1);
}

#[test]
fn life_goes_on_off_the_lot() {
    let mut sim = street(21);
    let before: Vec<(usize, i64)> = sim.world.res::<Neighborhood>().households.iter().map(|h| (h.home.unwrap(), h.funds)).collect();
    let workers: Vec<Entity> = hood::neighbors(&sim.world).into_iter().filter(|e| sim.world.has::<Job>(*e)).collect();
    assert!(!workers.is_empty(), "some neighbors have jobs");
    let days0: u32 = workers.iter().map(|e| sim.world.get::<Job>(*e).unwrap().days_worked).sum();
    let age0 = sim.world.get::<Age>(workers[0]).unwrap().total_days;
    sim.run_days(4);
    let days1: u32 = workers.iter().map(|e| sim.world.get::<Job>(*e).unwrap().days_worked).sum();
    assert!(days1 > days0, "they go to work");
    let after = sim.world.res::<Neighborhood>().households.clone();
    let richer = before.iter().any(|(home, f)| after.iter().any(|h| h.home == Some(*home) && h.funds > *f));
    assert!(richer, "paychecks arrive");
    assert!(sim.world.get::<Age>(workers[0]).unwrap().total_days > age0 + 3.5, "one clock: neighbors age too");
}

#[test]
fn a_household_that_dies_out_frees_its_lot() {
    let mut sim = street(37);
    let (lot, members) = {
        let h = &sim.world.res::<Neighborhood>().households[2];
        (h.home.unwrap(), h.members.clone())
    };
    for e in members {
        sim::life::die(&mut sim.world, e, "old age");
    }
    sim.world.flush();
    assert!(hood::residents(&sim.world, lot).is_none());
    assert!(hood::vacant_lots(&sim.world).contains(&lot));
    assert!(sim.world.res::<SimLog>().contains("up for sale"));
    sim.run_minutes(60);
}

#[test]
fn ghosts_stay_with_their_graves() {
    let mut sim = street(25);
    let spec = sim.random_spec(LifeStage::Elder, None);
    let grandpa = sim.spawn_peon(spec, Lot::tile_center(0, glam::IVec2::new(12, 17)));
    sim::life::die(&mut sim.world, grandpa, "old age");
    sim.world.flush();
    let grave = sim.world.get::<Ghost>(grandpa).expect("buried in the yard").grave;
    assert!(sim.world.has::<sim::objects::ObjectInstance>(grave));
    let park = lot_of_kind(&sim, LotKind::Community, true)[0];
    sim.go_out(park).unwrap();
    sim.run_days(1);
    assert!(sim.world.has::<Ghost>(grandpa), "didn't move on while the grave was parked");
    assert!(sim.world.has::<Offlot>(grandpa));
    sim.go_home().unwrap();
    let grave = sim.world.get::<Ghost>(grandpa).unwrap().grave;
    assert!(sim.world.has::<sim::objects::ObjectInstance>(grave), "the ghost found its grave again");
    assert!(!sim.world.has::<Offlot>(grandpa));
}

#[test]
fn saves_keep_the_whole_street_and_stay_deterministic() {
    let mut a = street(29);
    let other = a.world.res::<Neighborhood>().households[0].home.unwrap();
    a.run_minutes(11 * 60);
    a.play_household(other).unwrap();
    a.run_minutes(3 * 60 + 7);
    let text = a.save_to_string().unwrap();
    let mut b = Sim::load_from_str(&text, GameData::embedded()).unwrap();
    assert_eq!(b.world.res::<Neighborhood>().lots.len(), 9);
    assert_eq!(a.summary(), b.summary());
    a.run_days(2);
    b.run_days(2);
    assert_eq!(a.summary(), b.summary());
    assert_eq!(a.save_to_string().unwrap(), b.save_to_string().unwrap());
    // and back home after loading
    b.play_household(0).unwrap();
    assert_eq!(shape(&b).2, starter_sim(29).objects().len());
}

#[test]
fn old_saves_get_a_street() {
    let mut sim = starter_sim(33);
    // a v1-style neighbor: no house, just an address
    let spec = sim.random_spec(LifeStage::Adult, None);
    let t = sim::sim::spawn_peon_as(&mut sim.world, spec, glam::Vec3::ZERO, false);
    sim.world.insert_one(t, AtWork { until: 500, event_at: u64::MAX, event_done: true, school: false });
    let mut save = sim::save::capture(&sim.world);
    save.hood = None;
    save.neighborhood = Some(hood::OldNeighborhood { name: "Old Gulch".into() });
    save.entities.iter_mut().find(|s| s.id == t).unwrap().townie = Some(hood::OldTownie { address: "3 Sycamore Lane".into(), leave_at: 0, leaving: false });
    let b = sim::save::restore(save, GameData::embedded()).unwrap();
    let h = b.world.res::<Neighborhood>().clone();
    assert_eq!(h.name, "Old Gulch");
    assert_eq!(h.households.len(), 1);
    assert_eq!(h.households[0].members, vec![t]);
    assert!(b.world.has::<Offlot>(t) && !b.world.has::<AtWork>(t));
    assert_eq!(b.world.res::<Household>().home, Some(0));
    assert!(h.lots.len() >= 6);
}
