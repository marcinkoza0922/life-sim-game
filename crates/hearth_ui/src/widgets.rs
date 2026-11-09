//! Widgets: text, bubbly buttons and chips, bars, sliders, text fields, dropdowns, swatches,
//! cards, scroll areas and pop-in windows.

use std::hash::Hash;

use glam::{Vec2, vec2};

use crate::anim::SpringCfg;
use crate::geom::{Color, Rect};
use crate::paint::{Order, Paint};
use crate::text::FontId;
use crate::theme::{self, CREAM, INK, INK_SOFT, LIP, OUTLINE, PAPER, TEAL};
use crate::{Dir, Id, Key, Layout, Resp, Ui};

#[derive(Clone, Debug)]
pub struct Text {
    pub text: String,
    pub font: FontId,
    pub size: f32,
    pub color: Color,
    pub wrap: bool,
}

impl Text {
    pub fn new(s: impl Into<String>) -> Text {
        Text { text: s.into(), font: FontId::Body, size: 14.0, color: INK, wrap: false }
    }
    pub fn size(mut self, s: f32) -> Text {
        self.size = s;
        self
    }
    pub fn color(mut self, c: Color) -> Text {
        self.color = c;
        self
    }
    pub fn font(mut self, f: FontId) -> Text {
        self.font = f;
        self
    }
    pub fn bold(self) -> Text {
        self.font(FontId::Bold)
    }
    pub fn display(self) -> Text {
        self.font(FontId::Display)
    }
    pub fn wrap(mut self) -> Text {
        self.wrap = true;
        self
    }
    pub fn soft(self) -> Text {
        self.color(INK_SOFT)
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Text {
        Text::new(s)
    }
}

impl From<String> for Text {
    fn from(s: String) -> Text {
        Text::new(s)
    }
}

/// A squashy pill button. With `selected(..)` it becomes a toggle chip (cream when off).
#[derive(Clone, Debug)]
pub struct Button {
    pub label: String,
    pub color: Color,
    pub height: f32,
    pub font: FontId,
    pub font_size: f32,
    pub enabled: bool,
    pub selected: Option<bool>,
    pub min_w: f32,
    pub round: bool,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Button {
        Button { label: label.into(), color: TEAL, height: 34.0, font: FontId::Bold, font_size: 15.0, enabled: true, selected: None, min_w: 0.0, round: false }
    }
    pub fn small(mut self) -> Button {
        self.height = 26.0;
        self.font_size = 13.0;
        self
    }
    pub fn big(mut self) -> Button {
        self.height = 46.0;
        self.font_size = 20.0;
        self.font = FontId::Display;
        self
    }
    pub fn height(mut self, h: f32) -> Button {
        self.height = h;
        self
    }
    pub fn font_size(mut self, s: f32) -> Button {
        self.font_size = s;
        self
    }
    pub fn color(mut self, c: Color) -> Button {
        self.color = c;
        self
    }
    pub fn enabled(mut self, e: bool) -> Button {
        self.enabled = e;
        self
    }
    pub fn selected(mut self, s: bool) -> Button {
        self.selected = Some(s);
        self
    }
    pub fn min_w(mut self, w: f32) -> Button {
        self.min_w = w;
        self
    }
    /// A circle (the label should be an icon or one or two characters).
    pub fn round(mut self) -> Button {
        self.round = true;
        self
    }
}

impl Ui {
    // --------------------------------------------------------------------------------------------
    // Text

    pub fn text(&mut self, t: Text) -> Rect {
        let wrap = t.wrap.then(|| self.avail_w().max(20.0));
        let g = self.fonts.layout(&t.text, t.font, t.size, wrap);
        let r = self.alloc(g.size);
        self.painter.galley(&mut self.fonts, &g, r.min, t.color);
        r
    }

    pub fn label(&mut self, s: impl Into<Text>) -> Rect {
        self.text(s.into())
    }

    /// Small soft wrapped text for hints and descriptions.
    pub fn small(&mut self, s: impl Into<String>) -> Rect {
        self.text(Text::new(s).size(12.5).soft().wrap())
    }

