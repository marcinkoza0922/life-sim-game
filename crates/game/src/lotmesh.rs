//! Procedural meshes for lot geometry that isn't a stack of boxes: the terrain heightmap and
//! roofs. Rebuilt only when the lot's version changes.

use glam::{IVec2, Vec3};
use hearth_render::{MeshData, MeshId, Renderer, Vertex};
use sim::lot::{LEVEL_HEIGHT, Lot, Roof, RoofKind};

/// GPU meshes derived from the lot, kept in sync by version.
#[derive(Default)]
pub struct LotMeshes {
    version: u64,
    pub terrain: Option<MeshId>,
    /// One combined mesh per roof level.
    pub roofs: Vec<(i32, MeshId)>,
    spare: Vec<MeshId>,
}

impl LotMeshes {
    pub fn sync(&mut self, r: &mut Renderer, lot: &Lot) {
        if self.version == lot.version && self.terrain.is_some() {
            return;
        }
        self.version = lot.version;
        let t = terrain_mesh(lot);
        match self.terrain {
            Some(id) => r.update_mesh(id, &t),
            None => self.terrain = Some(r.upload_mesh(&t)),
        }
        // reuse mesh slots for roofs
        self.spare.extend(self.roofs.drain(..).map(|(_, id)| id));
        let mut levels: Vec<i32> = lot.roofs.iter().map(|r| r.level).collect();
        levels.sort();
        levels.dedup();
        for l in levels {
            let mut m = MeshData::default();
            for roof in lot.roofs.iter().filter(|r| r.level == l) {
                m.append(&roof_mesh(roof), glam::Mat4::IDENTITY, [1.0; 4]);
            }
            let id = match self.spare.pop() {
                Some(id) => {
                    r.update_mesh(id, &m);
                    id
                }
                None => r.upload_mesh(&m),
            };
            self.roofs.push((l, id));
        }
        // park unused slots as empty meshes
        for id in &self.spare {
            r.update_mesh(*id, &MeshData::default());
        }
    }
}

fn vtx(p: Vec3, n: Vec3, c: [f32; 3]) -> Vertex {
    Vertex { pos: p.to_array(), normal: n.to_array(), color: [c[0], c[1], c[2], 1.0] }
}

fn tri(m: &mut MeshData, a: Vec3, b: Vec3, c: Vec3, col: [f32; 3], up_hint: Vec3) {
    let mut n = (b - a).cross(c - a).normalize_or_zero();
    if n.dot(up_hint) < 0.0 {
        n = -n;
    }
    let base = m.vertices.len() as u32;
    m.vertices.extend([vtx(a, n, col), vtx(b, n, col), vtx(c, n, col)]);
    m.indices.extend([base, base + 1, base + 2]);
}

fn quad(m: &mut MeshData, a: Vec3, b: Vec3, c: Vec3, d: Vec3, col: [f32; 3], up_hint: Vec3) {
    tri(m, a, b, c, col, up_hint);
    tri(m, a, c, d, col, up_hint);
}

/// Grass (and dirt on slopes) for every outdoor ground tile, following the heightmap.
pub fn terrain_mesh(lot: &Lot) -> MeshData {
    let mut m = MeshData::default();
    let h = |c: IVec2| lot.terrain.corner(c);
    for x in 0..lot.width {
        for z in 0..lot.depth {
            let t = IVec2::new(x, z);
            if lot.has_floor(0, t) || !lot.has_ground(0, t) {
                continue;
            }
            let p = |c: IVec2| Vec3::new(c.x as f32, h(c) - 0.01, c.y as f32);
            let (a, b, c, d) = (p(t), p(t + IVec2::X), p(t + IVec2::ONE), p(t + IVec2::Y));
            let v = (((x * 31 + z * 17) % 7) as f32) * 0.006;
            let slope = [a.y, b.y, c.y, d.y].iter().fold(f32::MIN, |m, y| m.max(*y)) - [a.y, b.y, c.y, d.y].iter().fold(f32::MAX, |m, y| m.min(*y));
            let dirt = (slope * 1.2).clamp(0.0, 1.0);
            let height_tint = (a.y * 0.04).clamp(-0.05, 0.08);
            let grass = [0.33 + v + height_tint, 0.5 + v + height_tint, 0.22];
            let mud = [0.45, 0.36, 0.25];
            let col = [grass[0] + (mud[0] - grass[0]) * dirt, grass[1] + (mud[1] - grass[1]) * dirt, grass[2] + (mud[2] - grass[2]) * dirt];
            // split along the flatter diagonal so ridges read correctly
            if (a.y - c.y).abs() <= (b.y - d.y).abs() {
                tri(&mut m, a, b, c, col, Vec3::Y);
                tri(&mut m, a, c, d, col, Vec3::Y);
            } else {
                tri(&mut m, a, b, d, col, Vec3::Y);
                tri(&mut m, b, c, d, col, Vec3::Y);
            }
            // skirts down to the street level along the lot edge so hills don't float
            let skirt = |m: &mut MeshData, p0: Vec3, p1: Vec3, out: Vec3| {
                let lo = -0.1;
                if p0.y > lo || p1.y > lo {
                    quad(m, Vec3::new(p0.x, lo, p0.z), Vec3::new(p1.x, lo, p1.z), p1, p0, [0.4, 0.32, 0.22], out);
                }
            };
            if x == 0 {
                skirt(&mut m, a, d, -Vec3::X);
            }
            if z == 0 {
                skirt(&mut m, a, b, -Vec3::Z);
            }
            if x == lot.width - 1 {
                skirt(&mut m, b, c, Vec3::X);
            }
            if z == lot.depth - 1 {
                skirt(&mut m, d, c, Vec3::Z);
            }
        }
    }
    m.fix_winding();
    m
}

