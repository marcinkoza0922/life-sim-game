//! Hearth UI: a small immediate-mode game UI built for bounce.
//!
//! Every widget keeps springs in per-id memory, so hovering lifts, pressing squashes, panels pop
//! in with overshoot and bars wobble when their value jumps. Shapes are SDF quads (round corners,
//! soft shadows, gradients, arcs) drawn in one instanced call; text comes from bundled OFL fonts
//! (Nunito, Fredoka, Noto Emoji for icons) rasterised into a glyph atlas; images registered in
//! [`Ui::images`] live in a second (RGBA) atlas, so they draw in the same call.
//!
//! Text fields copy and paste through [`Ui::clipboard`]; the host should install one backed by
//! the system clipboard (the default only shares text within the process).
//!
//! Frame flow: feed winit events to [`Ui::input`] (`Input::on_event`), call [`Ui::begin_frame`],
//! build the interface, call [`Ui::end_frame`], then [`Renderer::prepare`] + [`Renderer::paint`].
//! Between frames, [`Ui::wants_pointer`] / [`Ui::wants_keyboard`] tell the game which events
//! belong to the UI.

pub mod anim;
pub mod clipboard;
pub mod geom;
pub mod images;
pub mod input;
pub mod paint;
pub mod render;
pub mod text;
pub mod theme;
pub mod widgets;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub use anim::{Spring, SpringCfg};
pub use clipboard::{Clipboard, LocalClipboard};
pub use geom::{Color, Rect};
pub use glam::{Vec2, vec2};
pub use images::{ImageError, ImageId, Images};
pub use input::{Input, Key, Mods};
pub use paint::{Order, Paint, Painter};
pub use render::Renderer;
pub use text::{FontId, Fonts, Galley};
pub use widgets::{Button, Text};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Id(pub u64);

impl Id {
    pub fn new(src: impl Hash) -> Id {
        Id(0).with(src)
    }
    pub fn with(self, src: impl Hash) -> Id {
        let mut h = std::hash::DefaultHasher::new();
        self.0.hash(&mut h);
        src.hash(&mut h);
        Id(h.finish())
    }
}

/// Result of interacting with a widget this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Resp {
    pub id: Id,
    pub rect: Rect,
    /// Pointer is over it (even when disabled, so tooltips can explain why).
    pub hovered: bool,
    /// Primary button went down on it this frame.
    pub pressed: bool,
    /// Held down (it owns the pointer).
    pub down: bool,
    /// Pressed and released on it.
    pub clicked: bool,
    /// The widget's value changed.
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Down,
    Right,
}

#[derive(Clone, Debug)]
struct Layout {
    max: Rect,
    cursor: Vec2,
    dir: Dir,
    gap: f32,
    line_h: f32,
    row_h: f32,
    wrap: bool,
    used: Option<Rect>,
    clip: Rect,
}