    pub fn heading(&mut self, s: impl Into<String>) -> Rect {
        self.text(Text::new(s).display().size(24.0))
    }

    /// A section title with a coloured blob.
    pub fn section(&mut self, s: impl Into<String>, color: Color) {
        let s = s.into();
        self.row(24.0, |ui| {
            let r = ui.alloc(vec2(12.0, 12.0));
            ui.painter.rect_rotated(r, 4.0, 0.3, Paint::fill(color).stroke(2.0, INK));
            ui.text(Text::new(s).display().size(17.0));
        });
    }

    /// Paints text anchored at `pos` (`anchor` 0..1 in each axis), without layout.
    pub fn paint_text(&mut self, pos: Vec2, anchor: Vec2, text: &str, font: FontId, size: f32, color: Color) -> Rect {
        let g = self.fonts.layout(text, font, size, None);
        let min = pos - g.size * anchor;
        self.painter.galley(&mut self.fonts, &g, min, color);
        Rect::from_min_size(min, g.size)
    }

    /// A dotted divider.
    pub fn dots(&mut self) {
        let w = self.avail_w();
        let r = self.alloc(vec2(w, 10.0));
        let n = (w / 9.0) as i32;
        for i in 0..n {
            let x = r.min.x + 4.0 + i as f32 * 9.0;
            self.painter.circle(vec2(x, r.center().y), 1.6, Paint::fill(INK.alpha(0.22)));
        }
    }

    // --------------------------------------------------------------------------------------------
    // Buttons

    pub fn button(&mut self, label: impl Into<String>) -> Resp {
        self.btn(Button::new(label))
    }

    /// A toggle chip.
    pub fn chip(&mut self, label: impl Into<String>, selected: bool) -> Resp {
        self.btn(Button::new(label).small().selected(selected))
    }

    pub fn btn(&mut self, b: Button) -> Resp {
        let id = self.make_id(("btn", &b.label));
        let g = self.fonts.layout(&b.label, b.font, b.font_size, None);
        let pad = b.height * if b.height <= 28.0 { 0.4 } else { 0.5 };
        let w = if b.round { b.height } else { (g.size.x + pad * 2.0).max(b.min_w) };
        let slot = self.alloc(vec2(w, b.height + LIP));
        let body = Rect::from_min_size(slot.min, vec2(w, b.height));
        let resp = self.interact(id, slot, b.enabled);
        let base = match b.selected {
            Some(false) => CREAM,
            _ => b.color,
        };
        let fill = self.bubble(id, body, base, &resp, b.enabled);
        let mut tc = theme::on(fill);
        if !b.enabled {
            tc = tc.alpha(0.55);
        }
        let tpos = body.center() - g.size * 0.5 + vec2(0.0, 0.5);
        self.painter.galley(&mut self.fonts, &g, tpos, tc);
        self.painter.pop_transform();
        resp
    }

    /// Draws a squashy pill body for `resp` and leaves its transform pushed (callers draw the
    /// content, then pop). Returns the fill colour used.
    pub fn bubble(&mut self, id: Id, body: Rect, base: Color, resp: &Resp, enabled: bool) -> Color {
        self.bubble_r(id, body, body.height() * 0.5, base, resp, enabled)
    }

