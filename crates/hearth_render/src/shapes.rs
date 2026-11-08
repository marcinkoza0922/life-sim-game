//! Procedural primitive meshes. All are unit-sized and centered at the origin
//! (box: 1×1×1, cylinder: radius 0.5 height 1, sphere: radius 0.5).

use glam::Vec3;

use crate::{MeshData, Vertex};

fn v(p: Vec3, n: Vec3) -> Vertex {
    Vertex { pos: p.to_array(), normal: n.to_array(), color: [1.0; 4] }
}

pub fn cube() -> MeshData {
    let mut m = MeshData::default();
    let faces = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (-Vec3::X, Vec3::Y, -Vec3::Z),
        (Vec3::Y, Vec3::Z, Vec3::X),
        (-Vec3::Y, Vec3::Z, -Vec3::X),
        (Vec3::Z, Vec3::Y, -Vec3::X),
        (-Vec3::Z, Vec3::Y, Vec3::X),
    ];
    for (n, up, right) in faces {
        let c = n * 0.5;
        let base = m.vertices.len() as u32;
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            m.vertices.push(v(c + right * 0.5 * a + up * 0.5 * b, n));
        }
        m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    m.fix_winding();
    m
}

pub fn cylinder(segments: u32) -> MeshData {
    let mut m = MeshData::default();
    let seg = segments.max(3);
    for i in 0..=seg {
        let a = i as f32 / seg as f32 * std::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        let n = Vec3::new(c, 0.0, s);
        m.vertices.push(v(Vec3::new(c * 0.5, -0.5, s * 0.5), n));
        m.vertices.push(v(Vec3::new(c * 0.5, 0.5, s * 0.5), n));
    }
    for i in 0..seg {
        let b = i * 2;
        m.indices.extend([b, b + 1, b + 3, b, b + 3, b + 2]);
    }
    for (y, n) in [(0.5f32, Vec3::Y), (-0.5, -Vec3::Y)] {
        let center = m.vertices.len() as u32;
        m.vertices.push(v(Vec3::new(0.0, y, 0.0), n));
        for i in 0..=seg {
            let a = i as f32 / seg as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            m.vertices.push(v(Vec3::new(c * 0.5, y, s * 0.5), n));
        }
        for i in 0..seg {
            m.indices.extend([center, center + 1 + i, center + 2 + i]);
        }
    }
    m.fix_winding();
    m
}

pub fn sphere(rings: u32, segments: u32) -> MeshData {
    let mut m = MeshData::default();
    for r in 0..=rings {
        let phi = r as f32 / rings as f32 * std::f32::consts::PI;
        for s in 0..=segments {
            let th = s as f32 / segments as f32 * std::f32::consts::TAU;
            let n = Vec3::new(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin());
            m.vertices.push(v(n * 0.5, n));
        }
    }
    let w = segments + 1;
    for r in 0..rings {
        for s in 0..segments {
            let a = r * w + s;
            m.indices.extend([a, a + w, a + 1, a + 1, a + w, a + w + 1]);
        }
    }
    m.fix_winding();
    m
}

/// A flat disc in the XZ plane (radius 0.5), facing +Y.
pub fn disc(segments: u32) -> MeshData {
    let mut m = MeshData::default();
    m.vertices.push(v(Vec3::ZERO, Vec3::Y));
    for i in 0..=segments {
        let a = i as f32 / segments as f32 * std::f32::consts::TAU;
        m.vertices.push(v(Vec3::new(a.cos() * 0.5, 0.0, a.sin() * 0.5), Vec3::Y));
    }
    for i in 0..segments {
        m.indices.extend([0, 1 + i, 2 + i]);
    }
    m.fix_winding();
    m
}

/// A right-triangle prism (wedge) for roofs/ramps: rises from y=0 at z=+0.5 to y=1 at z=-0.5.
pub fn wedge() -> MeshData {
    let mut m = MeshData::default();
    let p = [
        Vec3::new(-0.5, -0.5, 0.5),
        Vec3::new(0.5, -0.5, 0.5),
        Vec3::new(0.5, -0.5, -0.5),
        Vec3::new(-0.5, -0.5, -0.5),
        Vec3::new(-0.5, 0.5, -0.5),
        Vec3::new(0.5, 0.5, -0.5),
    ];
    let mut quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| {
        let n = (b - a).cross(c - a).normalize();
        let base = m.vertices.len() as u32;
        for q in [a, b, c, d] {
            m.vertices.push(v(q, n));
        }
        m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    quad(p[0], p[1], p[2], p[3]); // bottom
    quad(p[3], p[2], p[5], p[4]); // back
    quad(p[0], p[4], p[5], p[1]); // slope
    let mut tri = |a: Vec3, b: Vec3, c: Vec3| {
        let n = (b - a).cross(c - a).normalize();
        let base = m.vertices.len() as u32;
        for q in [a, b, c] {
            m.vertices.push(v(q, n));
        }
        m.indices.extend([base, base + 1, base + 2]);
    };
    tri(p[0], p[3], p[4]);
    tri(p[1], p[5], p[2]);
    m.fix_winding();
    m
}
