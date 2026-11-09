//! A ready-made two-story house and family for quick starts, tests and tools.

use glam::IVec2;
use hearth_ecs::Entity;

use crate::data::LifeStage;
use crate::lot::{Axis, Dir4, Edge, Lot, Opening, Stair};
use crate::sim::Sim;

pub const WALL_EXTERIOR: u16 = 0;
pub const WALL_INTERIOR: u16 = 1;
pub const WALL_BATH: u16 = 2;
pub const FLOOR_WOOD: u16 = 0;
pub const FLOOR_TILE: u16 = 1;
pub const FLOOR_CARPET: u16 = 2;

fn v(x: i32, z: i32) -> IVec2 {
    IVec2::new(x, z)
}

/// Builds the starter house on the sim's lot. Ground floor: living room, kitchen/dining,
/// bathroom, master bedroom, stair hall. Upstairs: kids' room and landing. Basement: rec room.
pub fn build_house(sim: &mut Sim) {
    {
        let mut lot = sim.lot_mut();
        *lot = Lot::new(32, 32);
        // ---- ground floor shell
        lot.floor_rect(0, v(8, 13), v(16, 20), FLOOR_WOOD);
        lot.floor_rect(0, v(16, 13), v(22, 20), FLOOR_TILE);
        lot.floor_rect(0, v(8, 8), v(12, 13), FLOOR_TILE);
        lot.floor_rect(0, v(12, 8), v(18, 13), FLOOR_CARPET);
        lot.floor_rect(0, v(18, 8), v(22, 13), FLOOR_WOOD);
        lot.wall_rect(0, v(8, 8), v(22, 20), WALL_EXTERIOR);
        // living | kitchen, with a wide opening
        lot.wall_line(0, v(16, 13), v(16, 15), WALL_INTERIOR);
        lot.wall_line(0, v(16, 18), v(16, 20), WALL_INTERIOR);
        // north rooms | south rooms
        lot.wall_line(0, v(8, 13), v(19, 13), WALL_INTERIOR);
        lot.wall_line(0, v(21, 13), v(22, 13), WALL_INTERIOR);
        // bath | bedroom | hall
        lot.wall_line(0, v(12, 8), v(12, 13), WALL_BATH);
        lot.wall_line(0, v(18, 8), v(18, 13), WALL_INTERIOR);
        // doors
        lot.set_opening(Edge::new(0, 12, 20, Axis::X), Opening::Door); // front door
        lot.set_opening(Edge::new(0, 10, 13, Axis::X), Opening::Door); // bathroom
        lot.set_opening(Edge::new(0, 14, 13, Axis::X), Opening::Door); // bedroom
        lot.set_opening(Edge::new(0, 22, 17, Axis::Z), Opening::Door); // kitchen back door
        // windows
        for (x, z, a) in [(10, 20, Axis::X), (14, 20, Axis::X), (18, 20, Axis::X), (20, 20, Axis::X), (8, 16, Axis::Z), (22, 15, Axis::Z), (15, 8, Axis::X), (9, 8, Axis::X), (22, 10, Axis::Z)] {
            lot.set_opening(Edge::new(0, x, z, a), Opening::Window);
        }
        // ---- stairs (hall) up to the second floor
        lot.floor_rect(1, v(12, 8), v(22, 13), FLOOR_CARPET);
        lot.wall_rect(1, v(12, 8), v(22, 13), WALL_EXTERIOR);
        lot.wall_line(1, v(18, 8), v(18, 13), WALL_INTERIOR);
        lot.set_opening(Edge::new(1, 18, 11, Axis::Z), Opening::Door);
        lot.set_opening(Edge::new(1, 15, 8, Axis::X), Opening::Window);
        lot.set_opening(Edge::new(1, 15, 13, Axis::X), Opening::Window);
        lot.set_opening(Edge::new(1, 22, 10, Axis::Z), Opening::Window);
        lot.add_stair(Stair { level: 0, start: v(19, 9), dir: Dir4::PosZ }).expect("upstairs");
        // ---- basement rec room under the living room, stairs down from the hall
        lot.dig_basement(-1, v(8, 13), v(16, 20), WALL_INTERIOR, FLOOR_CARPET);
        // stairs from basement (bottom access 15,19 on -1) rising -z to the living room
        lot.add_stair(Stair { level: -1, start: v(15, 18), dir: Dir4::NegZ }).expect("downstairs");
        lot.auto_roof(crate::lot::RoofKind::Gable, 0.5);
        // a gentle hill in the back yard and a dip by the street, so the terrain isn't a pancake
        for (c, r, s, b) in [(v(4, 25), 3.0, 0.6, crate::lot::TerrainBrush::Raise), (v(28, 26), 2.5, 0.4, crate::lot::TerrainBrush::Raise), (v(25, 4), 2.5, 0.25, crate::lot::TerrainBrush::Lower)] {
            lot.sculpt(c, r, s, b);
            lot.sculpt(c, r + 1.0, 0.0, crate::lot::TerrainBrush::Smooth);
        }
    }

    let place = |sim: &mut Sim, id: &str, level: i32, x: i32, z: i32, rot: u8| {
        sim.place_object(id, level, v(x, z), rot, false)
            .unwrap_or_else(|e| panic!("starter: placing {id} at ({x},{z}) L{level}: {e}"));
    };
    // living room (stair hole for basement at x=15, z=16..18 on level 0)
    place(sim, "tv_basic", 0, 10, 14, 0);
    place(sim, "sofa_basic", 0, 10, 18, 2);
    place(sim, "rug", 0, 10, 15, 0);
    place(sim, "armchair", 0, 13, 17, 3);
    place(sim, "plant_potted", 0, 8, 19, 0);
    place(sim, "floor_lamp", 0, 8, 13, 0);
    place(sim, "bookshelf", 0, 12, 13, 0);
    place(sim, "painting", 0, 9, 19, 2);
    // kitchen / dining
    place(sim, "fridge_basic", 0, 21, 13, 3);
    place(sim, "stove_basic", 0, 21, 14, 3);
    place(sim, "counter_basic", 0, 21, 15, 3);
    place(sim, "kitchen_sink", 0, 21, 16, 3);
    place(sim, "dining_table", 0, 17, 17, 0);
    place(sim, "dining_chair", 0, 17, 16, 0);
    place(sim, "dining_chair", 0, 18, 16, 0);
    place(sim, "dining_chair", 0, 17, 18, 2);
    place(sim, "dining_chair", 0, 18, 18, 2);
    place(sim, "trash_can", 0, 16, 19, 0);
    // bathroom
    place(sim, "toilet_basic", 0, 8, 8, 0);
    place(sim, "shower_basic", 0, 11, 8, 0);
    place(sim, "mirror", 0, 9, 8, 0);
    // master bedroom
    place(sim, "bed_double", 0, 13, 8, 0);
    place(sim, "crib", 0, 17, 8, 0);
    place(sim, "end_table", 0, 16, 8, 0);
    place(sim, "computer_desk", 0, 17, 12, 2);
    // upstairs kids' room
    place(sim, "bed_single", 1, 12, 8, 0);
    place(sim, "toy_box", 1, 14, 8, 0);
    place(sim, "chess_table", 1, 15, 11, 0);
    place(sim, "kids_desk", 1, 16, 8, 0);
    place(sim, "plant_potted", 1, 21, 8, 0);
    // basement rec room
    place(sim, "exercise_bike", -1, 8, 13, 0);
    place(sim, "floor_lamp", -1, 8, 15, 0);
    place(sim, "sofa_basic", -1, 10, 19, 2);
    // yard
    place(sim, "plant_potted", 0, 6, 21, 0);
    place(sim, "plant_potted", 0, 23, 21, 0);
}