pub struct Ui {
    pub input: Input,
    pub painter: Painter,
    pub fonts: Fonts,
    pub images: Images,
    pub clipboard: Box<dyn Clipboard>,
    /// Seconds since start, and since the last frame.
    pub time: f32,
    pub dt: f32,
    /// Screen in points.
    pub screen: Rect,
    /// Snap every spring to its target (deterministic screenshots).
    pub instant: bool,
    frame: u64,
    springs: HashMap<(Id, u32), (Spring, u64)>,
    floats: HashMap<(Id, u32), f32>,
    seen: HashMap<Id, u32>,
    id_stack: Vec<Id>,
    layouts: Vec<Layout>,
    areas: Vec<(Rect, Order)>,
    prev_areas: Vec<(Rect, Order)>,
    pub active: Option<Id>,
    pub focus: Option<Id>,
    focus_touched: bool,
    /// Caret byte index in the focused text field.
    pub caret: usize,
    /// The other end of the selection (equal to `caret` when nothing is selected).
    pub anchor: usize,
    hover: Option<Id>,
    hover_now: Option<Id>,
    hover_since: f32,
    tooltip: Option<String>,
    popup: Option<Id>,
    popup_touched: bool,
    out: Vec<paint::Instance>,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Ui {
        Ui {
            input: Input::default(),
            painter: Painter::default(),
            fonts: Fonts::new(),
            images: Images::default(),
            clipboard: Box::new(LocalClipboard::default()),
            time: 0.0,
            dt: 1.0 / 60.0,
            screen: Rect::new(0.0, 0.0, 800.0, 600.0),
            instant: false,
            frame: 0,
            springs: HashMap::new(),
            floats: HashMap::new(),
            seen: HashMap::new(),
            id_stack: Vec::new(),
            layouts: Vec::new(),
            areas: Vec::new(),
            prev_areas: Vec::new(),
            active: None,
            focus: None,
            focus_touched: false,
            caret: 0,
            anchor: 0,
            hover: None,
            hover_now: None,
            hover_since: 0.0,
            tooltip: None,
            popup: None,
            popup_touched: false,
            out: Vec::new(),
        }
    }

    // --------------------------------------------------------------------------------------------
    // Frame

    pub fn begin_frame(&mut self, dt: f32, size_px: [u32; 2]) {
        self.fonts.maybe_reset();
        self.painter.clear();
        self.painter.ppp = self.input.ppp;
        self.painter.order = Order::Panel;
        self.dt = dt;
        self.time += dt;
        self.frame += 1;
        self.screen = Rect::new(0.0, 0.0, size_px[0] as f32 / self.input.ppp, size_px[1] as f32 / self.input.ppp);
        self.id_stack.clear();
        self.seen.clear();
        self.layouts.clear();
        self.layouts.push(Layout { max: self.screen, cursor: Vec2::ZERO, dir: Dir::Down, gap: 0.0, line_h: 0.0, row_h: 0.0, wrap: false, used: None, clip: Rect::EVERYTHING });
        self.hover_now = None;
        self.tooltip = None;
        self.focus_touched = false;
        self.popup_touched = false;
    }

    pub fn end_frame(&mut self) {
        if let Some(text) = self.tooltip.take() {
            self.draw_tooltip(&text);
        }
        if self.hover_now != self.hover {
            self.hover = self.hover_now;
            self.hover_since = self.time;
        }
        if self.input.pressed[0] && !self.focus_touched {
            self.focus = None;
        }
        if !self.popup_touched {
            self.popup = None;
        }
        if self.input.released[0] || !self.input.down[0] {
            self.active = None;
        }
        self.prev_areas = std::mem::take(&mut self.areas);
        let frame = self.frame;
        self.springs.retain(|_, (_, f)| frame - *f < 30);
        self.input.end_frame();
        self.out.clear();
        for l in &self.painter.layers {
            self.out.extend_from_slice(l);
        }
    }

    pub(crate) fn instances(&self) -> &[paint::Instance] {
        &self.out
    }

    /// The pointer is over the UI (or dragging something in it): don't pass clicks to the world.
    pub fn wants_pointer(&self) -> bool {
        self.active.is_some() || self.popup.is_some() || self.input.pointer.is_some_and(|p| self.prev_areas.iter().any(|(r, o)| *o > Order::Background && r.contains(p)))
    }

    /// A text field has focus: don't treat key presses as shortcuts.
    pub fn wants_keyboard(&self) -> bool {
        self.focus.is_some()
    }

    // --------------------------------------------------------------------------------------------
    // Ids and memory

    /// A unique id for a widget from `src` within the current id scope (repeats get a counter).
    pub fn make_id(&mut self, src: impl Hash) -> Id {
        let base = self.id_stack.last().copied().unwrap_or_default().with(src);
        let n = self.seen.entry(base).or_insert(0);
        *n += 1;
        if *n > 1 { base.with(*n) } else { base }
    }

