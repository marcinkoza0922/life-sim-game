//! Tile-grid navigation across levels. A single Dijkstra flood from the peon gives distances to
//! every reachable tile (used to score all candidate objects at once) and paths via predecessors.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use glam::{IVec2, Vec3};

use crate::lot::{LEVEL_HEIGHT, Lot, MAX_LEVEL, MIN_LEVEL, STAIR_LEN};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NavNode {
    pub level: i32,
    pub tile: IVec2,
}

impl NavNode {
    pub fn new(level: i32, tile: IVec2) -> Self {
        NavNode { level, tile }
    }
    pub fn center(self) -> Vec3 {
        Lot::tile_center(self.level, self.tile)
    }
}

pub struct NavField {
    width: i32,
    depth: i32,
    dist: Vec<f32>,
    prev: Vec<u32>,
    pub start: NavNode,
}

const NONE: u32 = u32::MAX;

#[derive(PartialEq)]
struct Item(f32, u32);
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal).then(o.1.cmp(&self.1))
    }
}

impl NavField {
    fn idx(&self, n: NavNode) -> Option<usize> {
        if n.tile.x < 0 || n.tile.y < 0 || n.tile.x >= self.width || n.tile.y >= self.depth {
            return None;
        }
        if !(MIN_LEVEL..=MAX_LEVEL).contains(&n.level) {
            return None;
        }
        Some((((n.level - MIN_LEVEL) * self.depth + n.tile.y) * self.width + n.tile.x) as usize)
    }

    fn node(&self, i: usize) -> NavNode {
        let i = i as i32;
        let x = i % self.width;
        let z = (i / self.width) % self.depth;
        let l = i / (self.width * self.depth) + MIN_LEVEL;
        NavNode::new(l, IVec2::new(x, z))
    }

    /// Flood from `start`. The start tile itself may be blocked (e.g. peon standing at an object).
    pub fn flood(lot: &Lot, start: NavNode, max_dist: f32) -> NavField {
        Self::flood_avoiding(lot, start, max_dist, &[])
    }

    /// Like [`NavField::flood`], treating `avoid` tiles as blocked (e.g. someone standing there).
    pub fn flood_avoiding(lot: &Lot, start: NavNode, max_dist: f32, avoid: &[NavNode]) -> NavField {
        #[cfg(feature = "profiling")]
        let _span = crate::profiling::Span::new(0);
        let levels = (MAX_LEVEL - MIN_LEVEL + 1) as usize;
        let n = (lot.width * lot.depth) as usize * levels;
        let mut f = NavField {
            width: lot.width,
            depth: lot.depth,
            dist: vec![f32::INFINITY; n],
            prev: vec![NONE; n],
            start,
        };
        let Some(si) = f.idx(start) else { return f };
        f.dist[si] = 0.0;
        let mut heap = BinaryHeap::new();
        heap.push(Item(0.0, si as u32));
        while let Some(Item(d, i)) = heap.pop() {
            if d > f.dist[i as usize] || d > max_dist {
                continue;
            }
            let cur = f.node(i as usize);
            let mut relax = |f: &mut NavField, next: NavNode, cost: f32| {
                if let Some(j) = f.idx(next) {
                    let nd = d + cost;
                    if nd < f.dist[j] {
                        f.dist[j] = nd;
                        f.prev[j] = i;
                        heap.push(Item(nd, j as u32));
                    }
                }
            };
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    let nt = cur.tile + IVec2::new(dx, dz);
                    if lot.can_step(cur.level, cur.tile, nt) && !avoid.contains(&NavNode::new(cur.level, nt)) {
                        let cost = if dx != 0 && dz != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
                        relax(&mut f, NavNode::new(cur.level, nt), cost);
                    }
                }
            }
            for s in &lot.stairs {
                let cost = STAIR_LEN as f32 * 1.3 + 1.0;
                if cur.level == s.level && cur.tile == s.bottom_access() && lot.walkable(s.level + 1, s.top_access()) {
                    relax(&mut f, NavNode::new(s.level + 1, s.top_access()), cost);
                }
                if cur.level == s.level + 1 && cur.tile == s.top_access() && lot.walkable(s.level, s.bottom_access()) {
                    relax(&mut f, NavNode::new(s.level, s.bottom_access()), cost);
                }
            }
        }
        f
    }

    pub fn distance(&self, n: NavNode) -> Option<f32> {
        self.idx(n).map(|i| self.dist[i]).filter(|d| d.is_finite())
    }

    pub fn path_to(&self, goal: NavNode) -> Option<Vec<NavNode>> {
        let mut i = self.idx(goal)?;
        if !self.dist[i].is_finite() {
            return None;
        }
        let mut out = vec![goal];
        while self.prev[i] != NONE {
            i = self.prev[i] as usize;
            out.push(self.node(i));
        }
        out.reverse();
        Some(out)
    }

    /// Reachable tiles within `radius` path distance (for idle wandering).
    pub fn tiles_within(&self, radius: f32) -> Vec<NavNode> {
        self.dist
            .iter()
            .enumerate()
            .filter(|(_, d)| d.is_finite() && **d <= radius && **d > 0.0)
            .map(|(i, _)| self.node(i))
            .collect()
    }
}