    /// [`Ui::bubble`] with a custom corner radius (tiles, cards).
    pub fn bubble_r(&mut self, id: Id, body: Rect, rad: f32, base: Color, resp: &Resp, enabled: bool) -> Color {
        let hs = self.spring(id, 0, if resp.hovered && enabled { 1.0 } else { 0.0 }, 0.0, SpringCfg::SNAPPY);
        let ps = self.spring(id, 1, if resp.down { 1.0 } else { 0.0 }, 0.0, SpringCfg::BOUNCY);
        if resp.clicked {
            self.kick(id, 2, 10.0);
        }
        let wob = self.spring(id, 2, 0.0, 0.0, SpringCfg::WOBBLY);
        let fill = if enabled { base.lighten(hs * 0.08) } else { base.desaturate(0.85).mix(CREAM, 0.35) };
        let press = ps.clamp(-0.6, 1.0);
        // the underside stays put; the body sinks into it when pressed
        self.painter.rect(body.translate(vec2(0.0, LIP)), rad, Paint::fill(fill.darken(0.3)).stroke(OUTLINE, INK));
        let sx = 1.0 + hs * 0.035 + ps * 0.05;
        let sy = 1.0 + hs * 0.035 - ps * 0.08;
        self.painter.push_transform(body.center(), vec2(sx, sy), wob * 0.025, vec2(0.0, press * LIP - hs * 1.0));
        self.painter.rect(body, rad, Paint::gradient(fill.lighten(0.2), fill).stroke(OUTLINE, INK));
        let gloss = Rect::new(body.min.x + rad.min(body.height() * 0.5) * 0.55, body.min.y + 3.5, (body.width() - rad.min(body.height() * 0.5) * 1.1).max(4.0), (body.height() * 0.22).min(10.0));
        self.painter.rect(gloss, gloss.height() * 0.5, Paint::fill(PAPER.alpha(if enabled { 0.4 } else { 0.2 })));
        fill
    }

    // --------------------------------------------------------------------------------------------
    // Bars, sliders, swatches

    /// A jelly progress bar (`frac` 0..1). Jumps in value make it wobble.
    pub fn bar(&mut self, src: impl Hash, frac: f32, color: Color, size: Vec2) -> Rect {
        let id = self.make_id(("bar", src));
        let frac = frac.clamp(0.0, 1.0);
        let r = self.alloc(size);
        let shown = self.spring(id, 0, frac, frac, SpringCfg::GENTLE);
        if let Some(last) = self.get_f(id, 1)
            && (frac - last).abs() > 0.03
        {
            self.kick(id, 2, 8.0);
        }
        self.set_f(id, 1, frac);
        let wob = self.spring(id, 2, 0.0, 0.0, SpringCfg::WOBBLY);
        self.paint_bar(r, shown, color, wob);
        r
    }

    pub fn paint_bar(&mut self, r: Rect, frac: f32, color: Color, wob: f32) {
        let rad = r.height() * 0.5;
        self.painter.push_transform(r.center(), vec2(1.0 - wob * 0.01, 1.0 + wob * 0.06), 0.0, Vec2::ZERO);
        self.painter.rect(r, rad, Paint::fill(INK.alpha(0.12)).stroke(2.0, INK));
        let inner = r.shrink(3.0);
        let w = (inner.width() * frac.clamp(0.0, 1.0)).max(if frac > 0.004 { inner.height() } else { 0.0 });
        if w > 0.0 {
            let f = Rect::from_min_size(inner.min, vec2(w, inner.height()));
            self.painter.rect(f, inner.height() * 0.5, Paint::gradient(color.lighten(0.3), color));
            let gl = Rect::new(f.min.x + 3.0, f.min.y + 1.5, (f.width() - 6.0).max(0.0), f.height() * 0.3);
            self.painter.rect(gl, gl.height() * 0.5, Paint::fill(PAPER.alpha(0.45)));
        }
        self.painter.pop_transform();
    }