pub fn roof_color(style: u16) -> [f32; 3] {
    match style {
        1 => [0.3, 0.32, 0.36],
        2 => [0.35, 0.45, 0.32],
        _ => [0.62, 0.3, 0.22],
    }
}

const OVERHANG: f32 = 0.3;

/// Geometry for one roof, including gable-end walls.
pub fn roof_mesh(r: &Roof) -> MeshData {
    let mut m = MeshData::default();
    let col = roof_color(r.style);
    let gable_wall = crate::scene::wall_color(0);
    let base = r.base_y();
    let (x0, z0, x1, z1) = (r.min.x as f32, r.min.y as f32, r.max.x as f32, r.max.y as f32);
    let (w, d) = (x1 - x0, z1 - z0);
    let rise = r.rise();
    let top = base + rise;
    let drop = OVERHANG * r.pitch;
    let (ox0, oz0, ox1, oz1) = (x0 - OVERHANG, z0 - OVERHANG, x1 + OVERHANG, z1 + OVERHANG);
    let center = Vec3::new((x0 + x1) * 0.5, base, (z0 + z1) * 0.5);
    let out = |p: Vec3| (p - center) * Vec3::new(1.0, 0.0, 1.0) + Vec3::Y * 0.5;
    let thick = 0.12;
    match r.kind {
        RoofKind::Flat => {
            let y0 = base;
            let y1 = base + 0.15;
            let c = |x, y, z| Vec3::new(x, y, z);
            let (a0, b0, c0, d0) = (c(ox0, y0, oz0), c(ox1, y0, oz0), c(ox1, y0, oz1), c(ox0, y0, oz1));
            let (a1, b1, c1, d1) = (c(ox0, y1, oz0), c(ox1, y1, oz0), c(ox1, y1, oz1), c(ox0, y1, oz1));
            quad(&mut m, a1, b1, c1, d1, [0.42, 0.42, 0.44], Vec3::Y);
            quad(&mut m, a0, b0, b1, a1, col, -Vec3::Z);
            quad(&mut m, b0, c0, c1, b1, col, Vec3::X);
            quad(&mut m, c0, d0, d1, c1, col, Vec3::Z);
            quad(&mut m, d0, a0, a1, d1, col, -Vec3::X);
            quad(&mut m, a0, b0, c0, d0, col, -Vec3::Y);
        }
        RoofKind::Gable => {
            let along_x = w >= d;
            if along_x {
                let zm = (z0 + z1) * 0.5;
                let r0 = Vec3::new(ox0, top, zm);
                let r1 = Vec3::new(ox1, top, zm);
                let e0 = |z: f32| (Vec3::new(ox0, base - drop, z), Vec3::new(ox1, base - drop, z));
                let (a, b) = e0(oz0);
                let (c, dd) = e0(oz1);
                quad(&mut m, a, b, r1, r0, col, out(Vec3::new(center.x, 0.0, z0)));
                quad(&mut m, dd, c, r0, r1, col, out(Vec3::new(center.x, 0.0, z1)));
                // underside so it reads from below too
                quad(&mut m, a - Vec3::Y * thick, b - Vec3::Y * thick, r1 - Vec3::Y * thick, r0 - Vec3::Y * thick, [0.5, 0.45, 0.4], -Vec3::Y);
                quad(&mut m, dd - Vec3::Y * thick, c - Vec3::Y * thick, r0 - Vec3::Y * thick, r1 - Vec3::Y * thick, [0.5, 0.45, 0.4], -Vec3::Y);
                for x in [x0, x1] {
                    let n = if x == x0 { -Vec3::X } else { Vec3::X };
                    tri(&mut m, Vec3::new(x, base, z0), Vec3::new(x, base, z1), Vec3::new(x, top - 0.05, zm), gable_wall, n);
                }
            } else {
                let xm = (x0 + x1) * 0.5;
                let r0 = Vec3::new(xm, top, oz0);
                let r1 = Vec3::new(xm, top, oz1);
                let a = Vec3::new(ox0, base - drop, oz0);
                let b = Vec3::new(ox0, base - drop, oz1);
                let c = Vec3::new(ox1, base - drop, oz0);
                let dd = Vec3::new(ox1, base - drop, oz1);
                quad(&mut m, a, b, r1, r0, col, out(Vec3::new(x0, 0.0, center.z)));
                quad(&mut m, c, dd, r1, r0, col, out(Vec3::new(x1, 0.0, center.z)));
                quad(&mut m, a - Vec3::Y * thick, b - Vec3::Y * thick, r1 - Vec3::Y * thick, r0 - Vec3::Y * thick, [0.5, 0.45, 0.4], -Vec3::Y);
                quad(&mut m, c - Vec3::Y * thick, dd - Vec3::Y * thick, r1 - Vec3::Y * thick, r0 - Vec3::Y * thick, [0.5, 0.45, 0.4], -Vec3::Y);
                for z in [z0, z1] {
                    let n = if z == z0 { -Vec3::Z } else { Vec3::Z };
                    tri(&mut m, Vec3::new(x0, base, z), Vec3::new(x1, base, z), Vec3::new(xm, top - 0.05, z), gable_wall, n);
                }
            }
        }
        RoofKind::Hip => {
            let half = w.min(d) * 0.5;
            let (r0, r1) = if w >= d {
                let zm = (z0 + z1) * 0.5;
                (Vec3::new(x0 + half, top, zm), Vec3::new(x1 - half, top, zm))
            } else {
                let xm = (x0 + x1) * 0.5;
                (Vec3::new(xm, top, z0 + half), Vec3::new(xm, top, z1 - half))
            };
            let a = Vec3::new(ox0, base - drop, oz0);
            let b = Vec3::new(ox1, base - drop, oz0);
            let c = Vec3::new(ox1, base - drop, oz1);
            let dd = Vec3::new(ox0, base - drop, oz1);
            // which ridge end is nearest each corner
            let near = |p: Vec3| if (p - r0).length_squared() <= (p - r1).length_squared() { r0 } else { r1 };
            let faces = [(a, b, -Vec3::Z), (b, c, Vec3::X), (c, dd, Vec3::Z), (dd, a, -Vec3::X)];
            for (p, q, n) in faces {
                let (np, nq) = (near(p), near(q));
                let hint = n + Vec3::Y * 0.5;
                if np == nq {
                    tri(&mut m, p, q, np, col, hint);
                } else {
                    quad(&mut m, p, q, nq, np, col, hint);
                }
                let s = Vec3::Y * thick;
                if np == nq {
                    tri(&mut m, p - s, q - s, np - s, [0.5, 0.45, 0.4], -Vec3::Y);
                } else {
                    quad(&mut m, p - s, q - s, nq - s, np - s, [0.5, 0.45, 0.4], -Vec3::Y);
                }
            }
        }
    }
    m.fix_winding();
    let _ = LEVEL_HEIGHT;
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roof_meshes_are_closed_enough_and_above_walls() {
        for kind in [RoofKind::Gable, RoofKind::Hip, RoofKind::Flat] {
            for (min, max) in [(IVec2::new(0, 0), IVec2::new(6, 4)), (IVec2::new(2, 2), IVec2::new(5, 9)), (IVec2::new(0, 0), IVec2::new(4, 4))] {
                let r = Roof { level: 0, min, max, kind, pitch: 0.5, style: 0 };
                let m = roof_mesh(&r);
                assert!(!m.indices.is_empty());
                assert_eq!(m.indices.len() % 3, 0);
                let ys: Vec<f32> = m.vertices.iter().map(|v| v.pos[1]).collect();
                let top = ys.iter().cloned().fold(f32::MIN, f32::max);
                assert!((top - (r.base_y() + r.rise())).abs() < 0.2, "{kind:?} top {top}");
                assert!(ys.iter().all(|y| *y > r.base_y() - 0.6));
            }
        }
    }

    #[test]
    fn terrain_mesh_follows_heights() {
        let mut lot = Lot::new(6, 6);
        lot.terrain.set_corner(IVec2::new(3, 3), 1.5);
        let m = terrain_mesh(&lot);
        let top = m.vertices.iter().map(|v| v.pos[1]).fold(f32::MIN, f32::max);
        assert!((top - 1.49).abs() < 0.02);
    }
}
