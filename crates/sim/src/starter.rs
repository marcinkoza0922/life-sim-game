//! A ready-made two-story house and family for quick starts, tests and tools.

use glam::IVec2;
use hearth_ecs::{Entity, World};

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
pub fn build_house(world: &mut World) {
    {
        let mut lot = world.res_mut::<Lot>();
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
        hills(&mut lot, &[(v(4, 25), 3.0, 0.6, crate::lot::TerrainBrush::Raise), (v(28, 26), 2.5, 0.4, crate::lot::TerrainBrush::Raise), (v(25, 4), 2.5, 0.25, crate::lot::TerrainBrush::Lower)]);
    }

    let sim = world;
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

fn place(world: &mut World, id: &str, level: i32, x: i32, z: i32, rot: u8) {
    crate::sim::place_object(world, id, level, v(x, z), rot, false)
        .unwrap_or_else(|e| panic!("starter: placing {id} at ({x},{z}) L{level}: {e}"));
}

fn hills(lot: &mut Lot, bumps: &[(IVec2, f32, f32, crate::lot::TerrainBrush)]) {
    for &(c, r, s, b) in bumps {
        lot.sculpt(c, r, s, b);
        lot.sculpt(c, r + 1.0, 0.0, crate::lot::TerrainBrush::Smooth);
    }
}

/// A one-story bungalow for the neighbors: open living room and kitchen in front, a shared
/// bedroom (double + single bed) and a bathroom at the back.
pub fn build_bungalow(world: &mut World) {
    use crate::lot::TerrainBrush::*;
    {
        let mut lot = world.res_mut::<Lot>();
        *lot = Lot::new(32, 32);
        lot.floor_rect(0, v(10, 16), v(16, 20), FLOOR_WOOD);
        lot.floor_rect(0, v(16, 16), v(20, 20), FLOOR_TILE);
        lot.floor_rect(0, v(10, 12), v(16, 16), FLOOR_CARPET);
        lot.floor_rect(0, v(16, 12), v(20, 16), FLOOR_TILE);
        lot.wall_rect(0, v(10, 12), v(20, 20), WALL_EXTERIOR);
        lot.wall_line(0, v(10, 16), v(20, 16), WALL_INTERIOR);
        lot.wall_line(0, v(16, 12), v(16, 16), WALL_BATH);
        lot.set_opening(Edge::new(0, 14, 20, Axis::X), Opening::Door); // front door
        lot.set_opening(Edge::new(0, 13, 16, Axis::X), Opening::Door); // bedroom
        lot.set_opening(Edge::new(0, 18, 16, Axis::X), Opening::Door); // bathroom
        for (x, z, a) in [(11, 20, Axis::X), (18, 20, Axis::X), (10, 18, Axis::Z), (20, 18, Axis::Z), (12, 12, Axis::X), (20, 14, Axis::Z)] {
            lot.set_opening(Edge::new(0, x, z, a), Opening::Window);
        }
        lot.auto_roof(crate::lot::RoofKind::Hip, 0.45);
        hills(&mut lot, &[(v(26, 8), 3.0, 0.5, Raise), (v(4, 6), 2.5, 0.35, Raise)]);
    }
    let w = world;
    // living room
    place(w, "tv_basic", 0, 11, 16, 0);
    place(w, "sofa_basic", 0, 11, 19, 2);
    place(w, "plant_potted", 0, 10, 19, 0);
    place(w, "floor_lamp", 0, 13, 19, 0);
    place(w, "bookshelf", 0, 15, 16, 0);
    // kitchen
    place(w, "fridge_basic", 0, 19, 16, 3);
    place(w, "stove_basic", 0, 19, 17, 3);
    place(w, "counter_basic", 0, 19, 18, 3);
    place(w, "kitchen_sink", 0, 19, 19, 3);
    place(w, "dining_table", 0, 16, 18, 0);
    place(w, "dining_chair", 0, 16, 17, 0);
    place(w, "dining_chair", 0, 16, 19, 2);
    place(w, "dining_chair", 0, 17, 19, 2);
    place(w, "trash_can", 0, 15, 19, 0);
    // bedroom
    place(w, "bed_double", 0, 10, 12, 0);
    place(w, "end_table", 0, 12, 12, 0);
    place(w, "bed_single", 0, 15, 12, 0);
    // bathroom
    place(w, "toilet_basic", 0, 16, 12, 0);
    place(w, "mirror", 0, 17, 12, 0);
    place(w, "shower_basic", 0, 19, 12, 0);
    // yard
    place(w, "plant_potted", 0, 9, 21, 0);
    place(w, "plant_potted", 0, 21, 21, 0);
}

/// A studio for one or two: a bed in the corner, a galley kitchen, a sofa, and a bathroom
/// boxed into the back.
pub fn build_studio(world: &mut World) {
    use crate::lot::TerrainBrush::*;
    {
        let mut lot = world.res_mut::<Lot>();
        *lot = Lot::new(32, 32);
        lot.floor_rect(0, v(11, 13), v(16, 20), FLOOR_WOOD);
        lot.floor_rect(0, v(16, 16), v(19, 20), FLOOR_WOOD);
        lot.floor_rect(0, v(16, 13), v(19, 16), FLOOR_TILE);
        lot.wall_rect(0, v(11, 13), v(19, 20), WALL_EXTERIOR);
        lot.wall_line(0, v(16, 13), v(16, 16), WALL_BATH);
        lot.wall_line(0, v(16, 16), v(19, 16), WALL_BATH);
        lot.set_opening(Edge::new(0, 15, 20, Axis::X), Opening::Door); // front door
        lot.set_opening(Edge::new(0, 16, 14, Axis::Z), Opening::Door); // bathroom
        for (x, z, a) in [(12, 20, Axis::X), (17, 20, Axis::X), (11, 16, Axis::Z), (19, 18, Axis::Z), (13, 13, Axis::X)] {
            lot.set_opening(Edge::new(0, x, z, a), Opening::Window);
        }
        lot.auto_roof(crate::lot::RoofKind::Gable, 0.45);
        hills(&mut lot, &[(v(5, 6), 3.0, 0.45, Raise), (v(27, 24), 2.5, 0.3, Raise)]);
    }
    let w = world;
    // sleeping corner
    place(w, "bed_double", 0, 11, 13, 0);
    place(w, "end_table", 0, 13, 13, 0);
    place(w, "bookshelf", 0, 14, 13, 0);
    place(w, "plant_potted", 0, 15, 13, 0);
    // bathroom
    place(w, "toilet_basic", 0, 16, 13, 0);
    place(w, "mirror", 0, 17, 13, 0);
    place(w, "shower_basic", 0, 18, 13, 0);
    // galley kitchen
    place(w, "fridge_basic", 0, 18, 16, 3);
    place(w, "stove_basic", 0, 18, 17, 3);
    place(w, "kitchen_sink", 0, 18, 18, 3);
    place(w, "counter_basic", 0, 18, 19, 3);
    place(w, "dining_table", 0, 15, 17, 0);
    place(w, "dining_chair", 0, 15, 16, 0);
    place(w, "dining_chair", 0, 16, 18, 2);
    place(w, "trash_can", 0, 11, 17, 1);
    // lounge
    place(w, "tv_basic", 0, 11, 16, 0);
    place(w, "sofa_basic", 0, 11, 19, 2);
    place(w, "armchair", 0, 13, 19, 2);
    place(w, "floor_lamp", 0, 14, 19, 0);
    // yard
    place(w, "plant_potted", 0, 10, 21, 0);
    place(w, "plant_potted", 0, 20, 21, 0);
}

/// A family cottage: living room and an open kitchen in front; a main bedroom (with a crib),
/// a bathroom with a tub, and a kids' room with two beds at the back.
pub fn build_cottage(world: &mut World) {
    use crate::lot::TerrainBrush::*;
    {
        let mut lot = world.res_mut::<Lot>();
        *lot = Lot::new(32, 32);
        lot.floor_rect(0, v(8, 14), v(15, 20), FLOOR_WOOD);
        lot.floor_rect(0, v(15, 14), v(23, 20), FLOOR_TILE);
        lot.floor_rect(0, v(8, 9), v(13, 14), FLOOR_CARPET);
        lot.floor_rect(0, v(13, 9), v(17, 14), FLOOR_TILE);
        lot.floor_rect(0, v(17, 9), v(23, 14), FLOOR_CARPET);
        lot.wall_rect(0, v(8, 9), v(23, 20), WALL_EXTERIOR);
        lot.wall_line(0, v(8, 14), v(23, 14), WALL_INTERIOR);
        lot.wall_line(0, v(13, 9), v(13, 14), WALL_BATH);
        lot.wall_line(0, v(17, 9), v(17, 14), WALL_BATH);
        lot.set_opening(Edge::new(0, 12, 20, Axis::X), Opening::Door); // front door
        lot.set_opening(Edge::new(0, 11, 14, Axis::X), Opening::Door); // main bedroom
        lot.set_opening(Edge::new(0, 15, 14, Axis::X), Opening::Door); // bathroom
        lot.set_opening(Edge::new(0, 19, 14, Axis::X), Opening::Door); // kids' room
        for (x, z, a) in [
            (10, 20, Axis::X),
            (14, 20, Axis::X),
            (18, 20, Axis::X),
            (21, 20, Axis::X),
            (8, 17, Axis::Z),
            (23, 17, Axis::Z),
            (10, 9, Axis::X),
            (20, 9, Axis::X),
            (8, 11, Axis::Z),
            (23, 11, Axis::Z),
        ] {
            lot.set_opening(Edge::new(0, x, z, a), Opening::Window);
        }
        lot.auto_roof(crate::lot::RoofKind::Gable, 0.5);
        hills(&mut lot, &[(v(27, 5), 3.0, 0.5, Raise), (v(4, 27), 2.5, 0.4, Raise)]);
    }
    let w = world;
    // main bedroom
    place(w, "bed_double", 0, 8, 9, 0);
    place(w, "end_table", 0, 10, 9, 0);
    place(w, "crib", 0, 12, 9, 0);
    place(w, "plant_potted", 0, 8, 13, 0);
    // bathroom
    place(w, "toilet_basic", 0, 13, 9, 0);
    place(w, "mirror", 0, 14, 9, 0);
    place(w, "shower_basic", 0, 16, 9, 0);
    place(w, "bathtub_basic", 0, 13, 11, 0);
    // kids' room
    place(w, "bed_single", 0, 17, 9, 0);
    place(w, "toy_box", 0, 19, 9, 0);
    place(w, "bed_single", 0, 21, 9, 0);
    place(w, "kids_desk", 0, 22, 12, 3);
    // living room
    place(w, "tv_basic", 0, 9, 14, 0);
    place(w, "rug", 0, 9, 15, 0);
    place(w, "sofa_basic", 0, 9, 18, 2);
    place(w, "armchair", 0, 12, 17, 3);
    place(w, "bookshelf", 0, 13, 14, 0);
    place(w, "floor_lamp", 0, 8, 19, 0);
    place(w, "painting", 0, 11, 19, 2);
    place(w, "computer_desk", 0, 14, 19, 2);
    // kitchen / dining
    place(w, "fridge_basic", 0, 22, 14, 3);
    place(w, "stove_basic", 0, 22, 15, 3);
    place(w, "counter_basic", 0, 22, 16, 3);
    place(w, "kitchen_sink", 0, 22, 17, 3);
    place(w, "counter_basic", 0, 22, 18, 3);
    table_for_four(w, 17, 17);
    place(w, "trash_can", 0, 15, 18, 1);
    // yard
    place(w, "plant_potted", 0, 7, 21, 0);
    place(w, "plant_potted", 0, 23, 21, 0);
}

/// An empty plot, ready to build on.
pub fn build_empty(world: &mut World, bumpy: bool) {
    use crate::lot::TerrainBrush::*;
    let mut lot = world.res_mut::<Lot>();
    *lot = Lot::new(32, 32);
    if bumpy {
        hills(&mut lot, &[(v(8, 8), 4.0, 0.7, Raise), (v(24, 12), 3.0, 0.4, Raise), (v(16, 24), 3.0, 0.3, Lower)]);
    } else {
        hills(&mut lot, &[(v(26, 6), 3.0, 0.4, Raise)]);
    }
}

/// The community park: a picnic pavilion with a snack kitchen, a restroom, chess tables, an
/// outdoor "gym", and a little free library.
pub fn build_park(world: &mut World) {
    use crate::lot::TerrainBrush::*;
    {
        let mut lot = world.res_mut::<Lot>();
        *lot = Lot::new(32, 32);
        hills(&mut lot, &[(v(4, 26), 3.0, 0.6, Raise), (v(28, 4), 4.0, 0.8, Raise), (v(27, 26), 2.5, 0.35, Raise)]);
        // restroom
        lot.floor_rect(0, v(4, 6), v(8, 10), FLOOR_TILE);
        lot.wall_rect(0, v(4, 6), v(8, 10), WALL_BATH);
        lot.set_opening(Edge::new(0, 6, 10, Axis::X), Opening::Door);
        lot.set_opening(Edge::new(0, 8, 8, Axis::Z), Opening::Window);
        lot.auto_roof(crate::lot::RoofKind::Hip, 0.4);
        // pavilion (open sides)
        lot.floor_rect(0, v(12, 8), v(22, 14), FLOOR_WOOD);
    }
    let w = world;
    place(w, "toilet_basic", 0, 4, 6, 0);
    place(w, "kitchen_sink", 0, 7, 6, 0);
    place(w, "mirror", 0, 6, 6, 0);
    // pavilion kitchen and tables
    place(w, "fridge_basic", 0, 12, 8, 0);
    place(w, "stove_basic", 0, 13, 8, 0);
    place(w, "counter_basic", 0, 14, 8, 0);
    place(w, "kitchen_sink", 0, 15, 8, 0);
    place(w, "trash_can", 0, 16, 8, 0);
    for x in [13, 18] {
        place(w, "dining_table", 0, x, 11, 0);
        for dx in 0..2 {
            place(w, "dining_chair", 0, x + dx, 10, 0);
            place(w, "dining_chair", 0, x + dx, 12, 2);
        }
    }
    place(w, "floor_lamp", 0, 21, 8, 0);
    // games and the "gym"
    place(w, "chess_table", 0, 8, 19, 0);
    place(w, "chess_table", 0, 12, 19, 0);
    place(w, "exercise_bike", 0, 23, 17, 3);
    place(w, "exercise_bike", 0, 23, 19, 3);
    place(w, "bookshelf", 0, 17, 21, 2);
    place(w, "trash_can", 0, 15, 24, 0);
    for (x, z) in [(3, 15), (10, 4), (20, 4), (26, 14), (9, 24), (20, 26), (11, 27), (21, 22)] {
        place(w, "plant_potted", 0, x, z, 0);
    }
}

/// A dining table at (x, z) with two chairs on each long side.
fn table_for_four(world: &mut World, x: i32, z: i32) {
    place(world, "dining_table", 0, x, z, 0);
    for dx in 0..2 {
        place(world, "dining_chair", 0, x + dx, z - 1, 0);
        place(world, "dining_chair", 0, x + dx, z + 1, 2);
    }
}

/// "The Grumpy Bean": a café with an espresso bar and kitchen along the back wall, tables, a
/// lounge corner with chess and a TV, two restrooms, and a patio out front.
pub fn build_cafe(world: &mut World) {
    use crate::lot::TerrainBrush::*;
    {
        let mut lot = world.res_mut::<Lot>();
        *lot = Lot::new(32, 32);
        hills(&mut lot, &[(v(3, 4), 3.0, 0.5, Raise), (v(28, 5), 2.5, 0.4, Raise)]);
        lot.floor_rect(0, v(7, 8), v(25, 12), FLOOR_TILE);
        lot.floor_rect(0, v(7, 12), v(25, 20), FLOOR_WOOD);
        lot.wall_rect(0, v(7, 8), v(25, 20), WALL_EXTERIOR);
        lot.wall_line(0, v(7, 12), v(14, 12), WALL_BATH);
        lot.wall_line(0, v(11, 8), v(11, 12), WALL_BATH);
        lot.wall_line(0, v(14, 8), v(14, 12), WALL_BATH);
        lot.set_opening(Edge::new(0, 16, 20, Axis::X), Opening::Door); // front door
        lot.set_opening(Edge::new(0, 9, 12, Axis::X), Opening::Door);
        lot.set_opening(Edge::new(0, 12, 12, Axis::X), Opening::Door);
        for x in [8, 10, 12, 14, 18, 20, 22, 24] {
            lot.set_opening(Edge::new(0, x, 20, Axis::X), Opening::Window);
        }
        for (x, z) in [(7, 15), (25, 15), (25, 10)] {
            lot.set_opening(Edge::new(0, x, z, Axis::Z), Opening::Window);
        }
        lot.auto_roof(crate::lot::RoofKind::Flat, 0.0);
    }
    let w = world;
    // restrooms
    place(w, "toilet_basic", 0, 7, 8, 0);
    place(w, "kitchen_sink", 0, 8, 8, 0);
    place(w, "mirror", 0, 10, 8, 0);
    place(w, "toilet_basic", 0, 11, 8, 0);
    place(w, "mirror", 0, 13, 8, 0);
    // the bar
    place(w, "espresso_bar", 0, 16, 8, 0);
    place(w, "espresso_bar", 0, 17, 8, 0);
    place(w, "counter_basic", 0, 18, 8, 0);
    place(w, "fridge_basic", 0, 19, 8, 0);
    place(w, "stove_basic", 0, 20, 8, 0);
    place(w, "counter_basic", 0, 21, 8, 0);
    place(w, "kitchen_sink", 0, 22, 8, 0);
    place(w, "trash_can", 0, 24, 8, 0);
    place(w, "vending_machine", 0, 24, 11, 3);
    // tables
    for (x, z) in [(8, 14), (12, 14), (8, 18), (12, 18)] {
        table_for_four(w, x, z);
    }
    // lounge: chess, a sofa facing the TV, books
    place(w, "chess_table", 0, 17, 14, 0);
    place(w, "chess_table", 0, 17, 17, 0);
    place(w, "tv_basic", 0, 21, 13, 0);
    place(w, "sofa_basic", 0, 20, 18, 2);
    place(w, "armchair", 0, 22, 18, 2);
    place(w, "bookshelf", 0, 24, 13, 3);
    place(w, "painting", 0, 24, 16, 3);
    place(w, "floor_lamp", 0, 7, 13, 0);
    place(w, "floor_lamp", 0, 24, 19, 0);
    for x in [15, 17] {
        place(w, "plant_potted", 0, x, 19, 0);
    }
    // patio
    for x in [9, 20] {
        table_for_four(w, x, 24);
    }
    for (x, z) in [(14, 22), (18, 22), (5, 21), (27, 21)] {
        place(w, "plant_potted", 0, x, z, 0);
    }
}

/// "Sweat Equity Gym": treadmills, bikes and weight benches, two locker rooms with showers,
/// a smoothie corner and a TV lounge for watching other people exercise.
pub fn build_gym(world: &mut World) {
    use crate::lot::TerrainBrush::*;
    {
        let mut lot = world.res_mut::<Lot>();
        *lot = Lot::new(32, 32);
        hills(&mut lot, &[(v(27, 27), 3.0, 0.5, Raise), (v(4, 4), 2.5, 0.35, Raise)]);
        lot.floor_rect(0, v(8, 8), v(14, 13), FLOOR_TILE);
        lot.floor_rect(0, v(14, 8), v(24, 13), FLOOR_CARPET);
        lot.floor_rect(0, v(8, 13), v(24, 20), FLOOR_CARPET);
        lot.wall_rect(0, v(8, 8), v(24, 20), WALL_EXTERIOR);
        lot.wall_line(0, v(8, 13), v(14, 13), WALL_BATH);
        lot.wall_line(0, v(11, 8), v(11, 13), WALL_BATH);
        lot.wall_line(0, v(14, 8), v(14, 13), WALL_BATH);
        lot.set_opening(Edge::new(0, 16, 20, Axis::X), Opening::Door); // front door
        lot.set_opening(Edge::new(0, 9, 13, Axis::X), Opening::Door);
        lot.set_opening(Edge::new(0, 12, 13, Axis::X), Opening::Door);
        for x in [10, 12, 14, 18, 20, 22] {
            lot.set_opening(Edge::new(0, x, 20, Axis::X), Opening::Window);
        }
        for (x, z) in [(24, 11), (24, 16), (8, 16)] {
            lot.set_opening(Edge::new(0, x, z, Axis::Z), Opening::Window);
        }
        lot.set_opening(Edge::new(0, 19, 8, Axis::X), Opening::Window);
        lot.auto_roof(crate::lot::RoofKind::Flat, 0.0);
    }
    let w = world;
    // locker rooms
    place(w, "toilet_basic", 0, 8, 8, 0);
    place(w, "mirror", 0, 9, 8, 0);
    place(w, "shower_basic", 0, 10, 8, 0);
    place(w, "toilet_basic", 0, 11, 8, 0);
    place(w, "mirror", 0, 12, 8, 0);
    place(w, "shower_basic", 0, 13, 8, 0);
    // cardio along the back wall
    for x in [15, 16, 17] {
        place(w, "treadmill", 0, x, 8, 0);
    }
    place(w, "exercise_bike", 0, 19, 8, 0);
    place(w, "exercise_bike", 0, 21, 8, 0);
    // weights
    for x in [17, 19, 21] {
        place(w, "weight_bench", 0, x, 15, 0);
    }
    // smoothie corner
    place(w, "fridge_basic", 0, 23, 12, 3);
    place(w, "espresso_bar", 0, 23, 13, 3);
    place(w, "trash_can", 0, 23, 14, 3);
    place(w, "vending_machine", 0, 23, 17, 3);
    // lounge
    place(w, "tv_basic", 0, 10, 15, 0);
    place(w, "sofa_basic", 0, 10, 18, 2);
    place(w, "floor_lamp", 0, 13, 19, 0);
    place(w, "plant_potted", 0, 8, 19, 0);
    for x in [15, 17] {
        place(w, "plant_potted", 0, x, 19, 0);
    }
    for (x, z) in [(6, 21), (26, 21), (14, 22), (18, 22)] {
        place(w, "plant_potted", 0, x, z, 0);
    }
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
    build_house(&mut sim.world);
    spawn_family(&mut sim);
    sim
}
