//! Search-local tile facts and wall flags. Rebuilt for each flood so direct edits to
//! public lot collections, object occupancy and terrain cannot leave stale routes.
use crate::lot::{Axis, Edge, Lot, MAX_LEVEL, MAX_SLOPE, MIN_LEVEL, Opening};
use glam::IVec2;

pub(super) struct Traversal<'a> {
    lot: &'a Lot,
    walkable: Vec<u8>,
    heights: Vec<f32>,
    // At each grid corner: blocked X edge, blocked Z edge, any touching wall.
    edges: Vec<u8>,
}

impl<'a> Traversal<'a> {
    pub(super) fn new(lot: &'a Lot) -> Self {
        let levels = (MAX_LEVEL - MIN_LEVEL + 1) as usize;
        let mut this = Self {
            lot,
            walkable: vec![0; (lot.width * lot.depth) as usize * levels],
            heights: vec![f32::NAN; (lot.width * lot.depth) as usize],
            edges: vec![0; ((lot.width + 1) * (lot.depth + 1)) as usize * levels],
        };
        for e in lot.walls.keys() {
            if let Some(i) = this.edge_idx(e.level, IVec2::new(e.x, e.z)) {
                this.edges[i] |= 4;
                if lot.openings.get(e) != Some(&Opening::Door) {
                    this.edges[i] |= if e.axis == Axis::X { 1 } else { 2 };
                }
            }
            let end = IVec2::new(e.x, e.z)
                + if e.axis == Axis::X {
                    IVec2::X
                } else {
                    IVec2::Y
                };
            if let Some(i) = this.edge_idx(e.level, end) {
                this.edges[i] |= 4;
            }
        }
        this
    }

    fn edge_idx(&self, level: i32, p: IVec2) -> Option<usize> {
        if !(MIN_LEVEL..=MAX_LEVEL).contains(&level)
            || p.x < 0
            || p.y < 0
            || p.x > self.lot.width
            || p.y > self.lot.depth
        {
            return None;
        }
        Some(
            (((level - MIN_LEVEL) * (self.lot.depth + 1) + p.y) * (self.lot.width + 1) + p.x)
                as usize,
        )
    }

    fn walkable(&mut self, level: i32, p: IVec2) -> bool {
        if !self.lot.in_bounds(p) || !Lot::level_ok(level) {
            return false;
        }
        let i = (((level - MIN_LEVEL) * self.lot.depth + p.y) * self.lot.width + p.x) as usize;
        if self.walkable[i] == 0 {
            self.walkable[i] = if self.lot.walkable(level, p) { 2 } else { 1 };
        }
        self.walkable[i] == 2
    }

    fn height(&mut self, p: IVec2) -> f32 {
        let i = (p.y * self.lot.width + p.x) as usize;
        if self.heights[i].is_nan() {
            self.heights[i] = self.lot.ground_y(0, p);
        }
        self.heights[i]
    }

    fn blocked(&self, level: i32, a: IVec2, b: IVec2) -> bool {
        let e = Edge::between(level, a, b).unwrap();
        self.edge_idx(level, IVec2::new(e.x, e.z))
            .is_some_and(|i| self.edges[i] & if e.axis == Axis::X { 1 } else { 2 } != 0)
    }

    /// Same directed step rules as Lot::can_step, including a blocked start tile.
    pub(super) fn can_step(&mut self, level: i32, a: IVec2, b: IVec2) -> bool {
        if !self.walkable(level, b) {
            return false;
        }
        if level == 0 && (self.height(a) - self.height(b)).abs() > MAX_SLOPE {
            return false;
        }
        let d = b - a;
        if d.x.abs() + d.y.abs() == 1 {
            return !self.blocked(level, a, b);
        }
        if d.x.abs() == 1 && d.y.abs() == 1 {
            let c1 = IVec2::new(b.x, a.y);
            let c2 = IVec2::new(a.x, b.y);
            let corner = IVec2::new(a.x.max(b.x), a.y.max(b.y));
            let touching = self
                .edge_idx(level, corner)
                .is_some_and(|i| self.edges[i] & 4 != 0);
            return !touching
                && self.walkable(level, c1)
                && !self.blocked(level, a, c1)
                && !self.blocked(level, c1, b)
                && self.walkable(level, c2)
                && !self.blocked(level, a, c2)
                && !self.blocked(level, c2, b);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(lot: &Lot) {
        let mut traversal = Traversal::new(lot);
        for level in MIN_LEVEL..=MAX_LEVEL {
            for x in 0..lot.width {
                for z in 0..lot.depth {
                    let a = IVec2::new(x, z);
                    for dx in -1..=1 {
                        for dz in -1..=1 {
                            let b = a + IVec2::new(dx, dz);
                            assert_eq!(
                                traversal.can_step(level, a, b),
                                lot.can_step(level, a, b),
                                "{level}: {a:?} -> {b:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn steps_match_live_lot_after_edits() {
        let sim = crate::starter::starter_sim(1);
        let mut lot = sim.world.res::<Lot>().clone();
        compare(&lot);
        // Direct edits need no revision bump or cache invalidation.
        lot.openings.clear();
        lot.occupancy.clear();
        lot.stairs.clear();
        lot.terrain.set_corner(IVec2::new(1, 1), 2.0);
        compare(&lot);
        lot.walls.clear();
        lot.floors.clear();
        compare(&lot);
    }
    #[test]
    fn steps_match_randomized_geometry() {
        use crate::{
            Entity,
            lot::{Floor, Wall},
            rng::SimRng,
        };
        for seed in 0..16 {
            let mut rng = SimRng::new(seed);
            let mut lot = Lot::new(8, 7);
            for level in MIN_LEVEL..=MAX_LEVEL {
                for x in 0..=lot.width {
                    for z in 0..=lot.depth {
                        for axis in [Axis::X, Axis::Z] {
                            if rng.chance(0.15) {
                                let edge = Edge::new(level, x, z, axis);
                                lot.walls.insert(edge, Wall { style: 0 });
                                if rng.chance(0.5) {
                                    lot.openings.insert(
                                        edge,
                                        if rng.chance(0.5) {
                                            Opening::Door
                                        } else {
                                            Opening::Window
                                        },
                                    );
                                }
                            }
                        }
                        if rng.chance(0.6) {
                            lot.floors.insert((level, x, z), Floor { style: 0 });
                        }
                        if rng.chance(0.1) {
                            lot.occupancy.insert((level, x, z), Entity::PLACEHOLDER);
                        }
                        if level == 0 && rng.chance(0.3) {
                            lot.terrain
                                .set_corner(IVec2::new(x, z), rng.range_f32(-1.0, 1.0));
                        }
                    }
                }
            }
            compare(&lot);
        }
    }
}
