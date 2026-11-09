//! The draw list: every shape is one instanced quad shaded by a signed distance function, so
//! corners stay round and edges stay soft at any scale or rotation.

use bytemuck::{Pod, Zeroable};
use glam::{Affine2, Vec2, vec2};

use crate::geom::{Color, Rect};
use crate::images::{ImageId, Images};
use crate::text::{Fonts, Galley};

/// Draw/hit-test layers, back to front.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Order {
    /// World-space labels: drawn under everything and never block the pointer.
    Background,
    Panel,
    Window,
    Popup,
    Tooltip,
}

pub const ORDERS: usize = 5;

pub(crate) const KIND_RRECT: u32 = 0;
pub(crate) const KIND_GLYPH: u32 = 1;
pub(crate) const KIND_TRI: u32 = 2;
pub(crate) const KIND_ARC: u32 = 3;
pub(crate) const KIND_IMAGE: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Instance {
    pub center: [f32; 2],
    pub half: [f32; 2],
    pub rot: f32,
    pub kind: u32,
    pub radius: f32,
    pub border: f32,
    pub fill0: [f32; 4],
    pub fill1: [f32; 4],
    pub stroke: [f32; 4],
    /// Glyph and image: uv rect. Arc: half-aperture, ring radius, half thickness.
    pub params: [f32; 4],
    pub clip: [f32; 4],
    pub soft: f32,
    pub _pad: [f32; 3],
}

/// Fill, gradient and outline for a shape.
#[derive(Clone, Copy, Debug)]
pub struct Paint {
    pub fill: Color,
    /// Bottom colour for a vertical gradient (defaults to `fill`).
    pub fill_bottom: Option<Color>,
    pub stroke: Option<(f32, Color)>,
}

impl Paint {
    pub fn fill(c: Color) -> Paint {
        Paint { fill: c, fill_bottom: None, stroke: None }
    }
    pub fn gradient(top: Color, bottom: Color) -> Paint {
        Paint { fill: top, fill_bottom: Some(bottom), stroke: None }
    }
    pub fn stroke(mut self, w: f32, c: Color) -> Paint {
        self.stroke = Some((w, c));
        self
    }
}

pub struct Painter {
    pub(crate) layers: [Vec<Instance>; ORDERS],
    pub order: Order,
    pub clip: Rect,
    xf: Affine2,
    xf_stack: Vec<Affine2>,
    pub ppp: f32,
}

impl Default for Painter {
    fn default() -> Self {
        Painter { layers: Default::default(), order: Order::Panel, clip: Rect::EVERYTHING, xf: Affine2::IDENTITY, xf_stack: Vec::new(), ppp: 1.0 }
    }
}

impl Painter {
    pub(crate) fn clear(&mut self) {
        for l in &mut self.layers {
            l.clear();
        }
        self.xf = Affine2::IDENTITY;
        self.xf_stack.clear();
        self.clip = Rect::EVERYTHING;
    }

    /// Scales by `scale` and rotates by `rot` (radians, clockwise) around `pivot`, then offsets.
    pub fn push_transform(&mut self, pivot: Vec2, scale: Vec2, rot: f32, offset: Vec2) {
        self.xf_stack.push(self.xf);
        let local = Affine2::from_translation(pivot + offset) * Affine2::from_angle(rot) * Affine2::from_scale(scale) * Affine2::from_translation(-pivot);
        self.xf *= local;
    }

    pub fn pop_transform(&mut self) {
        self.xf = self.xf_stack.pop().unwrap_or(Affine2::IDENTITY);
    }

    pub fn is_identity(&self) -> bool {
        self.xf == Affine2::IDENTITY
    }

    fn push(&mut self, mut i: Instance) {
        if self.xf != Affine2::IDENTITY {
            let m = self.xf.matrix2;
            let (sx, sy) = (m.x_axis.length(), m.y_axis.length());
            let c = self.xf.transform_point2(Vec2::from(i.center));
            i.center = c.into();
            i.half = [i.half[0] * sx, i.half[1] * sy];
            i.rot += m.x_axis.y.atan2(m.x_axis.x);
            let s = sx.min(sy);
            i.radius *= s;
            i.border *= s;
            if i.kind == KIND_ARC {
                i.params[1] *= s;
                i.params[2] *= s;
            }
        }
        let c = self.clip;
        i.clip = [c.min.x, c.min.y, c.max.x, c.max.y];
        self.layers[self.order as usize].push(i);
    }

    fn base(center: Vec2, half: Vec2, kind: u32, paint: &Paint) -> Instance {
        let (bw, bc) = paint.stroke.unwrap_or((0.0, Color::TRANSPARENT));
        Instance {
            center: center.into(),
            half: half.into(),
            rot: 0.0,
            kind,
            radius: 0.0,
            border: bw,
            fill0: paint.fill.0,
            fill1: paint.fill_bottom.unwrap_or(paint.fill).0,
            stroke: bc.0,
            params: [0.0; 4],
            clip: [0.0; 4],
            soft: 0.0,
            _pad: [0.0; 3],
        }
    }

