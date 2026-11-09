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

/// Lowest/highest terrain height (meters). Kept within level 0 so `level_of` stays correct.
pub const TERRAIN_MIN: f32 = -0.7;
pub const TERRAIN_MAX: f32 = 2.0;
/// Peons won't walk up a step steeper than this between tile centers.
pub const MAX_SLOPE: f32 = 0.6;

/// Ground heightmap: one height per tile corner, (width+1) × (depth+1). Empty = flat at 0.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Terrain {
    pub width: i32,
    pub depth: i32,
    pub heights: Vec<f32>,
}

impl Terrain {
    pub fn new(width: i32, depth: i32) -> Self {
        Terrain { width, depth, heights: vec![0.0; ((width + 1) * (depth + 1)) as usize] }
    }
    fn idx(&self, c: IVec2) -> Option<usize> {
        (c.x >= 0 && c.y >= 0 && c.x <= self.width && c.y <= self.depth && !self.heights.is_empty())
            .then(|| (c.y * (self.width + 1) + c.x) as usize)
    }
    /// Height at a tile corner (0 outside the map).
    pub fn corner(&self, c: IVec2) -> f32 {
        self.idx(c).map(|i| self.heights[i]).unwrap_or(0.0)
    }
    pub fn set_corner(&mut self, c: IVec2, h: f32) {
        if let Some(i) = self.idx(c) {
            self.heights[i] = h.clamp(TERRAIN_MIN, TERRAIN_MAX);
        }
    }
    pub fn tile_corners(t: IVec2) -> [IVec2; 4] {
        [t, t + IVec2::X, t + IVec2::Y, t + IVec2::ONE]
    }
    /// Bilinear height at a world (x, z) point.
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        if self.heights.is_empty() {
            return 0.0;
        }
        let (fx, fz) = (x.floor(), z.floor());
        let (tx, tz) = (x - fx, z - fz);
        let c = IVec2::new(fx as i32, fz as i32);
        let h00 = self.corner(c);
        let h10 = self.corner(c + IVec2::X);
        let h01 = self.corner(c + IVec2::Y);
        let h11 = self.corner(c + IVec2::ONE);
        let a = h00 + (h10 - h00) * tx;
        let b = h01 + (h11 - h01) * tx;
        a + (b - a) * tz
    }
    /// Height at a tile's center.
    pub fn tile_height(&self, t: IVec2) -> f32 {
        self.height_at(t.x as f32 + 0.5, t.y as f32 + 0.5)
    }
    /// All four corners at the same height (objects need flat ground).
    pub fn tile_flat(&self, t: IVec2) -> bool {
        let c = Self::tile_corners(t).map(|c| self.corner(c));
        c.iter().all(|h| (h - c[0]).abs() < 0.01)
    }
    pub fn is_flat_everywhere(&self) -> bool {
        self.heights.iter().all(|h| h.abs() < 1e-4)
    }
}

/// How a terrain brush changes corner heights.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TerrainBrush {
    Raise,
    Lower,
    /// Pull toward the height at the brush center.
    Flatten,
    /// Average with neighbours.
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoofKind {
    /// Ridge runs along the rectangle's longer side.
    Gable,
    Hip,
    Flat,
}

/// A rectangular roof sitting on top of `level`'s walls, covering tiles [min, max).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Roof {
    pub level: i32,
    pub min: IVec2,
    pub max: IVec2,
    pub kind: RoofKind,
    /// Rise per meter of run (0.5 ≈ 27°).
    pub pitch: f32,
    pub style: u16,
}

