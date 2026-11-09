//! The toy-box palette: cream paper, plum ink outlines and candy accents.

use glam::vec2;

use crate::geom::{Color, Rect};
use crate::paint::{Paint, Painter};

pub const INK: Color = Color::hex(0x3B2F5C);
pub const INK_SOFT: Color = Color::hex(0x7D7199);
pub const CREAM: Color = Color::hex(0xFFF8EC);
pub const CREAM_DEEP: Color = Color::hex(0xFBE7CB);
pub const PAPER: Color = Color::hex(0xFFFFFF);
pub const TEAL: Color = Color::hex(0x2EC4B6);
pub const CORAL: Color = Color::hex(0xFF6B6B);
pub const SUN: Color = Color::hex(0xFFC94D);
pub const GRASS: Color = Color::hex(0x6CCB7A);
pub const SKY: Color = Color::hex(0x5BB8F5);
pub const LILAC: Color = Color::hex(0xB59CF0);
pub const PINK: Color = Color::hex(0xFF9EC7);
pub const PEACH: Color = Color::hex(0xFFAE7A);

/// Outline width for everything chunky.
pub const OUTLINE: f32 = 2.5;
/// Height of the solid "sticker" shadow under cards and buttons.
pub const LIP: f32 = 4.0;

/// Red → yellow → green for a 0..1 level.
pub fn level_color(frac: f32) -> Color {
    let f = frac.clamp(0.0, 1.0);
    if f < 0.5 { CORAL.mix(SUN, f / 0.5) } else { SUN.mix(GRASS, (f - 0.5) / 0.5) }
}

/// Text colour that reads on `bg`.
pub fn on(bg: Color) -> Color {
    if bg.luminance() > 0.65 { INK } else { PAPER }
}

/// A paper card with a plum outline and a solid drop shadow.
pub fn card(p: &mut Painter, r: Rect, radius: f32, fill: Color) {
    p.rect(r.translate(vec2(0.0, 6.0)), radius, Paint::fill(INK.alpha(0.22)));
    p.rect(r, radius, Paint::gradient(fill.lighten(0.25), fill).stroke(OUTLINE, INK));
}

/// A lighter inset well (for grouping inside cards).
pub fn well(p: &mut Painter, r: Rect, radius: f32) {
    p.rect(r, radius, Paint::fill(INK.alpha(0.06)).stroke(1.5, INK.alpha(0.12)));
}