    pub fn push_id(&mut self, src: impl Hash) {
        let id = self.id_stack.last().copied().unwrap_or_default().with(src);
        self.id_stack.push(id);
    }

    pub fn pop_id(&mut self) {
        self.id_stack.pop();
    }

    pub fn with_id<R>(&mut self, src: impl Hash, f: impl FnOnce(&mut Ui) -> R) -> R {
        self.push_id(src);
        let r = f(self);
        self.pop_id();
        r
    }

    /// Steps the spring `(id, slot)` towards `target` and returns its value. A new spring starts at `init`.
    pub fn spring(&mut self, id: Id, slot: u32, target: f32, init: f32, cfg: SpringCfg) -> f32 {
        let (frame, dt, instant) = (self.frame, self.dt, self.instant);
        let e = self.springs.entry((id, slot)).or_insert((Spring::new(init), frame));
        if instant {
            e.0 = Spring::new(target);
        } else if e.1 != frame {
            e.0.step(target, dt, cfg);
        }
        e.1 = frame;
        e.0.x
    }

    /// Adds velocity to a spring (a wobble or jiggle).
    pub fn kick(&mut self, id: Id, slot: u32, v: f32) {
        if self.instant {
            return;
        }
        let frame = self.frame;
        self.springs.entry((id, slot)).or_insert((Spring::new(0.0), frame)).0.v += v;
    }

    pub fn get_f(&self, id: Id, slot: u32) -> Option<f32> {
        self.floats.get(&(id, slot)).copied()
    }

    pub fn set_f(&mut self, id: Id, slot: u32, v: f32) {
        self.floats.insert((id, slot), v);
    }

    // --------------------------------------------------------------------------------------------
    // Layout

    fn lay(&mut self) -> &mut Layout {
        self.layouts.last_mut().expect("layout stack")
    }

    /// Reserves `size` in the current layout and returns where it went.
    pub fn alloc(&mut self, size: Vec2) -> Rect {
        let l = self.lay();
        let (r, row) = match l.dir {
            Dir::Down => {
                let r = Rect::from_min_size(l.cursor, size);
                l.cursor.y += size.y + l.gap;
                (r, r)
            }
            Dir::Right => {
                if l.wrap && l.cursor.x > l.max.min.x && l.cursor.x + size.x > l.max.max.x {
                    l.cursor.x = l.max.min.x;
                    l.cursor.y += l.line_h + l.gap;
                    l.line_h = 0.0;
                }
                let y = if l.row_h > size.y { l.cursor.y + (l.row_h - size.y) * 0.5 } else { l.cursor.y };
                let r = Rect::from_min_size(vec2(l.cursor.x, y), size);
                let row = Rect::from_min_size(l.cursor, vec2(size.x, size.y.max(l.row_h)));
                l.cursor.x += size.x + l.gap;
                l.line_h = l.line_h.max(row.height());
                (r, row)
            }
        };
        l.used = Some(l.used.map_or(row, |u| u.union(&row)));
        r
    }

    pub fn space(&mut self, amount: f32) {
        let l = self.lay();
        match l.dir {
            Dir::Down => l.cursor.y += amount,
            Dir::Right => l.cursor.x += amount,
        }
    }

    /// Width left in the current line.
    pub fn avail_w(&self) -> f32 {
        let l = self.layouts.last().expect("layout stack");
        (l.max.max.x - l.cursor.x).max(0.0)
    }

    pub fn cursor(&self) -> Vec2 {
        self.layouts.last().expect("layout stack").cursor
    }

    pub fn clip(&self) -> Rect {
        self.layouts.last().expect("layout stack").clip
    }