/// Two adults (married), a child and an infant.
pub fn spawn_family(sim: &mut Sim) -> Vec<Entity> {
    // spawn each before generating the next so first names stay unique
    let mut a_spec = sim.random_spec(LifeStage::Adult, None);
    let last = a_spec.last.clone();
    a_spec.skills.push((crate::data::Skill::Cooking, 2.0));
    let a = sim.spawn_peon(a_spec, Lot::tile_center(0, v(11, 16)));
    let b_spec = sim.random_spec(LifeStage::Adult, Some(&last));
    let b = sim.spawn_peon(b_spec, Lot::tile_center(0, v(13, 16)));
    let c_spec = sim.random_spec(LifeStage::Child, Some(&last));
    let c = sim.spawn_peon(c_spec, Lot::tile_center(0, v(12, 15)));
    let d_spec = sim.random_spec(LifeStage::Infant, Some(&last));
    let d = sim.spawn_peon(d_spec, Lot::tile_center(0, v(14, 15)));
    sim.marry(a, b);
    for kid in [c, d] {
        sim.set_parent(a, kid);
        sim.set_parent(b, kid);
    }
    sim.world.res_mut::<crate::social::Relationships>().add(c, d, 30.0, 0.0);
    sim.world.res_mut::<crate::economy::Household>().name = format!("The {} Family", last);
    vec![a, b, c, d]
}

/// A complete starter game.
pub fn starter_sim(seed: u64) -> Sim {
    let mut sim = Sim::new(seed, crate::data::GameData::embedded());
    build_house(&mut sim);
    spawn_family(&mut sim);
    sim
}