    /// A labelled slider; returns true when the value changed.
    pub fn slider(&mut self, label: &str, v: &mut f32, lo: f32, hi: f32, step: f32, width: f32) -> bool {
        let id = self.make_id(("slider", label));
        let mut changed = false;
        self.row(26.0, |ui| {
            ui.text(Text::new(label).size(13.5));
            let lw = ui.fonts.layout(label, FontId::Body, 13.5, None).size.x;
            ui.space((90.0 - lw - 6.0).max(0.0));
            let r = ui.alloc(vec2(width, 24.0));
            let resp = ui.interact(id, r, true);
            let track = Rect::new(r.min.x + 9.0, r.center().y - 5.0, r.width() - 18.0, 10.0);
            if resp.down
                && let Some(p) = ui.input.pointer
            {
                let t = ((p.x - track.min.x) / track.width()).clamp(0.0, 1.0);
                let mut nv = lo + t * (hi - lo);
                if step > 0.0 {
                    nv = (nv / step).round() * step;
                }
                nv = nv.clamp(lo, hi);
                if (nv - *v).abs() > f32::EPSILON {
                    *v = nv;
                    changed = true;
                }
            }
            let t = ((*v - lo) / (hi - lo)).clamp(0.0, 1.0);
            let shown = ui.spring(id, 3, t, t, SpringCfg::SNAPPY);
            ui.painter.rect(track, 5.0, Paint::fill(INK.alpha(0.12)).stroke(2.0, INK));
            let filled = Rect::new(track.min.x, track.min.y, track.width() * shown, track.height());
            if filled.width() > 1.0 {
                ui.painter.rect(filled, 5.0, Paint::fill(TEAL).stroke(2.0, INK));
            }
            let grow = ui.spring(id, 0, if resp.down { 1.3 } else if resp.hovered { 1.15 } else { 1.0 }, 1.0, SpringCfg::BOUNCY);
            let c = vec2(track.min.x + track.width() * shown, track.center().y);
            ui.painter.circle(c + vec2(0.0, 2.5), 9.0 * grow, Paint::fill(INK.alpha(0.25)));
            ui.painter.circle(c, 9.0 * grow, Paint::gradient(PAPER, theme::CREAM_DEEP).stroke(OUTLINE, INK));
            let txt = if step >= 1.0 { format!("{}", v.round() as i64) } else { format!("{:.2}", v) };
            ui.text(Text::new(txt).bold().size(13.0).soft());
        });
        changed
    }

    pub fn slider_u8(&mut self, label: &str, v: &mut u8, lo: u8, hi: u8, width: f32) -> bool {
        let mut f = *v as f32;
        let changed = self.slider(label, &mut f, lo as f32, hi as f32, 1.0, width);
        *v = f.round() as u8;
        changed
    }

    /// A round colour swatch.
    pub fn swatch(&mut self, color: Color, selected: bool, size: f32) -> Resp {
        let id = self.make_id(("swatch", (color.0.map(|c| (c * 255.0) as u8))));
        let r = self.alloc(Vec2::splat(size));
        let resp = self.interact(id, r, true);
        if resp.clicked {
            self.kick(id, 0, 6.0);
        }
        let s = self.spring(id, 0, if selected { 1.15 } else if resp.hovered { 1.08 } else { 1.0 }, 1.0, SpringCfg::BOUNCY);
        let rad = size * 0.5 * s;
        if selected {
            self.painter.circle(r.center(), rad + 3.5, Paint::fill(PAPER).stroke(OUTLINE, INK));
        }
        self.painter.circle(r.center() + vec2(0.0, 2.0), rad, Paint::fill(INK.alpha(0.2)));
        self.painter.circle(r.center(), rad, Paint::gradient(color.lighten(0.15), color).stroke(OUTLINE, INK));
        resp
    }

    // --------------------------------------------------------------------------------------------
    // Text field