/// Converts a node path into world waypoints, adding intermediate points along stairs.
pub fn waypoints(lot: &Lot, path: &[NavNode]) -> Vec<Vec3> {
    let mut out = Vec::with_capacity(path.len() + 4);
    for (k, n) in path.iter().enumerate() {
        if k > 0 {
            let p = path[k - 1];
            if p.level != n.level {
                let up = n.level > p.level;
                let lower = p.level.min(n.level);
                if let Some(s) = lot.stairs.iter().find(|s| {
                    s.level == lower
                        && ((up && s.bottom_access() == p.tile && s.top_access() == n.tile)
                            || (!up && s.top_access() == p.tile && s.bottom_access() == n.tile))
                }) {
                    let base = lower as f32 * LEVEL_HEIGHT;
                    let tiles: Vec<IVec2> = s.tiles().collect();
                    let steps = tiles.len();
                    let mut pts: Vec<Vec3> = tiles
                        .iter()
                        .enumerate()
                        .map(|(i, t)| {
                            let frac = (i as f32 + 0.5) / steps as f32;
                            Vec3::new(t.x as f32 + 0.5, base + frac * LEVEL_HEIGHT, t.y as f32 + 0.5)
                        })
                        .collect();
                    if !up {
                        pts.reverse();
                    }
                    out.extend(pts);
                }
            }
        }
        out.push(lot.ground_center(n.level, n.tile));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lot::{Axis, Dir4, Edge, Opening, Stair};

    #[test]
    fn path_goes_through_door() {
        let mut lot = Lot::new(12, 12);
        lot.wall_rect(0, IVec2::new(2, 2), IVec2::new(8, 8), 0);
        lot.floor_rect(0, IVec2::new(2, 2), IVec2::new(8, 8), 0);
        let inside = NavNode::new(0, IVec2::new(4, 4));
        let outside = NavNode::new(0, IVec2::new(0, 0));
        let f = NavField::flood(&lot, outside, 1e9);
        assert!(f.path_to(inside).is_none(), "sealed room unreachable");
        lot.set_opening(Edge::new(0, 5, 8, Axis::X), Opening::Door);
        let f = NavField::flood(&lot, outside, 1e9);
        let p = f.path_to(inside).expect("reachable via door");
        assert!(p.contains(&NavNode::new(0, IVec2::new(5, 8))) || p.contains(&NavNode::new(0, IVec2::new(5, 7))));
        // each step is a legal move
        for w in p.windows(2) {
            assert!(lot.can_step(0, w[0].tile, w[1].tile), "{:?} -> {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn stairs_connect_levels() {
        let mut lot = Lot::new(12, 12);
        lot.floor_rect(1, IVec2::new(0, 0), IVec2::new(12, 12), 0);
        lot.add_stair(Stair { level: 0, start: IVec2::new(5, 3), dir: Dir4::PosZ }).unwrap();
        let f = NavField::flood(&lot, NavNode::new(0, IVec2::new(0, 0)), 1e9);
        let goal = NavNode::new(1, IVec2::new(10, 10));
        let p = f.path_to(goal).expect("upstairs reachable");
        assert!(p.iter().any(|n| n.level == 1));
        let w = waypoints(&lot, &p);
        assert!(w.iter().any(|v| v.y > 0.5 && v.y < 2.5), "stair waypoints climb gradually");
        assert_eq!(w.last().unwrap().y, LEVEL_HEIGHT);
    }
}