    /// Runs `f` in a nested layout starting at the cursor, then reserves what it used.
    pub fn scope<R>(&mut self, dir: Dir, gap: f32, row_h: f32, wrap: bool, f: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
        let parent = self.layouts.last().expect("layout stack");
        let start = parent.cursor;
        let max = Rect { min: start, max: parent.max.max };
        let clip = parent.clip;
        self.layouts.push(Layout { max, cursor: start, dir, gap, line_h: 0.0, row_h, wrap, used: None, clip });
        let r = f(self);
        let l = self.layouts.pop().expect("layout stack");
        let used = l.used.unwrap_or(Rect::from_min_size(start, Vec2::ZERO));
        let saved = std::mem::replace(&mut self.lay().row_h, 0.0);
        let rect = self.alloc(used.max - start);
        self.lay().row_h = saved;
        (r, rect)
    }

    /// Lays children left to right, vertically centred in `h` (0 = top-aligned).
    pub fn row<R>(&mut self, h: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
        self.scope(Dir::Right, 6.0, h, false, f).0
    }

    /// Left to right, wrapping onto new lines.
    pub fn wrap<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> R {
        self.scope(Dir::Right, 6.0, 0.0, true, f).0
    }

    pub fn column<R>(&mut self, gap: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
        self.scope(Dir::Down, gap, 0.0, false, f).0
    }

    /// Runs `f` with a vertical layout inside `rect` (doesn't reserve space in the parent).
    pub fn region<R>(&mut self, rect: Rect, gap: f32, f: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
        let clip = self.clip();
        self.layouts.push(Layout { max: rect, cursor: rect.min, dir: Dir::Down, gap, line_h: 0.0, row_h: 0.0, wrap: false, used: None, clip });
        let r = f(self);
        let l = self.layouts.pop().expect("layout stack");
        (r, l.used.unwrap_or(Rect::from_min_size(rect.min, Vec2::ZERO)))
    }

    // --------------------------------------------------------------------------------------------
    // Areas

    /// A free-floating layer region at `pos`, `width` wide. Its content rect blocks the pointer
    /// from reaching the world (except on [`Order::Background`]). Returns the content rect.
    pub fn area<R>(&mut self, src: impl Hash, order: Order, pos: Vec2, width: f32, f: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
        let id = self.make_id(src);
        self.id_stack.push(id);
        let old_order = self.painter.order;
        self.painter.order = order;
        let old_clip = self.painter.clip;
        self.painter.clip = Rect::EVERYTHING;
        let rect = Rect { min: pos, max: vec2(pos.x + width, self.screen.max.y.max(pos.y)) };
        self.layouts.push(Layout { max: rect, cursor: pos, dir: Dir::Down, gap: 0.0, line_h: 0.0, row_h: 0.0, wrap: false, used: None, clip: Rect::EVERYTHING });
        let r = f(self);
        let l = self.layouts.pop().expect("layout stack");
        let used = l.used.unwrap_or(Rect::from_min_size(pos, Vec2::ZERO));
        if order > Order::Background {
            self.areas.push((used, order));
        }
        self.set_f(id, 0, used.width());
        self.set_f(id, 1, used.height());
        self.painter.order = old_order;
        self.painter.clip = old_clip;
        self.id_stack.pop();
        (r, used)
    }

    /// The size an area had last frame (for anchoring to the bottom or right).
    pub fn area_size(&self, src: impl Hash) -> Option<Vec2> {
        let id = self.id_stack.last().copied().unwrap_or_default().with(src);
        Some(vec2(self.get_f(id, 0)?, self.get_f(id, 1)?))
    }

    /// Marks a rectangle as UI so world clicks under it are ignored.
    pub fn block(&mut self, rect: Rect) {
        self.areas.push((rect, self.painter.order.max(Order::Panel)));
    }

    // --------------------------------------------------------------------------------------------
    // Interaction

    fn blocked(&self, p: Vec2, order: Order) -> bool {
        self.prev_areas.iter().any(|(r, o)| *o > order && r.contains(p))
    }

    pub fn pointer_in(&self, rect: Rect) -> bool {
        self.input.pointer.is_some_and(|p| rect.contains(p) && self.clip().contains(p) && !self.blocked(p, self.painter.order))
    }