    /// A single-line text field. `resp.changed` is set when the text changes.
    pub fn text_field(&mut self, src: impl Hash, s: &mut String, hint: &str, width: f32) -> Resp {
        let id = self.make_id(("field", src));
        let r = self.alloc(vec2(width, 32.0));
        let mut resp = self.interact(id, r, true);
        let g = self.fonts.layout(s, FontId::Body, 15.0, None);
        let pad = 12.0;
        let scroll_key = 7;
        if resp.pressed {
            let was = self.focus == Some(id);
            self.focus = Some(id);
            self.focus_touched = true;
            self.caret = s.len();
            if was && let Some(p) = self.input.pointer {
                let x = p.x - r.min.x - pad + self.get_f(id, scroll_key).unwrap_or(0.0);
                self.caret = g.glyphs.iter().find(|gl| gl.pos.x + gl.advance * 0.5 > x).map(|gl| gl.byte).unwrap_or(s.len());
            }
            self.kick(id, 0, 5.0);
        }
        let focused = self.focus == Some(id);
        if focused {
            let mut c = self.caret.min(s.len());
            while !s.is_char_boundary(c) {
                c -= 1;
            }
            let typed = std::mem::take(&mut self.input.text);
            for ch in typed.chars() {
                s.insert(c, ch);
                c += ch.len_utf8();
                resp.changed = true;
            }
            for k in std::mem::take(&mut self.input.keys) {
                match k {
                    Key::Backspace => {
                        if let Some((i, _)) = s[..c].char_indices().next_back() {
                            s.replace_range(i..c, "");
                            c = i;
                            resp.changed = true;
                        }
                    }
                    Key::Delete => {
                        if let Some(ch) = s[c..].chars().next() {
                            s.replace_range(c..c + ch.len_utf8(), "");
                            resp.changed = true;
                        }
                    }
                    Key::Left => c = s[..c].char_indices().next_back().map(|(i, _)| i).unwrap_or(0),
                    Key::Right => c = s[c..].chars().next().map(|ch| c + ch.len_utf8()).unwrap_or(c),
                    Key::Home => c = 0,
                    Key::End => c = s.len(),
                    Key::Enter | Key::Escape | Key::Tab => self.focus = None,
                }
            }
            self.caret = c;
        }
        let g = if resp.changed { self.fonts.layout(s, FontId::Body, 15.0, None) } else { g };
        let wob = self.spring(id, 0, 0.0, 0.0, SpringCfg::WOBBLY);
        self.painter.push_transform(r.center(), vec2(1.0 + wob * 0.01, 1.0 - wob * 0.02), 0.0, Vec2::ZERO);
        let (bw, bc) = if focused { (3.0, TEAL) } else { (OUTLINE, INK) };
        self.painter.rect(r, 12.0, Paint::fill(PAPER).stroke(bw, bc));
        let inner = r.shrink2(vec2(pad, 0.0));
        let old = self.painter.clip;
        self.painter.clip = old.intersect(&inner.expand(1.0));
        let caret_x = g.caret_x(self.caret.min(s.len()));
        let mut scroll = self.get_f(id, scroll_key).unwrap_or(0.0);
        if focused {
            if caret_x - scroll > inner.width() - 4.0 {
                scroll = caret_x - inner.width() + 4.0;
            }
            if caret_x - scroll < 0.0 {
                scroll = caret_x;
            }
        } else {
            scroll = 0.0;
        }
        self.set_f(id, scroll_key, scroll);
        let ty = r.center().y - g.line_height * 0.5;
        if s.is_empty() {
            let hg = self.fonts.layout(hint, FontId::Body, 15.0, None);
            self.painter.galley(&mut self.fonts, &hg, vec2(inner.min.x, ty), INK_SOFT.alpha(0.7));
        } else {
            self.painter.galley(&mut self.fonts, &g, vec2(inner.min.x - scroll, ty), INK);
        }
        if focused && ((self.time * 1.8).fract() < 0.6 || self.instant) {
            let x = inner.min.x + caret_x - scroll;
            self.painter.line(vec2(x, r.min.y + 8.0), vec2(x, r.max.y - 8.0), 2.0, TEAL);
        }
        self.painter.clip = old;
        self.painter.pop_transform();
        resp
    }

    // --------------------------------------------------------------------------------------------
    // Dropdown