    pub fn rect(&mut self, r: Rect, radius: f32, paint: Paint) {
        let mut i = Self::base(r.center(), r.size() * 0.5, KIND_RRECT, &paint);
        i.radius = radius.min(r.width() * 0.5).min(r.height() * 0.5).max(0.0);
        self.push(i);
    }

    pub fn rect_rotated(&mut self, r: Rect, radius: f32, rot: f32, paint: Paint) {
        let mut i = Self::base(r.center(), r.size() * 0.5, KIND_RRECT, &paint);
        i.radius = radius.min(r.width() * 0.5).min(r.height() * 0.5).max(0.0);
        i.rot = rot;
        self.push(i);
    }

    /// A blurred rounded rectangle.
    pub fn shadow(&mut self, r: Rect, radius: f32, blur: f32, color: Color) {
        let mut i = Self::base(r.center(), r.size() * 0.5, KIND_RRECT, &Paint::fill(color));
        i.radius = radius.min(r.width() * 0.5).min(r.height() * 0.5);
        i.soft = blur;
        self.push(i);
    }

    pub fn circle(&mut self, c: Vec2, r: f32, paint: Paint) {
        let mut i = Self::base(c, Vec2::splat(r), KIND_RRECT, &paint);
        i.radius = r;
        self.push(i);
    }

    /// An isosceles triangle pointing up (before `rot`), inside a box of `half` extents.
    pub fn triangle(&mut self, c: Vec2, half: Vec2, rot: f32, round: f32, paint: Paint) {
        let mut i = Self::base(c, half, KIND_TRI, &paint);
        i.rot = rot;
        i.radius = round;
        self.push(i);
    }

    /// A ring segment of radius `r` and `thickness`, starting at 12 o'clock and running clockwise
    /// through `frac` of the circle (round caps).
    pub fn arc(&mut self, c: Vec2, r: f32, thickness: f32, frac: f32, color: Color) {
        self.arc_at(c, r, thickness, frac, frac.clamp(0.0, 1.0) * std::f32::consts::PI, color);
    }

    /// A ring segment covering `frac` of the circle, centred on `angle` (radians clockwise from
    /// 12 o'clock): `angle = PI` with a small `frac` is a smile.
    pub fn arc_at(&mut self, c: Vec2, r: f32, thickness: f32, frac: f32, angle: f32, color: Color) {
        let frac = frac.clamp(0.0, 1.0);
        if frac <= 0.0 {
            return;
        }
        let ext = r + thickness;
        let mut i = Self::base(c, Vec2::splat(ext), KIND_ARC, &Paint::fill(color));
        i.params = [frac * std::f32::consts::PI, r, thickness * 0.5, 0.0];
        i.rot = angle;
        self.push(i);
    }

    /// A straight line with round caps.
    pub fn line(&mut self, a: Vec2, b: Vec2, w: f32, color: Color) {
        let d = b - a;
        let len = d.length();
        let mut i = Self::base((a + b) * 0.5, vec2(len * 0.5 + w * 0.5, w * 0.5), KIND_RRECT, &Paint::fill(color));
        i.radius = w * 0.5;
        i.rot = d.y.atan2(d.x);
        self.push(i);
    }

    /// Draws image `id` stretched over `r`, multiplied by `tint` (WHITE = as is), with rounded
    /// corners and an optional outline from `stroke`.
    pub fn image(&mut self, images: &Images, id: ImageId, r: Rect, radius: f32, tint: Color, stroke: Option<(f32, Color)>) {
        let mut i = Self::base(r.center(), r.size() * 0.5, KIND_IMAGE, &Paint { fill: tint, fill_bottom: None, stroke });
        i.radius = radius.min(r.width() * 0.5).min(r.height() * 0.5).max(0.0);
        i.params = images.entry(id).uv;
        self.push(i);
    }

    /// Draws laid-out text with its top-left at `pos`.
    pub fn galley(&mut self, fonts: &mut Fonts, g: &Galley, pos: Vec2, color: Color) {
        let ppp = self.ppp;
        let snap = self.is_identity();
        for pg in &g.glyphs {
            let Some(e) = fonts.glyph(pg, g.font_size, ppp) else { continue };
            let mut pen = (pos + pg.pos) * ppp;
            if snap {
                pen = pen.round();
            }
            let tl = (pen + e.offset) / ppp;
            let size = e.size / ppp;
            let mut i = Self::base(tl + size * 0.5, size * 0.5, KIND_GLYPH, &Paint::fill(color));
            i.params = e.uv;
            self.push(i);
        }
    }
}
