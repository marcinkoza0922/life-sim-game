//! Placed object instances and footprint/slot geometry.

use glam::{IVec2, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::data::{ObjectDef, ObjectId};
use crate::lot::{LEVEL_HEIGHT, Lot};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectInstance {
    pub def: ObjectId,
    /// Min corner tile of the rotated footprint.
    pub tile: IVec2,
    pub level: i32,
    /// Quarter turns (0–3).
    pub rot: u8,
    pub dirt: f32,
    pub broken: bool,
    pub slot_users: Vec<Option<hearth_ecs::Entity>>,
    pub uses: u32,
    pub price_paid: i64,
    /// Height of the ground it stands on above the level's floor (outdoor terrain).
    #[serde(default)]
    pub ground: f32,
}

/// Footprint size after rotation.
pub fn rotated_size(def: &ObjectDef, rot: u8) -> IVec2 {
    let (w, d) = (def.footprint.0 as i32, def.footprint.1 as i32);
    if rot % 2 == 1 { IVec2::new(d, w) } else { IVec2::new(w, d) }
}

/// Rotates a local (x, z) offset by `rot` quarter turns (+z front maps to +x at rot 1).
pub fn rotate(v: Vec2, rot: u8) -> Vec2 {
    match rot % 4 {
        0 => v,
        1 => Vec2::new(v.y, -v.x),
        2 => Vec2::new(-v.x, -v.y),
        _ => Vec2::new(-v.y, v.x),
    }
}

pub fn rot_yaw(rot: u8) -> f32 {
    rot as f32 * std::f32::consts::FRAC_PI_2
}

impl ObjectInstance {
    pub fn new(def_id: ObjectId, def: &ObjectDef, level: i32, tile: IVec2, rot: u8) -> Self {
        ObjectInstance {
            def: def_id,
            tile,
            level,
            rot: rot % 4,
            dirt: 0.0,
            broken: false,
            slot_users: vec![None; def.slots.len()],
            uses: 0,
            price_paid: def.price,
            ground: 0.0,
        }
    }

    /// Object-local point → world position.
    pub fn local_to_world(&self, def: &ObjectDef, p: Vec3) -> Vec3 {
        let c = Vec2::new(def.footprint.0 as f32, def.footprint.1 as f32) * 0.5;
        let rs = rotated_size(def, self.rot).as_vec2() * 0.5;
        let q = rotate(Vec2::new(p.x, p.z) - c, self.rot) + rs + self.tile.as_vec2();
        Vec3::new(q.x, p.y + self.level as f32 * LEVEL_HEIGHT + self.ground, q.y)
    }

    pub fn yaw(&self) -> f32 {
        rot_yaw(self.rot)
    }

    pub fn footprint_tiles(&self, def: &ObjectDef) -> Vec<IVec2> {
        let s = rotated_size(def, self.rot);
        let mut v = Vec::new();
        for x in 0..s.x {
            for z in 0..s.y {
                v.push(self.tile + IVec2::new(x, z));
            }
        }
        v
    }

    /// The tile a peon walks to before using slot `i`.
    pub fn slot_stand_tile(&self, def: &ObjectDef, i: usize) -> IVec2 {
        let s = &def.slots[i];
        Lot::tile_of(self.local_to_world(def, Vec3::new(s.stand.0, 0.0, s.stand.1)))
    }

    pub fn slot_use_pos(&self, def: &ObjectDef, i: usize) -> Vec3 {
        self.local_to_world(def, def.slots[i].use_at)
    }

    pub fn slot_yaw(&self, def: &ObjectDef, i: usize) -> f32 {
        def.slots[i].facing.to_radians() + self.yaw()
    }

    pub fn center(&self, def: &ObjectDef) -> Vec3 {
        self.local_to_world(def, Vec3::new(def.footprint.0 as f32 * 0.5, 0.0, def.footprint.1 as f32 * 0.5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GameData;

    #[test]
    fn rotation_keeps_footprint_in_place() {
        let data = GameData::embedded();
        let id = data.object_id("sofa_basic").unwrap();
        let def = data.object(id);
        for rot in 0..4 {
            let o = ObjectInstance::new(id, def, 0, IVec2::new(5, 5), rot);
            let tiles = o.footprint_tiles(def);
            for part in &def.parts {
                let w = o.local_to_world(def, part.at);
                let t = Lot::tile_of(w);
                assert!(tiles.contains(&t), "rot {rot}: part at {w} outside footprint {tiles:?}");
            }
            // stand tile is outside the footprint
            assert!(!tiles.contains(&o.slot_stand_tile(def, 0)));
        }
        // rot 0 front faces +z
        let o = ObjectInstance::new(id, def, 0, IVec2::new(5, 5), 0);
        assert_eq!(o.slot_stand_tile(def, 0), IVec2::new(5, 6));
        let o = ObjectInstance::new(id, def, 0, IVec2::new(5, 5), 1);
        assert_eq!(o.slot_stand_tile(def, 0).x, 6, "rot 1 front faces +x");
    }
}
