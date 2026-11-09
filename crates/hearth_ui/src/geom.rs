//! Rectangles and colours. All UI coordinates are logical points (y down).

use glam::{Vec2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    pub const EVERYTHING: Rect = Rect { min: Vec2::splat(-1e9), max: Vec2::splat(1e9) };

    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { min: vec2(x, y), max: vec2(x + w, y + h) }
    }
    pub fn from_min_size(min: Vec2, size: Vec2) -> Rect {
        Rect { min, max: min + size }
    }
    pub fn from_center_size(c: Vec2, size: Vec2) -> Rect {
        Rect { min: c - size * 0.5, max: c + size * 0.5 }
    }
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }
    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }
    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.y >= self.min.y && p.x < self.max.x && p.y < self.max.y
    }
    pub fn shrink(&self, by: f32) -> Rect {
        self.shrink2(Vec2::splat(by))
    }
    pub fn shrink2(&self, by: Vec2) -> Rect {
        Rect { min: self.min + by, max: self.max - by }
    }
    pub fn expand(&self, by: f32) -> Rect {
        self.shrink(-by)
    }
    pub fn translate(&self, v: Vec2) -> Rect {
        Rect { min: self.min + v, max: self.max + v }
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        Rect { min: self.min.max(o.min), max: self.max.min(o.max) }
    }
    pub fn union(&self, o: &Rect) -> Rect {
        Rect { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn left(&self) -> f32 {
        self.min.x
    }
    pub fn right(&self) -> f32 {
        self.max.x
    }
    pub fn top(&self) -> f32 {
        self.min.y
    }
    pub fn bottom(&self) -> f32 {
        self.max.y
    }
}

/// Non-premultiplied sRGB colour, 0..1 per channel. The UI blends in gamma space.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Color(pub [f32; 4]);

impl Color {
    pub const TRANSPARENT: Color = Color([0.0; 4]);
    pub const WHITE: Color = Color([1.0; 4]);
    pub const BLACK: Color = Color([0.0, 0.0, 0.0, 1.0]);

    pub const fn hex(rgb: u32) -> Color {
        Color([((rgb >> 16) & 0xff) as f32 / 255.0, ((rgb >> 8) & 0xff) as f32 / 255.0, (rgb & 0xff) as f32 / 255.0, 1.0])
    }
    pub fn rgb(c: [f32; 3]) -> Color {
        Color([c[0], c[1], c[2], 1.0])
    }
    pub fn alpha(self, a: f32) -> Color {
        let [r, g, b, a0] = self.0;
        Color([r, g, b, a0 * a])
    }
    pub fn mix(self, o: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let mut c = [0.0; 4];
        for (i, v) in c.iter_mut().enumerate() {
            *v = self.0[i] + (o.0[i] - self.0[i]) * t;
        }
        Color(c)
    }
    pub fn lighten(self, t: f32) -> Color {
        let a = self.0[3];
        self.mix(Color::WHITE, t).with_a(a)
    }
    pub fn darken(self, t: f32) -> Color {
        let a = self.0[3];
        self.mix(Color::BLACK, t).with_a(a)
    }
    pub fn with_a(self, a: f32) -> Color {
        let [r, g, b, _] = self.0;
        Color([r, g, b, a])
    }
    /// Greyed-out version for disabled widgets.
    pub fn desaturate(self, t: f32) -> Color {
        let [r, g, b, a] = self.0;
        let l = r * 0.3 + g * 0.59 + b * 0.11;
        Color([r + (l - r) * t, g + (l - g) * t, b + (l - b) * t, a])
    }
    pub fn luminance(self) -> f32 {
        let [r, g, b, _] = self.0;
        r * 0.3 + g * 0.59 + b * 0.11
    }
}