    pub fn interact(&mut self, id: Id, rect: Rect, enabled: bool) -> Resp {
        let free = self.active.is_none() || self.active == Some(id);
        let inside = free && self.pointer_in(rect);
        let mut r = Resp { id, rect, hovered: inside, ..Default::default() };
        if inside {
            self.hover_now = Some(id);
        }
        if !enabled {
            return r;
        }
        if inside && self.input.pressed[0] {
            self.active = Some(id);
            r.pressed = true;
        }
        if self.active == Some(id) {
            r.down = self.input.down[0];
            if self.input.released[0] && inside {
                r.clicked = true;
            }
        }
        r
    }

    /// Shows `text` near the pointer once `resp` has been hovered for a moment.
    pub fn tooltip(&mut self, resp: &Resp, text: impl Into<String>) {
        if resp.hovered && self.hover == Some(resp.id) && (self.time - self.hover_since > 0.35 || self.instant) {
            let t = text.into();
            if !t.trim().is_empty() {
                self.tooltip = Some(t);
            }
        }
    }

    fn draw_tooltip(&mut self, text: &str) {
        let Some(p) = self.input.pointer else { return };
        let g = self.fonts.layout(text, FontId::Body, 13.0, Some(280.0));
        let size = g.size + vec2(20.0, 14.0);
        let mut at = p + vec2(16.0, 20.0);
        at.x = at.x.min(self.screen.max.x - size.x - 6.0);
        if at.y + size.y > self.screen.max.y - 6.0 {
            at.y = p.y - size.y - 10.0;
        }
        let r = Rect::from_min_size(at, size);
        let id = Id::new(("tooltip", self.hover));
        let pop = self.spring(id, 0, 1.0, 0.6, SpringCfg::BOUNCY);
        let old = self.painter.order;
        self.painter.order = Order::Tooltip;
        self.painter.clip = Rect::EVERYTHING;
        self.painter.push_transform(r.min, Vec2::splat(pop), 0.0, Vec2::ZERO);
        self.painter.rect(r.translate(vec2(0.0, 3.0)), 10.0, Paint::fill(theme::INK.alpha(0.25)));
        self.painter.rect(r, 10.0, Paint::fill(theme::INK));
        self.painter.galley(&mut self.fonts, &g, r.min + vec2(10.0, 7.0), theme::CREAM);
        self.painter.pop_transform();
        self.painter.order = old;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame<R: Default>(ui: &mut Ui, f: impl FnOnce(&mut Ui) -> R) -> R {
        ui.begin_frame(1.0 / 60.0, [800, 600]);
        let r = f(ui);
        ui.end_frame();
        r
    }

    fn press(ui: &mut Ui) {
        ui.input.down[0] = true;
        ui.input.pressed[0] = true;
    }

    fn release(ui: &mut Ui) {
        ui.input.down[0] = false;
        ui.input.released[0] = true;
    }

    fn panel_button(ui: &mut Ui) -> Resp {
        ui.area("panel", Order::Panel, vec2(10.0, 10.0), 200.0, |ui| ui.button("Hello")).0
    }

    #[test]
    fn buttons_click_and_claim_the_pointer() {
        let mut ui = Ui::new();
        ui.input.pointer = Some(vec2(30.0, 25.0));
        frame(&mut ui, panel_button);
        assert!(ui.wants_pointer(), "pointer over a panel belongs to the UI");
        press(&mut ui);
        let r = frame(&mut ui, panel_button);
        assert!(r.pressed && r.down && !r.clicked);
        release(&mut ui);
        let r = frame(&mut ui, panel_button);
        assert!(r.clicked);
        ui.input.pointer = Some(vec2(500.0, 500.0));
        frame(&mut ui, panel_button);
        assert!(!ui.wants_pointer(), "empty screen goes to the world");
    }

    #[test]
    fn releasing_elsewhere_is_not_a_click() {
        let mut ui = Ui::new();
        ui.input.pointer = Some(vec2(30.0, 25.0));
        frame(&mut ui, panel_button);
        press(&mut ui);
        frame(&mut ui, panel_button);
        ui.input.pointer = Some(vec2(500.0, 500.0));
        release(&mut ui);
        assert!(!frame(&mut ui, panel_button).clicked);
    }

    #[test]
    fn popups_block_widgets_underneath() {
        let mut ui = Ui::new();
        ui.input.pointer = Some(vec2(30.0, 25.0));
        let both = |ui: &mut Ui| {
            let r = panel_button(ui);
            ui.area("popup", Order::Popup, vec2(0.0, 0.0), 300.0, |ui| ui.alloc(vec2(300.0, 200.0)));
            r
        };
        frame(&mut ui, both);
        let r = frame(&mut ui, both);
        assert!(!r.hovered, "the popup above covers the button");
    }

    #[test]
    fn text_field_types_edits_and_unfocuses() {
        let mut ui = Ui::new();
        let mut s = String::from("ab");
        ui.input.pointer = Some(vec2(20.0, 20.0));
        let field = |ui: &mut Ui, s: &mut String| ui.area("f", Order::Panel, vec2(10.0, 10.0), 300.0, |ui| ui.text_field("name", s, "", 200.0)).0;
        press(&mut ui);
        frame(&mut ui, |ui| field(ui, &mut s));
        assert!(ui.wants_keyboard());
        release(&mut ui);
        assert_eq!(ui.caret, 0, "the caret goes where the field was clicked");
        ui.input.keys.push(Key::End);
        frame(&mut ui, |ui| field(ui, &mut s));
        ui.input.text.push_str("cd");
        ui.input.keys.extend([Key::Left, Key::Backspace]);
        let r = frame(&mut ui, |ui| field(ui, &mut s));
        assert!(r.changed);
        assert_eq!(s, "abd");
        ui.input.keys.push(Key::Enter);
        frame(&mut ui, |ui| field(ui, &mut s));
        assert!(!ui.wants_keyboard());
        // clicking away also drops focus
        press(&mut ui);
        frame(&mut ui, |ui| field(ui, &mut s));
        assert!(ui.wants_keyboard());
        ui.input.pointer = Some(vec2(700.0, 500.0));
        release(&mut ui);
        frame(&mut ui, |ui| field(ui, &mut s));
        press(&mut ui);
        frame(&mut ui, |ui| field(ui, &mut s));
        assert!(!ui.wants_keyboard());
    }

    fn focused_field(text: &str) -> (Ui, String) {
        let mut ui = Ui::new();
        let mut s = String::from(text);
        ui.input.pointer = Some(vec2(20.0, 20.0));
        press(&mut ui);
        frame(&mut ui, |ui| field(ui, &mut s));
        release(&mut ui);
        frame(&mut ui, |ui| field(ui, &mut s));
        (ui, s)
    }

    fn field(ui: &mut Ui, s: &mut String) -> Resp {
        ui.area("f", Order::Panel, vec2(10.0, 10.0), 300.0, |ui| ui.text_field("name", s, "", 200.0)).0
    }

    fn keys(ui: &mut Ui, s: &mut String, k: &[Key]) -> Resp {
        ui.input.keys.extend_from_slice(k);
        frame(ui, |ui| field(ui, s))
    }

    #[test]
    fn text_field_copies_cuts_and_pastes() {
        let (mut ui, mut s) = focused_field("hello world");
        keys(&mut ui, &mut s, &[Key::End]);
        ui.input.mods.shift = true;
        ui.input.mods.command = true;
        keys(&mut ui, &mut s, &[Key::Left]);
        ui.input.mods = Mods::default();
        assert_eq!((ui.anchor, ui.caret), (11, 6), "shift+ctrl+left selects the last word");
        keys(&mut ui, &mut s, &[Key::Copy]);
        assert_eq!(ui.clipboard.get().as_deref(), Some("world"));
        // typing replaces the selection
        ui.input.text.push_str("there");
        frame(&mut ui, |ui| field(ui, &mut s));
        assert_eq!(s, "hello there");
        // paste at the caret, newlines flattened
        ui.clipboard.set("big\nwide\n".into());
        keys(&mut ui, &mut s, &[Key::Paste]);
        assert_eq!(s, "hello therebig wide");
        keys(&mut ui, &mut s, &[Key::SelectAll, Key::Cut]);
        assert_eq!(s, "");
        assert_eq!(ui.clipboard.get().as_deref(), Some("hello therebig wide"));
        // nothing selected: copy keeps the clipboard as it was
        s.push_str("abc");
        keys(&mut ui, &mut s, &[Key::End, Key::Copy]);
        assert_eq!(ui.clipboard.get().as_deref(), Some("hello therebig wide"));
        // a plain arrow collapses a selection onto its edge
        keys(&mut ui, &mut s, &[Key::SelectAll, Key::Left]);
        assert_eq!((ui.anchor, ui.caret), (0, 0));
        ui.input.mods.command = true;
        let r = keys(&mut ui, &mut s, &[Key::End, Key::Backspace]);
        assert!(r.changed);
        assert_eq!(s, "", "ctrl+backspace deletes a word");
    }

    #[test]
    fn text_field_drag_and_double_click_select() {
        let (mut ui, mut s) = focused_field("one two three");
        ui.time += 1.0; // too late for a double-click
        // drag from the very left to past the end
        ui.input.pointer = Some(vec2(23.0, 20.0));
        press(&mut ui);
        frame(&mut ui, |ui| field(ui, &mut s));
        assert_eq!(ui.caret, 0);
        ui.input.pressed[0] = false;
        ui.input.pointer = Some(vec2(205.0, 20.0));
        frame(&mut ui, |ui| field(ui, &mut s));
        assert_eq!((ui.anchor, ui.caret), (0, s.len()));
        release(&mut ui);
        frame(&mut ui, |ui| field(ui, &mut s));
        ui.time += 1.0;
        // double-click in "two"
        let x = 22.0 + ui.fonts.layout("one tw", FontId::Body, 15.0, None).size.x;
        ui.input.pointer = Some(vec2(x, 20.0));
        for _ in 0..2 {
            press(&mut ui);
            frame(&mut ui, |ui| field(ui, &mut s));
            release(&mut ui);
            frame(&mut ui, |ui| field(ui, &mut s));
        }
        assert_eq!((ui.anchor, ui.caret), (4, 7));
    }

    #[test]
    fn image_buttons_size_to_icon_and_label() {
        let mut ui = Ui::new();
        let im = ui.images.add_rgba(2, 1, &[255; 8]).unwrap();
        let (plain, with_icon) = frame(&mut ui, |ui| {
            ui.area("a", Order::Panel, vec2(0.0, 0.0), 400.0, |ui| (ui.button("Go").rect, ui.btn(Button::new("Go").icon(im)).rect)).0
        });
        // a 2:1 icon fitted to 1.35 em of 15pt is 20.25 wide, plus a 6pt gap
        assert!((with_icon.width() - plain.width() - 26.25).abs() < 0.01, "{plain:?} {with_icon:?}");
        let n = ui.instances().iter().filter(|i| i.kind == paint::KIND_IMAGE).count();
        assert_eq!(n, 1);
    }

    #[test]
    fn instant_mode_snaps_springs() {
        let mut ui = Ui::new();
        ui.instant = true;
        let v = frame(&mut ui, |ui| ui.spring(Id::new("x"), 0, 5.0, 0.0, SpringCfg::BOUNCY));
        assert_eq!(v, 5.0);
    }
}