    /// A button showing `current` that opens a bouncy list; returns the picked index.
    pub fn dropdown(&mut self, src: impl Hash, current: &str, items: &[String], width: f32) -> Option<usize> {
        let id = self.make_id(("dropdown", src));
        let h = 30.0;
        let slot = self.alloc(vec2(width, h + LIP));
        let body = Rect::from_min_size(slot.min, vec2(width, h));
        let resp = self.interact(id, slot, !items.is_empty());
        if resp.clicked {
            self.popup = if self.popup == Some(id) { None } else { Some(id) };
        }
        let open = self.popup == Some(id);
        self.bubble(id, body, if open { theme::SUN } else { CREAM }, &resp, !items.is_empty());
        let old = self.painter.clip;
        self.painter.clip = old.intersect(&Rect { min: vec2(body.min.x + 10.0, body.min.y), max: vec2(body.max.x - 28.0, body.max.y) });
        self.paint_text(vec2(body.min.x + 14.0, body.center().y), vec2(0.0, 0.5), current, FontId::Bold, 14.0, INK);
        self.painter.clip = old;
        let spin = self.spring(id, 4, if open { std::f32::consts::PI } else { 0.0 }, 0.0, SpringCfg::BOUNCY);
        self.painter.triangle(vec2(body.max.x - 16.0, body.center().y), vec2(5.5, 4.0), spin + std::f32::consts::PI, 1.0, Paint::fill(INK));
        self.painter.pop_transform();
        if !open {
            return None;
        }
        self.popup_touched = true;
        let mut picked = None;
        let item_h = 28.0;
        let list_h = (items.len() as f32 * item_h + 16.0).min(320.0);
        let w = width.max(170.0);
        let below = body.max.y + LIP + 6.0;
        let pos = if below + list_h > self.screen.max.y - 8.0 { vec2(body.min.x, body.min.y - 6.0 - list_h) } else { vec2(body.min.x, below) };
        let pos = vec2(pos.x.min(self.screen.max.x - w - 8.0), pos.y.max(8.0));
        let pop = self.spring(id, 5, 1.0, 0.3, SpringCfg::BOUNCY);
        let (_, list_rect) = self.area(("dropdown-list", id), Order::Popup, pos, w, |ui| {
            ui.painter.push_transform(pos + vec2(w * 0.5, 0.0), vec2(1.0, pop), 0.0, Vec2::ZERO);
            ui.card(PAPER, 8.0, Some(w), |ui| {
                ui.scroll("items", list_h - 16.0, |ui| {
                    for (i, it) in items.iter().enumerate() {
                        let iid = ui.make_id(("item", i));
                        let r = ui.alloc(vec2(ui.avail_w(), item_h - 4.0));
                        let ir = ui.interact(iid, r, true);
                        let hs = ui.spring(iid, 0, if ir.hovered { 1.0 } else { 0.0 }, 0.0, SpringCfg::SNAPPY);
                        if hs > 0.01 {
                            ui.painter.rect(r, 10.0, Paint::fill(theme::SUN.alpha(hs)));
                        }
                        let selected = it == current;
                        ui.paint_text(vec2(r.min.x + 10.0 + hs * 4.0, r.center().y), vec2(0.0, 0.5), it, if selected { FontId::Bold } else { FontId::Body }, 14.0, INK);
                        if ir.clicked {
                            picked = Some(i);
                        }
                    }
                });
            });
            ui.painter.pop_transform();
        });
        let outside = self.input.pressed[0] && self.input.pointer.is_some_and(|p| !list_rect.contains(p) && !slot.contains(p));
        if picked.is_some() || outside || self.input.keys.contains(&Key::Escape) {
            self.popup = None;
        }
        picked
    }

    // --------------------------------------------------------------------------------------------
    // Containers

    /// A content-sized paper card at the cursor. `width` defaults to the available width.
    pub fn card<R>(&mut self, fill: Color, pad: f32, width: Option<f32>, f: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
        let order = self.painter.order as usize;
        let mark = self.painter.layers[order].len();
        let start = self.cursor();
        let w = width.unwrap_or_else(|| self.avail_w());
        let inner = Rect { min: start + Vec2::splat(pad), max: vec2(start.x + w - pad, start.y + 1e5) };
        let (r, used) = self.region(inner, 8.0, f);
        let rect = Rect { min: start, max: vec2(start.x + w, used.max.y.max(start.y + pad) + pad) };
        self.alloc(rect.size());
        let len = self.painter.layers[order].len();
        theme::card(&mut self.painter, rect, 20.0, fill);
        let bg: Vec<_> = self.painter.layers[order].drain(len..).collect();
        self.painter.layers[order].splice(mark..mark, bg);
        (r, rect)
    }

