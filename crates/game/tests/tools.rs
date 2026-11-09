use game::state::{Game, Mode, Tool};
use glam::{IVec2, Vec3};
use sim::economy::Household;
use sim::lot::Lot;

#[test]
fn room_tool_builds_and_charges() {
    let mut g = Game::new(sim::starter::starter_sim(1));
    g.mode = Mode::Build;
    g.tool = Tool::Room;
    let funds = g.sim.world.res::<Household>().funds;
    let walls = g.sim.world.res::<Lot>().walls.len();
    // a 3x2 room in the back yard: corners (24,2) -> (27,4)
    g.tool_press(Vec3::new(24.1, 0.0, 2.1));
    g.tool_release(Vec3::new(26.9, 0.0, 3.9));
    let lot = g.sim.world.res::<Lot>();
    assert_eq!(lot.walls.len(), walls + 10);
    assert!(lot.has_floor(0, IVec2::new(25, 3)));
    drop(lot);
    let spent = funds - g.sim.world.res::<Household>().funds;
    assert_eq!(spent, 10 * game::state::COST_WALL + 6 * game::state::COST_FLOOR);
}

#[test]
fn buy_place_and_sell() {
    let mut g = Game::new(sim::starter::starter_sim(1));
    g.mode = Mode::Buy;
    g.tool = Tool::Place { def: "plant_potted".into(), rot: 0 };
    let funds = g.sim.world.res::<Household>().funds;
    g.tool_press(Vec3::new(3.5, 0.0, 3.5));
    assert_eq!(g.sim.world.res::<Household>().funds, funds - 60);
    let e = *g.sim.world.res::<Lot>().occupancy.get(&(0, 3, 3)).expect("placed");
    // can't place on top of it
    assert!(g.sim.can_place("plant_potted", 0, IVec2::new(3, 3), 0).is_err());
    let v = g.sim.sell_object(e).unwrap();
    assert_eq!(v, 60, "unused items refund in full");
    assert!(!g.sim.world.res::<Lot>().occupancy.contains_key(&(0, 3, 3)));
}

#[test]
fn stairs_tool_respects_bounds() {
    let mut g = Game::new(sim::starter::starter_sim(1));
    g.mode = Mode::Build;
    g.tool = Tool::Stairs { dir: sim::lot::Dir4::PosZ };
    let funds = g.sim.world.res::<Household>().funds;
    g.tool_press(Vec3::new(1.5, 0.0, 30.5)); // runs off the lot
    assert_eq!(g.sim.world.res::<Household>().funds, funds, "failed build is refunded");
}
