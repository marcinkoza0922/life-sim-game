//! Fonts (bundled OFL faces), a glyph atlas and simple word-wrapping layout.

use std::collections::HashMap;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, VariableFont, point};
use glam::{Vec2, vec2};

/// Typefaces bundled with the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontId {
    /// Nunito semibold: body text.
    Body,
    /// Nunito extra-bold: labels on buttons, emphasis.
    Bold,
    /// Fredoka: chunky rounded headings and numbers.
    Display,
}

const EMOJI: u8 = 3;
pub const ATLAS_SIZE: u32 = 2048;

#[derive(Clone, Copy, Debug)]
pub struct PGlyph {
    pub face: u8,
    pub id: GlyphId,
    /// Pen position (left, baseline) relative to the galley's top-left, in points.
    pub pos: Vec2,
    pub advance: f32,
    /// Byte index of the source character.
    pub byte: usize,
}

/// Laid-out text.
#[derive(Clone, Debug, Default)]
pub struct Galley {
    pub glyphs: Vec<PGlyph>,
    pub size: Vec2,
    pub font_size: f32,
    pub line_height: f32,
}

impl Galley {
    /// The caret x position (points) before the given byte index.
    pub fn caret_x(&self, byte: usize) -> f32 {
        let mut x = 0.0;
        for g in &self.glyphs {
            if g.byte >= byte {
                return g.pos.x;
            }
            x = g.pos.x + g.advance;
        }
        x
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphEntry {
    /// Normalised atlas rect: u0, v0, u1, v1.
    pub uv: [f32; 4],
    /// Bitmap top-left relative to the pen, in pixels.
    pub offset: Vec2,
    /// Bitmap size in pixels.
    pub size: Vec2,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: u8,
    id: u16,
    /// Pixel scale in quarter pixels.
    px4: u32,
}

pub struct Atlas {
    pub pixels: Vec<u8>,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    /// Rows touched since the last upload (y0..y1).
    pub dirty: Option<(u32, u32)>,
    pub full: bool,
}

impl Atlas {
    fn new() -> Atlas {
        Atlas { pixels: vec![0; (ATLAS_SIZE * ATLAS_SIZE) as usize], shelf_x: 1, shelf_y: 1, shelf_h: 0, dirty: Some((0, ATLAS_SIZE)), full: false }
    }

    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if self.shelf_x + w + 1 > ATLAS_SIZE {
            self.shelf_y += self.shelf_h + 1;
            self.shelf_x = 1;
            self.shelf_h = 0;
        }
        if self.shelf_y + h + 1 > ATLAS_SIZE {
            self.full = true;
            return None;
        }
        let at = (self.shelf_x, self.shelf_y);
        self.shelf_x += w + 1;
        self.shelf_h = self.shelf_h.max(h);
        Some(at)
    }

    fn mark(&mut self, y0: u32, y1: u32) {
        self.dirty = Some(match self.dirty {
            Some((a, b)) => (a.min(y0), b.max(y1)),
            None => (y0, y1),
        });
    }
}

pub struct Fonts {
    faces: [FontRef<'static>; 4],
    cache: HashMap<GlyphKey, Option<GlyphEntry>>,
    pub atlas: Atlas,
}

fn face(bytes: &'static [u8], axes: &[(&[u8; 4], f32)]) -> FontRef<'static> {
    let mut f = FontRef::try_from_slice(bytes).expect("bundled font parses");
    for (tag, v) in axes {
        f.set_variation(tag, *v);
    }
    f
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    pub fn new() -> Fonts {
        static NUNITO: &[u8] = include_bytes!("../../../assets/fonts/Nunito.ttf");
        static FREDOKA: &[u8] = include_bytes!("../../../assets/fonts/Fredoka.ttf");
        static EMOJI_TTF: &[u8] = include_bytes!("../../../assets/fonts/NotoEmoji.ttf");
        Fonts {
            faces: [face(NUNITO, &[(b"wght", 680.0)]), face(NUNITO, &[(b"wght", 860.0)]), face(FREDOKA, &[(b"wght", 600.0), (b"wdth", 100.0)]), face(EMOJI_TTF, &[(b"wght", 600.0)])],
            cache: HashMap::new(),
            atlas: Atlas::new(),
        }
    }

    fn px_scale(&self, face: u8, size: f32) -> PxScale {
        let f = &self.faces[face as usize];
        let upem = f.units_per_em().unwrap_or(1000.0);
        PxScale::from(size * f.height_unscaled() / upem)
    }

    /// Lays out `text` at `size` points, wrapping at `wrap` points if given.
    pub fn layout(&self, text: &str, font: FontId, size: f32, wrap: Option<f32>) -> Galley {
        let main = font as u8;
        let mf = self.faces[main as usize].as_scaled(self.px_scale(main, size));
        let ascent = mf.ascent();
        let line_h = (mf.ascent() - mf.descent() + mf.line_gap()).ceil();
        let mut lines: Vec<Vec<PGlyph>> = Vec::new();
        let mut line: Vec<PGlyph> = Vec::new();
        let mut x = 0.0f32;
        let mut brk: Option<usize> = None;
        let mut prev: Option<(u8, GlyphId)> = None;
        for (byte, ch) in text.char_indices() {
            if ch == '\n' {
                lines.push(std::mem::take(&mut line));
                x = 0.0;
                brk = None;
                prev = None;
                continue;
            }
            if ch == '\u{FE0F}' || ch == '\u{200D}' || ch == '\r' {
                continue;
            }
            let mut face = main;
            let mut id = self.faces[main as usize].glyph_id(ch);
            if id.0 == 0 {
                let e = self.faces[EMOJI as usize].glyph_id(ch);
                if e.0 != 0 {
                    face = EMOJI;
                    id = e;
                }
            }
            let sf = self.faces[face as usize].as_scaled(self.px_scale(face, size));
            if let Some((pf, pid)) = prev
                && pf == face
            {
                x += sf.kern(pid, id);
            }
            let adv = sf.h_advance(id);
            if let Some(w) = wrap
                && x + adv > w
                && ch != ' '
                && let Some(b) = brk
                && b < line.len()
            {
                let rest = line.split_off(b);
                lines.push(std::mem::take(&mut line));
                let dx = rest.first().map(|g| g.pos.x).unwrap_or(0.0);
                line = rest.into_iter().map(|mut g| {
                    g.pos.x -= dx;
                    g
                }).collect();
                x = line.last().map(|g| g.pos.x + g.advance).unwrap_or(0.0);
                brk = None;
            }
            line.push(PGlyph { face, id, pos: vec2(x, 0.0), advance: adv, byte });
            x += adv;
            prev = Some((face, id));
            if ch == ' ' || ch == '-' || ch == '/' {
                brk = Some(line.len());
            }
        }
        lines.push(line);
        let mut glyphs = Vec::new();
        let mut width = 0.0f32;
        for (i, l) in lines.iter().enumerate() {
            // trailing spaces don't count towards the width
            let w = l.iter().rev().find(|g| self.faces[g.face as usize].glyph_id(' ') != g.id).map(|g| g.pos.x + g.advance).unwrap_or(0.0);
            width = width.max(w);
            for g in l {
                let mut g = *g;
                g.pos.y = i as f32 * line_h + ascent;
                glyphs.push(g);
            }
        }
        Galley { glyphs, size: vec2(width.ceil(), line_h * lines.len() as f32), font_size: size, line_height: line_h }
    }

    /// The rasterised glyph (None for blank glyphs such as spaces).
    pub fn glyph(&mut self, g: &PGlyph, size: f32, ppp: f32) -> Option<GlyphEntry> {
        let scale = self.px_scale(g.face, size * ppp);
        let key = GlyphKey { face: g.face, id: g.id.0, px4: (scale.y * 4.0).round() as u32 };
        if let Some(e) = self.cache.get(&key) {
            return *e;
        }
        let font = &self.faces[g.face as usize];
        let entry = font.outline_glyph(g.id.with_scale_and_position(scale, point(0.0, 0.0))).and_then(|og| {
            let b = og.px_bounds();
            let (w, h) = (b.width().ceil() as u32, b.height().ceil() as u32);
            if w == 0 || h == 0 {
                return None;
            }
            let (ax, ay) = self.atlas.alloc(w, h)?;
            let atlas = &mut self.atlas;
            og.draw(|x, y, c| {
                if x < w && y < h {
                    let i = ((ay + y) * ATLAS_SIZE + ax + x) as usize;
                    atlas.pixels[i] = (c.clamp(0.0, 1.0) * 255.0) as u8;
                }
            });
            atlas.mark(ay, ay + h);
            let s = ATLAS_SIZE as f32;
            Some(GlyphEntry {
                uv: [ax as f32 / s, ay as f32 / s, (ax + w) as f32 / s, (ay + h) as f32 / s],
                offset: vec2(b.min.x, b.min.y),
                size: vec2(w as f32, h as f32),
            })
        });
        // don't cache misses caused by a full atlas: they'll succeed after the reset
        if entry.is_some() || !self.atlas.full {
            self.cache.insert(key, entry);
        }
        entry
    }

    /// Starts over with an empty atlas if it ran out of room last frame.
    pub fn maybe_reset(&mut self) {
        if self.atlas.full {
            log::info!("glyph atlas full; rebuilding");
            self.cache.clear();
            self.atlas = Atlas::new();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_on_spaces() {
        let f = Fonts::new();
        let one = f.layout("hello there friend", FontId::Body, 14.0, None);
        assert!((one.size.y - one.line_height).abs() < 0.01);
        let wrapped = f.layout("hello there friend", FontId::Body, 14.0, Some(one.size.x * 0.6));
        assert!(wrapped.size.y > one.size.y * 1.5);
        assert!(wrapped.size.x <= one.size.x * 0.6 + 1.0);
    }

    #[test]
    fn falls_back_to_emoji_and_rasterises() {
        let mut f = Fonts::new();
        let g = f.layout("A🍼", FontId::Body, 16.0, None);
        assert_eq!(g.glyphs.len(), 2);
        assert_eq!(g.glyphs[1].face, EMOJI);
        assert!(f.glyph(&g.glyphs[0], 16.0, 1.0).is_some());
        assert!(f.glyph(&g.glyphs[1], 16.0, 1.0).is_some());
    }
}
