//! The buildable lot: a tile grid per level with edge walls, floors, openings and stairs.
//!
//! Coordinates: tile (x, z) covers [x, x+1] × [z, z+1] meters. Level `l` floor is at y = l × 3 m.
//! An [`Edge`] with [`Axis::X`] runs along +x from (x, z) to (x+1, z) and separates tiles
//! (x, z−1) and (x, z). [`Axis::Z`] runs along +z from (x, z) to (x, z+1), separating (x−1, z) and (x, z).

use std::collections::{BTreeMap, HashMap};

use glam::{IVec2, Vec3};
use hearth_ecs::Entity;
use serde::{Deserialize, Serialize};

pub const LEVEL_HEIGHT: f32 = 3.0;
pub const MIN_LEVEL: i32 = -2;
pub const MAX_LEVEL: i32 = 4;
pub const STAIR_LEN: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Axis {
    X,
    Z,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Edge {
    pub level: i32,
    pub x: i32,
    pub z: i32,
    pub axis: Axis,
}

impl Edge {
    pub fn new(level: i32, x: i32, z: i32, axis: Axis) -> Self {
        Edge { level, x, z, axis }
    }
    /// World-space endpoints at the floor of its level.
    pub fn endpoints(&self) -> (Vec3, Vec3) {
        let y = self.level as f32 * LEVEL_HEIGHT;
        let a = Vec3::new(self.x as f32, y, self.z as f32);
        let b = match self.axis {
            Axis::X => a + Vec3::X,
            Axis::Z => a + Vec3::Z,
        };
        (a, b)
    }
    /// The two tiles this edge separates.
    pub fn tiles(&self) -> (IVec2, IVec2) {
        match self.axis {
            Axis::X => (IVec2::new(self.x, self.z - 1), IVec2::new(self.x, self.z)),
            Axis::Z => (IVec2::new(self.x - 1, self.z), IVec2::new(self.x, self.z)),
        }
    }
    /// The edge between two orthogonally adjacent tiles.
    pub fn between(level: i32, a: IVec2, b: IVec2) -> Option<Edge> {
        let d = b - a;
        match (d.x, d.y) {
            (1, 0) => Some(Edge::new(level, b.x, b.y, Axis::Z)),
            (-1, 0) => Some(Edge::new(level, a.x, a.y, Axis::Z)),
            (0, 1) => Some(Edge::new(level, b.x, b.y, Axis::X)),
            (0, -1) => Some(Edge::new(level, a.x, a.y, Axis::X)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Opening {
    Door,
    Window,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wall {
    /// Index into the wall-covering palette.
    pub style: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Floor {
    pub style: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dir4 {
    PosX,
    PosZ,
    NegX,
    NegZ,
}

impl Dir4 {
    pub fn vec(self) -> IVec2 {
        match self {
            Dir4::PosX => IVec2::new(1, 0),
            Dir4::PosZ => IVec2::new(0, 1),
            Dir4::NegX => IVec2::new(-1, 0),
            Dir4::NegZ => IVec2::new(0, -1),
        }
    }
    pub fn yaw(self) -> f32 {
        match self {
            Dir4::PosZ => 0.0,
            Dir4::PosX => std::f32::consts::FRAC_PI_2,
            Dir4::NegZ => std::f32::consts::PI,
            Dir4::NegX => -std::f32::consts::FRAC_PI_2,
        }
    }
}

/// Straight stairs rising from `level` to `level + 1`, occupying [`STAIR_LEN`] tiles starting at
/// `start` going in `dir`. Entered from the tile before `start` on the lower level and exited onto
/// the tile after the last stair tile on the upper level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stair {
    pub level: i32,
    pub start: IVec2,
    pub dir: Dir4,
}

impl Stair {
    pub fn tiles(&self) -> impl Iterator<Item = IVec2> + '_ {
        (0..STAIR_LEN).map(|i| self.start + self.dir.vec() * i)
    }
    pub fn bottom_access(&self) -> IVec2 {
        self.start - self.dir.vec()
    }
    pub fn top_access(&self) -> IVec2 {
        self.start + self.dir.vec() * STAIR_LEN
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RoomInfo {
    pub id: u32,
    pub level: i32,
    pub tiles: u32,
    pub indoor: bool,
    pub beauty: f32,
    pub dirt: f32,
    /// 0–100 environment score used by the Environment need.
    pub score: f32,
}

/// The lot resource.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lot {
    pub width: i32,
    pub depth: i32,
    pub walls: BTreeMap<Edge, Wall>,
    pub openings: BTreeMap<Edge, Opening>,
    pub floors: BTreeMap<(i32, i32, i32), Floor>,
    pub stairs: Vec<Stair>,
    /// Tiles blocked by objects.
    #[serde(skip)]
    pub occupancy: HashMap<(i32, i32, i32), Entity>,
    /// Per-tile room id, keyed by (level, x, z). Recomputed when `rooms_dirty`.
    #[serde(skip)]
    pub room_of: HashMap<(i32, i32, i32), u32>,
    #[serde(skip)]
    pub rooms: Vec<RoomInfo>,
    #[serde(skip)]
    pub rooms_dirty: bool,
    /// Bumped on every structural change so renderers can rebuild meshes.
    #[serde(skip)]
    pub version: u64,
}

impl Lot {
    pub fn new(width: i32, depth: i32) -> Self {
        Lot {
            width,
            depth,
            walls: BTreeMap::new(),
            openings: BTreeMap::new(),
            floors: BTreeMap::new(),
            stairs: Vec::new(),
            occupancy: HashMap::new(),
            room_of: HashMap::new(),
            rooms: Vec::new(),
            rooms_dirty: true,
            version: 1,
        }
    }

    fn touch(&mut self) {
        self.rooms_dirty = true;
        self.version += 1;
    }

    pub fn in_bounds(&self, t: IVec2) -> bool {
        t.x >= 0 && t.y >= 0 && t.x < self.width && t.y < self.depth
    }

    pub fn level_ok(level: i32) -> bool {
        (MIN_LEVEL..=MAX_LEVEL).contains(&level)
    }

    // ------------------------------------------------------------------ editing

    pub fn add_wall(&mut self, e: Edge, style: u16) -> bool {
        if !Self::level_ok(e.level) {
            return false;
        }
        let (a, b) = e.tiles();
        if !self.in_bounds(a) && !self.in_bounds(b) {
            return false;
        }
        let new = self.walls.insert(e, Wall { style }).is_none();
        self.touch();
        new
    }

    pub fn remove_wall(&mut self, e: Edge) -> bool {
        self.openings.remove(&e);
        let r = self.walls.remove(&e).is_some();
        self.touch();
        r
    }

    /// Adds walls along a straight line of edges from tile corner `a` to `b` (must share x or z).
    pub fn wall_line(&mut self, level: i32, a: IVec2, b: IVec2, style: u16) -> usize {
        let mut n = 0;
        if a.y == b.y {
            for x in a.x.min(b.x)..a.x.max(b.x) {
                n += self.add_wall(Edge::new(level, x, a.y, Axis::X), style) as usize;
            }
        } else if a.x == b.x {
            for z in a.y.min(b.y)..a.y.max(b.y) {
                n += self.add_wall(Edge::new(level, a.x, z, Axis::Z), style) as usize;
            }
        }
        n
    }

    /// Walls around the rectangle of tiles [min, max) on `level`.
    pub fn wall_rect(&mut self, level: i32, min: IVec2, max: IVec2, style: u16) -> usize {
        self.wall_line(level, min, IVec2::new(max.x, min.y), style)
            + self.wall_line(level, IVec2::new(min.x, max.y), max, style)
            + self.wall_line(level, min, IVec2::new(min.x, max.y), style)
            + self.wall_line(level, IVec2::new(max.x, min.y), max, style)
    }

    pub fn set_opening(&mut self, e: Edge, o: Opening) -> bool {
        if !self.walls.contains_key(&e) {
            return false;
        }
        self.openings.insert(e, o);
        self.touch();
        true
    }

    pub fn set_floor(&mut self, level: i32, t: IVec2, style: u16) -> bool {
        if !self.in_bounds(t) || !Self::level_ok(level) {
            return false;
        }
        self.floors.insert((level, t.x, t.y), Floor { style });
        self.touch();
        true
    }

    pub fn floor_rect(&mut self, level: i32, min: IVec2, max: IVec2, style: u16) {
        for x in min.x..max.x {
            for z in min.y..max.y {
                self.set_floor(level, IVec2::new(x, z), style);
            }
        }
    }

    pub fn remove_floor(&mut self, level: i32, t: IVec2) -> bool {
        let r = self.floors.remove(&(level, t.x, t.y)).is_some();
        self.touch();
        r
    }

    pub fn add_stair(&mut self, s: Stair) -> Result<(), String> {
        if !Self::level_ok(s.level) || !Self::level_ok(s.level + 1) {
            return Err("stairs out of level range".into());
        }
        for t in s.tiles().chain([s.bottom_access(), s.top_access()]) {
            if !self.in_bounds(t) {
                return Err("stairs out of bounds".into());
            }
        }
        for t in s.tiles() {
            if self.occupancy.contains_key(&(s.level, t.x, t.y)) || self.stair_at(s.level, t).is_some() {
                return Err("stairs blocked".into());
            }
        }
        // cut a hole in the floor above
        for t in s.tiles() {
            self.floors.remove(&(s.level + 1, t.x, t.y));
        }
        // make sure there's somewhere to step off
        let top = s.top_access();
        if s.level + 1 != 0 && !self.floors.contains_key(&(s.level + 1, top.x, top.y)) {
            self.floors.insert((s.level + 1, top.x, top.y), Floor { style: 0 });
        }
        self.stairs.push(s);
        self.touch();
        Ok(())
    }

    /// Digs a basement room: floors on `level` (< 0) and walls around it, plus a ground floor above.
    pub fn dig_basement(&mut self, level: i32, min: IVec2, max: IVec2, wall_style: u16, floor_style: u16) {
        assert!(level < 0);
        self.floor_rect(level, min, max, floor_style);
        self.wall_rect(level, min, max, wall_style);
        for l in (level + 1)..=0 {
            for x in min.x..max.x {
                for z in min.y..max.y {
                    self.floors.entry((l, x, z)).or_insert(Floor { style: floor_style });
                }
            }
        }
        self.touch();
    }

    // ------------------------------------------------------------------ queries

    pub fn has_floor(&self, level: i32, t: IVec2) -> bool {
        self.floors.contains_key(&(level, t.x, t.y))
    }

    pub fn stair_at(&self, level: i32, t: IVec2) -> Option<&Stair> {
        self.stairs.iter().find(|s| s.level == level && s.tiles().any(|st| st == t))
    }

    /// Whether a tile has something to stand on (ignores objects).
    pub fn has_ground(&self, level: i32, t: IVec2) -> bool {
        if !self.in_bounds(t) || !Self::level_ok(level) {
            return false;
        }
        if level == 0 {
            // ground level is terrain unless a basement has been dug beneath with no floor left
            !self.floors.contains_key(&(-1, t.x, t.y)) || self.has_floor(0, t)
        } else {
            self.has_floor(level, t)
        }
    }

    /// Whether a peon may stand on this tile.
    pub fn walkable(&self, level: i32, t: IVec2) -> bool {
        self.has_ground(level, t)
            && !self.occupancy.contains_key(&(level, t.x, t.y))
            && self.stair_at(level, t).is_none()
    }

    pub fn edge_blocked(&self, e: &Edge) -> bool {
        self.walls.contains_key(e) && self.openings.get(e) != Some(&Opening::Door)
    }

    /// Can a peon step directly from `a` to neighbouring tile `b` (8-connected) on `level`?
    pub fn can_step(&self, level: i32, a: IVec2, b: IVec2) -> bool {
        if !self.walkable(level, b) {
            return false;
        }
        let d = b - a;
        if d.x.abs() + d.y.abs() == 1 {
            return !self.edge_blocked(&Edge::between(level, a, b).unwrap());
        }
        if d.x.abs() == 1 && d.y.abs() == 1 {
            // diagonal: both orthogonal routes must be open, and no wall may touch the shared corner
            let c1 = IVec2::new(b.x, a.y);
            let c2 = IVec2::new(a.x, b.y);
            let ok1 = self.walkable(level, c1)
                && !self.edge_blocked(&Edge::between(level, a, c1).unwrap())
                && !self.edge_blocked(&Edge::between(level, c1, b).unwrap());
            let ok2 = self.walkable(level, c2)
                && !self.edge_blocked(&Edge::between(level, a, c2).unwrap())
                && !self.edge_blocked(&Edge::between(level, c2, b).unwrap());
            // any wall touching the corner point blocks diagonal cutting
            let corner = IVec2::new(a.x.max(b.x), a.y.max(b.y));
            let touching = [
                Edge::new(level, corner.x - 1, corner.y, Axis::X),
                Edge::new(level, corner.x, corner.y, Axis::X),
                Edge::new(level, corner.x, corner.y - 1, Axis::Z),
                Edge::new(level, corner.x, corner.y, Axis::Z),
            ]
            .iter()
            .any(|e| self.walls.contains_key(e));
            return ok1 && ok2 && !touching;
        }
        false
    }

    pub fn tile_center(level: i32, t: IVec2) -> Vec3 {
        Vec3::new(t.x as f32 + 0.5, level as f32 * LEVEL_HEIGHT, t.y as f32 + 0.5)
    }

    pub fn tile_of(pos: Vec3) -> IVec2 {
        IVec2::new(pos.x.floor() as i32, pos.z.floor() as i32)
    }

    pub fn level_of(pos: Vec3) -> i32 {
        (pos.y / LEVEL_HEIGHT + 0.25).floor() as i32
    }

    // ------------------------------------------------------------------ rooms

    pub fn room_at(&self, level: i32, t: IVec2) -> Option<&RoomInfo> {
        self.room_of.get(&(level, t.x, t.y)).and_then(|&id| self.rooms.get(id as usize))
    }

    /// Flood-fills rooms on every level. Room 0 is "outdoors". Walls (including doors) bound rooms.
    pub fn recompute_rooms(&mut self) {
        self.room_of.clear();
        self.rooms.clear();
        self.rooms.push(RoomInfo { id: 0, level: 0, tiles: 0, indoor: false, beauty: 0.0, dirt: 0.0, score: 60.0 });
        for level in MIN_LEVEL..=MAX_LEVEL {
            for x in 0..self.width {
                for z in 0..self.depth {
                    let t = IVec2::new(x, z);
                    if self.room_of.contains_key(&(level, x, z)) || !self.has_ground(level, t) {
                        continue;
                    }
                    // flood fill
                    let id = self.rooms.len() as u32;
                    let mut stack = vec![t];
                    let mut tiles = Vec::new();
                    let mut touches_outside = false;
                    self.room_of.insert((level, x, z), id);
                    while let Some(c) = stack.pop() {
                        tiles.push(c);
                        for d in [IVec2::X, IVec2::Y, -IVec2::X, -IVec2::Y] {
                            let n = c + d;
                            let e = Edge::between(level, c, n).unwrap();
                            if self.walls.contains_key(&e) {
                                continue;
                            }
                            // a stairwell hole from the level below is still part of the room
                            let stairwell = self.in_bounds(n) && self.stair_at(level - 1, n).is_some();
                            if !self.in_bounds(n) || (!self.has_ground(level, n) && !stairwell) {
                                touches_outside = true;
                                continue;
                            }
                            if self.room_of.contains_key(&(level, n.x, n.y)) {
                                continue;
                            }
                            self.room_of.insert((level, n.x, n.y), id);
                            stack.push(n);
                        }
                    }
                    let indoor = !touches_outside
                        && (level != 0 || tiles.iter().all(|t| self.has_floor(0, *t) || self.stair_at(-1, *t).is_some()));
                    if !indoor && level == 0 {
                        for t in &tiles {
                            self.room_of.insert((level, t.x, t.y), 0);
                        }
                        self.rooms[0].tiles += tiles.len() as u32;
                        continue;
                    }
                    self.rooms.push(RoomInfo {
                        id,
                        level,
                        tiles: tiles.len() as u32,
                        indoor,
                        beauty: 0.0,
                        dirt: 0.0,
                        score: 50.0,
                    });
                }
            }
        }
        self.rooms_dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed_lot() -> Lot {
        let mut lot = Lot::new(10, 10);
        lot.wall_rect(0, IVec2::new(2, 2), IVec2::new(6, 6), 0);
        lot.floor_rect(0, IVec2::new(2, 2), IVec2::new(6, 6), 0);
        lot
    }

    #[test]
    fn walls_block_steps_doors_dont() {
        let mut lot = boxed_lot();
        let inside = IVec2::new(2, 3);
        let outside = IVec2::new(1, 3);
        assert!(!lot.can_step(0, inside, outside));
        assert!(lot.set_opening(Edge::new(0, 2, 3, Axis::Z), Opening::Door));
        assert!(lot.can_step(0, inside, outside));
        assert!(lot.can_step(0, outside, inside));
    }

    #[test]
    fn diagonal_blocked_by_corner() {
        let lot = boxed_lot();
        // diagonal out of the box corner
        assert!(!lot.can_step(0, IVec2::new(2, 2), IVec2::new(1, 1)));
        // open field diagonal is fine
        assert!(lot.can_step(0, IVec2::new(8, 8), IVec2::new(9, 9)));
    }

    #[test]
    fn rooms_detected() {
        let mut lot = boxed_lot();
        lot.recompute_rooms();
        let r = lot.room_at(0, IVec2::new(3, 3)).unwrap();
        assert!(r.indoor);
        assert_eq!(r.tiles, 16);
        assert_eq!(lot.room_at(0, IVec2::new(0, 0)).unwrap().id, 0);
        // a door does not merge rooms
        lot.set_opening(Edge::new(0, 2, 3, Axis::Z), Opening::Door);
        lot.recompute_rooms();
        assert!(lot.room_at(0, IVec2::new(3, 3)).unwrap().indoor);
    }

    #[test]
    fn basement_ground_rules() {
        let mut lot = Lot::new(10, 10);
        lot.dig_basement(-1, IVec2::new(2, 2), IVec2::new(5, 5), 0, 0);
        assert!(lot.has_ground(-1, IVec2::new(3, 3)));
        assert!(lot.has_ground(0, IVec2::new(3, 3)), "ground floor above basement gets a floor");
        lot.remove_floor(0, IVec2::new(3, 3));
        assert!(!lot.has_ground(0, IVec2::new(3, 3)), "hole above basement");
        assert!(!lot.has_ground(-1, IVec2::new(8, 8)), "undug earth");
    }
}
