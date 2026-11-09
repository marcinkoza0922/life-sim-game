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
    assert_eq!(h.lots.len(), 11, "home, 4 neighbors, a house for sale, 2 plots, a park, a café, a gym");
    assert_eq!(h.households.len(), 4);
    assert_eq!(h.active, 0);
    assert!(h.lots[0].parked.is_none() && h.lots[1..].iter().all(|l| l.parked.is_some()));
    assert_eq!(lot_of_kind(&sim, LotKind::Community, true).len(), 3);
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
    assert_eq!(b.world.res::<Neighborhood>().lots.len(), 11);
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

fn find_object(sim: &Sim, id: &str) -> Entity {
    let data = sim.data();
    sim.objects()
        .into_iter()
        .find(|e| sim.world.get::<sim::objects::ObjectInstance>(*e).is_some_and(|o| data.object(o.def).id == id))
        .unwrap_or_else(|| panic!("no {id}"))
}

fn community(sim: &Sim, name: &str) -> usize {
    sim.world.res::<Neighborhood>().lots.iter().position(|l| l.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no {name}"))
}

/// Beds per person a parked lot sleeps (double beds count twice, cribs once).
fn sleeps(sim: &Sim, lot: usize) -> usize {
    let hood = sim.world.res::<Neighborhood>();
    hood.lots[lot].parked.as_ref().unwrap().objects.iter().map(|o| match o.def.as_str() {
        "bed_double" => 2,
        "bed_single" | "crib" => 1,
        _ => 0,
    }).sum::<usize>()
}

#[test]
fn houses_fit_their_households() {
    let sim = street(45);
    let households = sim.world.res::<Neighborhood>().households.clone();
    let mut plans = std::collections::BTreeSet::new();
    for h in households {
        let lot = h.home.unwrap();
        assert!(sleeps(&sim, lot) >= h.members.len(), "{} have a bed each", h.name);
        plans.insert(shape_of(&sim, lot));
    }
    assert!(plans.len() >= 3, "studio, bungalow and cottage all appear on the street");
}

fn shape_of(sim: &Sim, lot: usize) -> (usize, usize) {
    let hood = sim.world.res::<Neighborhood>();
    let p = hood.lots[lot].parked.as_ref().unwrap();
    (p.lot.walls.len(), p.objects.len())
}

#[test]
fn visiting_the_neighbors() {
    let mut sim = street(49);
    run_until_hour(&mut sim, 10);
    let home_shape = shape(&sim);
    let (lot, hosts) = {
        let h = sim.world.res::<Neighborhood>().households.iter().find(|h| h.members.len() >= 2).cloned().unwrap();
        (h.home.unwrap(), h.members)
    };
    let going = hood::travelers(&sim.world);
    sim.visit(lot).unwrap();
    assert_eq!(hood::active_lot(&sim.world), lot);
    assert!(hood::visiting(&sim.world) && !hood::at_home(&sim.world));
    let present = sim::sim::present_peons(&sim.world);
    for e in going.iter().chain(&hosts) {
        assert!(present.contains(e), "guests and hosts are both there");
    }
    for &e in &hosts {
        assert!(!sim.world.has::<Visitor>(e), "hosts aren't visitors in their own home");
        let pos = sim.world.get::<Transform>(e).unwrap().pos;
        assert!(sim.world.res::<Lot>().has_floor(0, glam::IVec2::new(pos.x as i32, pos.z as i32)), "hosts start indoors");
    }
    assert!(sim.world.res::<SimLog>().contains("dropped in on"));
    assert!(sim.visit(lot).is_err(), "already there");
    sim.run_minutes(3 * 60);
    sim.go_home().unwrap();
    assert_eq!(shape(&sim), home_shape);
    for &e in &hosts {
        assert!(sim.world.has::<Offlot>(e), "hosts stay home when the guests leave");
    }
    // nobody drops in at midnight
    run_until_hour(&mut sim, 23);
    assert!(sim.visit(lot).is_err());
    assert!(hood::at_home(&sim.world));
}

#[test]
fn hosts_at_work_come_home_to_their_own_lot() {
    let mut sim = street(53);
    run_until_hour(&mut sim, 10);
    let (lot, host) = {
        let h = &sim.world.res::<Neighborhood>().households[1];
        (h.home.unwrap(), h.members[0])
    };
    sim.visit(lot).unwrap();
    let until = sim.clock().tick + 30;
    sim.world.insert_one(host, AtWork { until, event_at: u64::MAX, event_done: true, school: false });
    sim.go_home().unwrap();
    assert!(sim.world.has::<Offlot>(host), "they'll come home to their place, not ours");
    sim.run_minutes(90);
    assert!(!sim::sim::present_peons(&sim.world).contains(&host) || sim.world.has::<Visitor>(host));
}

#[test]
fn newcomers_move_in_when_the_street_empties() {
    let mut sim = street(57);
    for _ in 0..2 {
        let members = sim.world.res::<Neighborhood>().households[0].members.clone();
        for e in members {
            sim::life::die(&mut sim.world, e, "old age");
        }
        sim.world.flush();
    }
    assert_eq!(hood::household_count(&sim.world), 3);
    sim.run_days(12);
    assert!(hood::household_count(&sim.world) >= 4, "someone moved in");
    assert!(sim.world.res::<SimLog>().contains("moving van"));
    assert!(!hood::vacant_lots(&sim.world).is_empty(), "a lot is always left for sale");
    for h in sim.world.res::<Neighborhood>().households.iter() {
        for &e in &h.members {
            assert!(sim.world.has::<Offlot>(e) || sim.world.has::<Visitor>(e) || sim.world.has::<Dead>(e));
        }
    }
}

#[test]
fn newcomers_build_on_an_empty_plot() {
    let mut sim = street(61);
    let plot = *hood::vacant_lots(&sim.world).iter().find(|&&l| hood::build_value(&sim.world, l) == 0).unwrap();
    use LifeStage::*;
    hood::move_in_newcomers(&mut sim.world, plot, &[Adult, Adult, Child, Toddler], true);
    let r = hood::residents(&sim.world, plot).expect("they live there now");
    assert_eq!(r.members.len(), 4);
    assert!(hood::build_value(&sim.world, plot) > 0, "and built a house");
    assert!(sleeps(&sim, plot) >= 4, "a cottage, with room for the kids");
    sim.run_days(1);
}

#[test]
fn lattes_at_the_cafe_and_reps_at_the_gym() {
    let mut sim = street(65);
    run_until_hour(&mut sim, 10);
    sim.world.res_mut::<sim::time::Settings>().free_will = sim::time::FreeWill::Off;
    let adult = members(&sim).into_iter().find(|e| sim.world.get::<Age>(*e).unwrap().stage.is_adult()).unwrap();
    for (place, obj, inter) in [("The Grumpy Bean", "espresso_bar", "latte"), ("Sweat Equity Gym", "treadmill", "run"), ("Sweat Equity Gym", "weight_bench", "lift")] {
        let lot = community(&sim, place);
        if hood::active_lot(&sim.world) != lot {
            if !hood::at_home(&sim.world) {
                sim.go_home().unwrap();
            }
            sim.go_out(lot).unwrap();
        }
        let o = find_object(&sim, obj);
        let i = {
            let data = sim.data();
            let inst = sim.world.get::<sim::objects::ObjectInstance>(o).unwrap();
            data.object(inst.def).interactions.iter().position(|x| x.id == inter).unwrap()
        };
        {
            let mut n = sim.world.get_mut::<Needs>(adult).unwrap();
            for k in sim::data::Need::ALL {
                n.set(k, 80.0);
            }
            n.set(sim::data::Need::Energy, 40.0);
        }
        let funds = sim.world.res::<Household>().funds;
        sim.order(adult, ActionKind::Object { obj: o, interaction: i }).unwrap();
        let mut did = false;
        for _ in 0..90 {
            sim.tick();
            let q = sim.world.get::<ActionQueue>(adult).unwrap();
            if q.current.as_ref().is_some_and(|c| matches!(c.action.kind, ActionKind::Object { obj, interaction } if obj == o && interaction == i) && matches!(c.phase, Phase::Performing { .. })) {
                did = true;
                break;
            }
        }
        assert!(did, "{obj}: {inter} gets done");
        if obj == "espresso_bar" {
            assert_eq!(sim.world.res::<Household>().funds, funds - 6, "a latte costs $6");
        }
    }
}

#[test]
fn neighbors_pay_for_their_own_coffee() {
    let sim = street(69);
    let n = hood::neighbors(&sim.world)[0];
    let ours = sim.world.res::<Household>().funds;
    let theirs = |sim: &Sim| sim.world.res::<Neighborhood>().households.iter().find(|h| h.members.contains(&n)).unwrap().funds;
    let before = theirs(&sim);
    assert!(sim::economy::debit(&sim.world, n, 6));
    assert_eq!(theirs(&sim), before - 6);
    assert_eq!(sim.world.res::<Household>().funds, ours);
    assert!(!sim::economy::debit(&sim.world, n, i64::MAX), "can't afford it");
}

#[test]
fn old_streets_get_the_new_community_lots() {
    let sim = street(73);
    let mut save = sim::save::capture(&sim.world);
    let hood_save = save.hood.as_mut().unwrap();
    hood_save.lots.retain(|l| l.name.as_deref() != Some("The Grumpy Bean") && l.name.as_deref() != Some("Sweat Equity Gym"));
    let b = sim::save::restore(save, GameData::embedded()).unwrap();
    let names: Vec<String> = b.world.res::<Neighborhood>().lots.iter().filter_map(|l| l.name.clone()).collect();
    assert_eq!(names, ["Gulch Green", "The Grumpy Bean", "Sweat Equity Gym"]);
}