    /// A vertically scrolling region at most `max_h` tall (wheel to scroll; springy).
    pub fn scroll<R>(&mut self, src: impl Hash, max_h: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
        let id = self.make_id(("scroll", src));
        let start = self.cursor();
        let w = self.avail_w();
        let content_h = self.get_f(id, 0).unwrap_or(max_h);
        let view_h = content_h.min(max_h).max(1.0);
        let view = Rect::from_min_size(start, vec2(w, view_h));
        let overflow = (content_h - view_h).max(0.0);
        let mut target = self.get_f(id, 1).unwrap_or(0.0);
        if overflow > 0.0 && self.input.scroll.y != 0.0 && self.pointer_in(view) {
            target -= self.input.scroll.y * 48.0;
            self.input.scroll = Vec2::ZERO;
        }
        target = target.clamp(0.0, overflow);
        self.set_f(id, 1, target);
        let off = self.spring(id, 2, target, target, SpringCfg::SNAPPY);
        let bar_w = if overflow > 0.0 { 12.0 } else { 0.0 };
        let parent_clip = self.clip();
        let clip = parent_clip.intersect(&Rect { min: vec2(view.min.x - 8.0, view.min.y), max: vec2(view.max.x + 8.0, view.max.y) });
        let old_painter_clip = self.painter.clip;
        self.painter.clip = clip;
        let inner = Rect { min: start - vec2(0.0, off), max: vec2(start.x + w - bar_w, start.y + 1e5) };
        self.layouts.push(Layout { max: inner, cursor: inner.min, dir: Dir::Down, gap: 6.0, line_h: 0.0, row_h: 0.0, wrap: false, used: None, clip });
        let r = f(self);
        let l = self.layouts.pop().expect("layout stack");
        self.painter.clip = old_painter_clip;
        let used_h = l.used.map(|u| u.max.y - inner.min.y).unwrap_or(0.0);
        self.set_f(id, 0, used_h);
        if overflow > 0.0 {
            let track = Rect::new(view.max.x - 7.0, view.min.y + 2.0, 6.0, view_h - 4.0);
            self.painter.rect(track, 3.0, Paint::fill(INK.alpha(0.08)));
            let th = (view_h * view_h / content_h).max(24.0);
            let ty = track.min.y + (off / overflow).clamp(0.0, 1.0) * (track.height() - th);
            self.painter.rect(Rect::new(track.min.x, ty, 6.0, th), 3.0, Paint::fill(INK.alpha(0.45)));
        }
        self.alloc(view.size());
        r
    }

    /// A pop-in window centred on `center`. Returns false when the close button was clicked.
    pub fn window(&mut self, src: impl Hash, title: &str, center: Vec2, width: f32, closable: bool, f: impl FnOnce(&mut Ui)) -> bool {
        let id = self.make_id(("window", src));
        let h = self.get_f(id, 10).unwrap_or(300.0);
        let pos = vec2((center.x - width * 0.5).clamp(8.0, (self.screen.max.x - width - 8.0).max(8.0)), (center.y - h * 0.5).clamp(70.0, (self.screen.max.y - h - 16.0).max(70.0)));
        let pop = self.spring(id, 0, 1.0, 0.55, SpringCfg::BOUNCY);
        let mut keep = true;
        let (_, rect) = self.area(id, Order::Window, pos, width, |ui| {
            ui.painter.push_transform(pos + vec2(width * 0.5, h * 0.5), Vec2::splat(pop), (1.0 - pop) * 0.15, Vec2::ZERO);
            ui.card(CREAM, 16.0, Some(width), |ui| {
                ui.row(32.0, |ui| {
                    let tw = ui.avail_w() - if closable { 40.0 } else { 0.0 };
                    let g = ui.fonts.layout(title, FontId::Display, 21.0, Some(tw));
                    let r = ui.alloc(vec2(tw, g.size.y));
                    ui.painter.galley(&mut ui.fonts, &g, r.min, INK);
                    if closable && ui.btn(Button::new("✖").round().height(30.0).font_size(13.0).color(theme::CORAL)).clicked {
                        keep = false;
                    }
                });
                f(ui);
            });
            ui.painter.pop_transform();
        });
        self.set_f(id, 10, rect.height());
        keep
    }
}