impl Roof {
    pub fn size(&self) -> IVec2 {
        self.max - self.min
    }
    /// Eave height (top of the walls below).
    pub fn base_y(&self) -> f32 {
        (self.level + 1) as f32 * LEVEL_HEIGHT
    }
    /// Ridge height above the eaves.
    pub fn rise(&self) -> f32 {
        let s = self.size();
        match self.kind {
            RoofKind::Flat => 0.15,
            _ => s.x.min(s.y) as f32 * 0.5 * self.pitch,
        }
    }
    pub fn covers(&self, t: IVec2) -> bool {
        t.x >= self.min.x && t.y >= self.min.y && t.x < self.max.x && t.y < self.max.y
    }
    /// Roof surface height at world (x, z), or None outside its footprint.
    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        let (x0, z0, x1, z1) = (self.min.x as f32, self.min.y as f32, self.max.x as f32, self.max.y as f32);
        if x < x0 || z < z0 || x > x1 || z > z1 {
            return None;
        }
        let dx = (x - x0).min(x1 - x);
        let dz = (z - z0).min(z1 - z);
        let run = match self.kind {
            RoofKind::Flat => return Some(self.base_y() + 0.15),
            RoofKind::Gable => {
                if x1 - x0 >= z1 - z0 { dz } else { dx }
            }
            RoofKind::Hip => dx.min(dz),
        };
        Some(self.base_y() + (run * self.pitch).min(self.rise()))
    }
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
    #[serde(default)]
    pub terrain: Terrain,
    #[serde(default)]
    pub roofs: Vec<Roof>,
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
            terrain: Terrain::new(width, depth),
            roofs: Vec::new(),
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
        if e.level == 0 {
            let (a, b) = e.endpoints();
            for p in [a, b] {
                self.terrain.set_corner(IVec2::new(p.x as i32, p.z as i32), 0.0);
            }
        }
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
        if level == 0 {
            for c in Terrain::tile_corners(t) {
                self.terrain.set_corner(c, 0.0);
            }
        }
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

    pub fn add_roof(&mut self, r: Roof) -> Result<(), String> {
        if !Self::level_ok(r.level) || r.level < 0 {
            return Err("roofs go on top of above-ground levels".into());
        }
        let s = r.size();
        if s.x < 1 || s.y < 1 || !self.in_bounds(r.min) || !self.in_bounds(r.max - IVec2::ONE) {
            return Err("roof out of bounds".into());
        }
        if self.roofs.iter().any(|o| o.level == r.level && o.min.x < r.max.x && r.min.x < o.max.x && o.min.y < r.max.y && r.min.y < o.max.y) {
            return Err("overlaps another roof".into());
        }
        self.roofs.push(r);
        self.touch();
        Ok(())
    }

    /// Removes roofs covering tile `t` on `level` (any level if None). Returns how many.
    pub fn remove_roof_at(&mut self, level: Option<i32>, t: IVec2) -> usize {
        let before = self.roofs.len();
        self.roofs.retain(|r| !(r.covers(t) && level.is_none_or(|l| l == r.level)));
        let n = before - self.roofs.len();
        if n > 0 {
            self.touch();
        }
        n
    }

    /// Replaces all roofs with automatic ones: the indoor tiles with nothing built above them are
    /// split greedily into rectangles, each getting its own roof. Returns the number of roofs.
    pub fn auto_roof(&mut self, kind: RoofKind, pitch: f32) -> usize {
        if self.rooms_dirty {
            self.recompute_rooms();
        }
        self.roofs.clear();
        for level in 0..=MAX_LEVEL {
            let mut covered = std::collections::HashSet::new();
            for z in 0..self.depth {
                for x in 0..self.width {
                    let t = IVec2::new(x, z);
                    if covered.contains(&t) || !self.roofable(level, t) {
                        continue;
                    }
                    let free = |c: IVec2, covered: &std::collections::HashSet<IVec2>| self.in_bounds(c) && !covered.contains(&c) && self.roofable(level, c);
                    let mut x1 = x + 1;
                    while free(IVec2::new(x1, z), &covered) {
                        x1 += 1;
                    }
                    let mut z1 = z + 1;
                    while (x..x1).all(|xx| free(IVec2::new(xx, z1), &covered)) {
                        z1 += 1;
                    }
                    for xx in x..x1 {
                        for zz in z..z1 {
                            covered.insert(IVec2::new(xx, zz));
                        }
                    }
                    self.roofs.push(Roof { level, min: t, max: IVec2::new(x1, z1), kind, pitch, style: 0 });
                }
            }
        }
        self.touch();
        self.roofs.len()
    }

    /// An indoor tile with no floor directly above it.
    fn roofable(&self, level: i32, t: IVec2) -> bool {
        let indoor = self.room_at(level, t).is_some_and(|r| r.indoor);
        indoor && !self.has_floor(level + 1, t) && self.stair_at(level, t).is_none()
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
        if level == 0 && (self.ground_y(0, a) - self.ground_y(0, b)).abs() > MAX_SLOPE {
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

    /// Height of the walking surface at a tile's center (floors are flat; bare ground follows terrain).
    pub fn ground_y(&self, level: i32, t: IVec2) -> f32 {
        if level == 0 && !self.has_floor(0, t) { self.terrain.tile_height(t) } else { level as f32 * LEVEL_HEIGHT }
    }

    /// Like [`Lot::tile_center`] but standing on the actual ground.
    pub fn ground_center(&self, level: i32, t: IVec2) -> Vec3 {
        let mut c = Self::tile_center(level, t);
        c.y = self.ground_y(level, t);
        c
    }

    /// A corner may be sculpted only if no ground-floor tile or wall touches it.
    pub fn corner_locked(&self, c: IVec2) -> bool {
        let tiles = [c, c - IVec2::X, c - IVec2::Y, c - IVec2::ONE];
        tiles.iter().any(|t| self.has_floor(0, *t) || self.floors.contains_key(&(-1, t.x, t.y)))
            || [
                Edge::new(0, c.x, c.y, Axis::X),
                Edge::new(0, c.x - 1, c.y, Axis::X),
                Edge::new(0, c.x, c.y, Axis::Z),
                Edge::new(0, c.x, c.y - 1, Axis::Z),
            ]
            .iter()
            .any(|e| self.walls.contains_key(e))
    }

    /// Applies a circular brush around corner `center`. Returns how many corners changed.
    /// `occupied` tiles (objects) are protected so furniture never ends up floating.
    pub fn sculpt(&mut self, center: IVec2, radius: f32, strength: f32, brush: TerrainBrush) -> usize {
        if self.terrain.heights.is_empty() {
            self.terrain = Terrain::new(self.width, self.depth);
        }
        let target = self.terrain.corner(center);
        let r = radius.ceil() as i32;
        let mut changed = 0;
        let before = self.terrain.clone();
        for dz in -r..=r {
            for dx in -r..=r {
                let c = center + IVec2::new(dx, dz);
                let d = ((dx * dx + dz * dz) as f32).sqrt();
                if d > radius || self.corner_locked(c) || self.corner_has_object(c) {
                    continue;
                }
                let falloff = 1.0 - d / (radius + 1.0);
                let h = before.corner(c);
                let nh = match brush {
                    TerrainBrush::Raise => h + strength * falloff,
                    TerrainBrush::Lower => h - strength * falloff,
                    TerrainBrush::Flatten => h + (target - h) * falloff.min(1.0),
                    TerrainBrush::Smooth => {
                        let n = [IVec2::X, -IVec2::X, IVec2::Y, -IVec2::Y].map(|o| before.corner(c + o));
                        h + ((n.iter().sum::<f32>() / 4.0) - h) * 0.5 * falloff
                    }
                };
                if (nh - h).abs() > 1e-4 && self.terrain.idx(c).is_some() {
                    self.terrain.set_corner(c, nh);
                    changed += 1;
                }
            }
        }
        if changed > 0 {
            self.touch();
        }
        changed
    }

    fn corner_has_object(&self, c: IVec2) -> bool {
        [c, c - IVec2::X, c - IVec2::Y, c - IVec2::ONE].iter().any(|t| self.occupancy.contains_key(&(0, t.x, t.y)))
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
    fn auto_roof_covers_top_floor() {
        let mut lot = boxed_lot();
        assert_eq!(lot.auto_roof(RoofKind::Gable, 0.5), 1);
        let r = lot.roofs[0];
        assert_eq!((r.level, r.min, r.max), (0, IVec2::new(2, 2), IVec2::new(6, 6)));
        // add a second storey: the roof moves up
        lot.floor_rect(1, IVec2::new(2, 2), IVec2::new(6, 6), 0);
        lot.wall_rect(1, IVec2::new(2, 2), IVec2::new(6, 6), 0);
        lot.auto_roof(RoofKind::Hip, 0.5);
        assert_eq!(lot.roofs.len(), 1);
        assert_eq!(lot.roofs[0].level, 1);
        let peak = lot.roofs[0].height_at(4.0, 4.0).unwrap();
        assert!((peak - (6.0 + 1.0)).abs() < 1e-4, "{peak}");
        assert!((lot.roofs[0].height_at(2.0, 4.0).unwrap() - 6.0).abs() < 1e-4);
    }

    #[test]
    fn terrain_sculpting_respects_buildings() {
        let mut lot = boxed_lot();
        let open = IVec2::new(8, 8);
        assert!(lot.sculpt(open, 1.5, 0.5, TerrainBrush::Raise) > 0);
        assert!(lot.terrain.corner(open) > 0.4);
        assert!(!lot.terrain.tile_flat(IVec2::new(8, 8)));
        // corners touching the house stay put
        let n = lot.sculpt(IVec2::new(4, 4), 3.0, 1.0, TerrainBrush::Raise);
        assert!(n > 0);
        assert_eq!(lot.terrain.corner(IVec2::new(4, 4)), 0.0);
        assert_eq!(lot.terrain.corner(IVec2::new(2, 2)), 0.0);
        // a cliff blocks walking
        for c in Terrain::tile_corners(IVec2::new(9, 1)) {
            lot.terrain.set_corner(c, 2.0);
        }
        assert!(!lot.can_step(0, IVec2::new(8, 1), IVec2::new(9, 1)), "too steep");
        assert!(lot.can_step(0, IVec2::new(9, 0), IVec2::new(9, 1)) == ((lot.ground_y(0, IVec2::new(9, 0)) - 2.0).abs() <= MAX_SLOPE));
        assert!(lot.ground_y(0, IVec2::new(8, 8)) > 0.0);
        assert_eq!(lot.ground_y(0, IVec2::new(3, 3)), 0.0);
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
