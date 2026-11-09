//! The player-facing interface, built on [`hearth_ui`]: a floating toy-box HUD (household name and
//! funds, clock and speeds, modes and menu), the family's portrait bubbles, slide-in side panels
//! (peon details, Buy catalog, Build tools), a radial interaction menu, pop-in windows (saves,
//! neighborhood, career events), toasts and world-space bubbles over the peons.
//!
//! Developer windows stay on egui in [`crate::debug_ui`].

use std::f32::consts::PI;

use hearth_render::Camera;
use hearth_ui::theme::{self, CORAL, CREAM, GRASS, INK, INK_SOFT, LILAC, LIP, OUTLINE, PAPER, PEACH, PINK, SKY, SUN, TEAL};
use hearth_ui::{Button, Color, FontId, Id, Order, Paint, Painter, Rect, Resp, SpringCfg, Text, Ui, Vec2, vec2};
use sim::Entity;
use sim::career::PendingChoices;
use sim::components::*;
use sim::data::{Category, LifeStage, Skill};
use sim::economy::Household;
use sim::log::LogKind;
use sim::social::Relationships;
use sim::time::FreeWill;

use crate::icons::{Icon, IconSize, Icons};
use crate::scene::{Label, LabelKind, WallMode};
use crate::state::*;

pub struct UiCtx<'a> {
    pub camera: &'a Camera,
    pub labels: &'a [Label],
    /// Framebuffer size in pixels.
    pub screen: (f32, f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PeonTab {
    #[default]
    Needs,
    Mood,
    Personality,
    Work,
    People,
}

/// Interface-only state that isn't part of the game.
#[derive(Default)]
pub struct HudState {
    pub game_menu: bool,
    pub adopt_menu: bool,
    pub tab: PeonTab,
    pub buy_cat: Option<Category>,
    /// The peon the side panel showed last frame (a change makes it wobble).
    pub shown_peon: Option<Entity>,
    pub icons: Icons,
}

const PANEL_W: f32 = 350.0;
const PANEL_TOP: f32 = 116.0;

pub fn draw(ui: &mut Ui, g: &mut Game, cx: &UiCtx) {
    g.hud.icons.ensure(ui);
    labels(ui, cx);
    if g.creator.is_some() {
        creator_screen(ui, g);
        toasts(ui, g);
        return;
    }
    hud_left(ui, g);
    hud_clock(ui, g);
    hud_right(ui, g);
    household(ui, g);
    match g.mode {
        Mode::Live => {
            if let Some(sel) = g.selected {
                peon_panel(ui, g, sel);
            }
        }
        Mode::Buy => buy_panel(ui, g),
        Mode::Build => build_panel(ui, g),
    }
    if g.show_neighborhood {
        neighborhood_window(ui, g);
    }
    if g.show_saves {
        saves_window(ui, g);
    }
    pie_menu(ui, g);
    career_events(ui, g);
    toasts(ui, g);
}

// ------------------------------------------------------------------------------------------------
// Little helpers

fn rgb(c: [f32; 3]) -> Color {
    Color::rgb(c)
}

pub fn money(v: i64) -> String {
    let s = v.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{}${out}", if v < 0 { "-" } else { "" })
}

/// A static rounded tag; hover it for a tooltip.
fn tag(ui: &mut Ui, text: &str, bg: Color, size: f32) -> Resp {
    let g = ui.fonts.layout(text, FontId::Bold, size, None);
    let pad = vec2(9.0, 3.0);
    let r = ui.alloc(g.size + pad * 2.0);
    let id = ui.make_id(("tag", text));
    let resp = ui.interact(id, r, false);
    ui.painter.rect(r, r.height() * 0.5, Paint::gradient(bg.lighten(0.2), bg).stroke(2.0, INK));
    ui.painter.galley(&mut ui.fonts, &g, r.min + pad, theme::on(bg));
    resp
}

fn log_color(k: LogKind) -> Color {
    match k {
        LogKind::Funny => SUN,
        LogKind::Social => PINK,
        LogKind::Career => SKY,
        LogKind::Life | LogKind::Money => GRASS,
        LogKind::Death => INK_SOFT,
        LogKind::Warning => CORAL,
        _ => LILAC,
    }
}

/// A round button with a hand-drawn icon.
fn icon_button(ui: &mut Ui, key: &str, size: f32, color: Color, selected: bool, icon: impl FnOnce(&mut Painter, Vec2, Color)) -> Resp {
    let id = ui.make_id(("icon", key));
    let slot = ui.alloc(vec2(size, size + LIP));
    let body = Rect::from_min_size(slot.min, Vec2::splat(size));
    let resp = ui.interact(id, slot, true);
    let fill = ui.bubble(id, body, if selected { color } else { CREAM }, &resp, true);
    icon(&mut ui.painter, body.center(), theme::on(fill));
    ui.painter.pop_transform();
    resp
}

fn pause_icon(p: &mut Painter, c: Vec2, col: Color) {
    for dx in [-4.5, 4.5] {
        p.rect(Rect::from_center_size(c + vec2(dx, 0.0), vec2(5.0, 14.0)), 2.0, Paint::fill(col));
    }
}

fn play_icon(n: usize) -> impl FnOnce(&mut Painter, Vec2, Color) {
    move |p: &mut Painter, c: Vec2, col: Color| {
        let w = 9.0;
        let x0 = c.x - (n as f32 - 1.0) * w * 0.5;
        for i in 0..n {
            p.triangle(vec2(x0 + i as f32 * w, c.y), vec2(6.0, 6.5), PI * 0.5, 1.0, Paint::fill(col));
        }
    }
}

/// A small card menu at `pos`. Returns false when the player clicked outside it (and `anchor`).
fn popup_card(ui: &mut Ui, key: &str, pos: Vec2, width: f32, anchor: Rect, f: impl FnOnce(&mut Ui)) -> bool {
    let id = Id::new(("popup", key));
    let pop = ui.spring(id, 0, 1.0, 0.4, SpringCfg::BOUNCY);
    let (_, rect) = ui.area(("popup", key), Order::Popup, pos, width, |ui| {
        ui.painter.push_transform(pos + vec2(width * 0.5, 0.0), Vec2::splat(pop), 0.0, Vec2::ZERO);
        ui.card(PAPER, 10.0, Some(width), f);
        ui.painter.pop_transform();
    });
    !(ui.input.pressed[0] && ui.input.pointer.is_some_and(|p| !rect.contains(p) && !anchor.contains(p)))
}

/// A cartoon face in a shirt-coloured bubble: hair, eyes, cheeks and a mouth that follows the mood.
#[allow(clippy::too_many_arguments)]
pub fn portrait(p: &mut Painter, c: Vec2, r: f32, skin: [f32; 3], hair: [f32; 3], shirt: [f32; 3], mood: f32, dead: bool) {
    let shirt = if dead { Color::rgb(shirt).desaturate(0.9) } else { rgb(shirt) };
    p.circle(c + vec2(0.0, 3.0), r, Paint::fill(INK.alpha(0.25)));
    p.circle(c, r, Paint::gradient(shirt.lighten(0.3), shirt).stroke(OUTLINE, INK));
    let hc = c + vec2(0.0, r * 0.08);
    let hr = r * 0.7;
    p.circle(hc - vec2(0.0, hr * 0.1), hr * 0.98, Paint::fill(rgb(hair)).stroke(2.0, INK));
    let skin = if dead { Color::rgb(skin).desaturate(0.9).lighten(0.2) } else { rgb(skin) };
    p.circle(hc + vec2(0.0, hr * 0.12), hr * 0.8, Paint::fill(skin).stroke(2.0, INK));
    // fringe
    p.arc_at(hc + vec2(0.0, hr * 0.12), hr * 0.7, hr * 0.26, 0.3, 0.0, rgb(hair));
    let ey = hc.y + hr * 0.24;
    for dx in [-hr * 0.34, hr * 0.34] {
        if dead {
            p.line(vec2(hc.x + dx - 3.0, ey), vec2(hc.x + dx + 3.0, ey), 2.0, INK);
        } else {
            p.circle(vec2(hc.x + dx, ey), (hr * 0.1).max(1.6), Paint::fill(INK));
            p.circle(vec2(hc.x + dx * 1.45, ey + hr * 0.25), hr * 0.13, Paint::fill(PINK.alpha(0.55)));
        }
    }
    let mc = vec2(hc.x, hc.y + hr * 0.5);
    if dead || (-25.0..=25.0).contains(&mood) {
        p.line(mc - vec2(hr * 0.18, 0.0), mc + vec2(hr * 0.18, 0.0), 2.0, INK);
    } else if mood > 25.0 {
        p.arc_at(mc - vec2(0.0, hr * 0.18), hr * 0.26, 2.0, 0.3, PI, INK);
    } else {
        p.arc_at(mc + vec2(0.0, hr * 0.2), hr * 0.24, 2.0, 0.28, 0.0, INK);
    }
}

/// The right-hand card shared by the peon, Buy and Build panels: slides in with a bounce.
fn side_panel(ui: &mut Ui, key: &str, wobble: f32, f: impl FnOnce(&mut Ui)) {
    let id = Id::new(("side-panel", key));
    let slide = ui.spring(id, 0, 0.0, 1.0, SpringCfg::BOUNCY);
    let x = ui.screen.max.x - PANEL_W - 16.0 + slide * (PANEL_W + 40.0);
    ui.area(("side", key), Order::Panel, vec2(x, PANEL_TOP), PANEL_W, |ui| {
        ui.painter.push_transform(vec2(x + PANEL_W * 0.5, PANEL_TOP), vec2(1.0 + wobble * 0.012, 1.0 - wobble * 0.008), wobble * 0.012, Vec2::ZERO);
        ui.card(CREAM, 14.0, Some(PANEL_W), f);
        ui.painter.pop_transform();
    });
}

/// Height left for a scroll area that should end at the bottom of the screen.
fn rest_h(ui: &Ui) -> f32 {
    (ui.screen.max.y - 34.0 - ui.cursor().y).max(80.0)
}

// ------------------------------------------------------------------------------------------------
// HUD

fn hud_left(ui: &mut Ui, g: &mut Game) {
    let hh = g.sim.world.res::<Household>().clone();
    let here = {
        let hood = g.sim.world.res::<sim::neighborhood::Neighborhood>();
        hood.lots[hood.active].title()
    };
    let home = sim::neighborhood::at_home(&g.sim.world);
    let trip = g.sim.trip();
    let at = if let Some(t) = trip.as_ref().filter(|t| !t.arriving) {
        let to = g.sim.world.res::<sim::neighborhood::Neighborhood>().lots[t.to].title();
        format!("{here}, walking over to {to}")
    } else if trip.is_some() {
        format!("arriving at {here}")
    } else if home {
        format!("at home · {here}")
    } else if sim::neighborhood::visiting(&g.sim.world) {
        format!("visiting {here}")
    } else {
        format!("out at {here}")
    };
    let mut go_home = false;
    ui.area("hud-left", Order::Panel, vec2(16.0, 14.0), 640.0, |ui| {
        ui.row(0.0, |ui| {
            let nw = ui.fonts.layout(&hh.name, FontId::Display, 21.0, None).size.x.max(ui.fonts.layout(&at, FontId::Body, 12.5, None).size.x);
            ui.card(CREAM, 10.0, Some(nw + 30.0), |ui| {
                ui.text(Text::new(hh.name.clone()).display().size(21.0));
                ui.text(Text::new(at.clone()).size(12.5).soft());
            });
            ui.space(4.0);
            ui.column(4.0, |ui| {
                ui.space(5.0);
                funds_badge(ui, hh.funds);
            });
            if !home && trip.is_none() {
                ui.column(0.0, |ui| {
                    ui.space(8.0);
                    go_home = ui.btn(Button::new("🏠 Go home").color(SKY)).clicked;
                });
            }
        });
    });
    if go_home {
        match g.sim.go_home() {
            Ok(()) => g.check_lot_change(),
            Err(e) => g.say(e),
        }
    }
}

/// Funds in a coin pill that counts up/down and wobbles when money changes hands.
fn funds_badge(ui: &mut Ui, funds: i64) {
    let id = ui.make_id("funds");
    let shown = ui.spring(id, 0, funds as f32, funds as f32, SpringCfg::GENTLE);
    if let Some(last) = ui.get_f(id, 1)
        && last as i64 != funds
    {
        ui.kick(id, 2, 14.0);
    }
    ui.set_f(id, 1, funds as f32);
    let wob = ui.spring(id, 2, 0.0, 0.0, SpringCfg::WOBBLY);
    let txt = money(shown.round() as i64);
    let gal = ui.fonts.layout(&txt, FontId::Display, 22.0, None);
    let h = 42.0;
    let slot = ui.alloc(vec2(gal.size.x + 62.0, h + LIP));
    let body = Rect::from_min_size(slot.min, vec2(slot.width(), h));
    let fill = if funds < 0 { CORAL } else { GRASS };
    ui.painter.rect(body.translate(vec2(0.0, LIP)), h * 0.5, Paint::fill(fill.darken(0.3)).stroke(OUTLINE, INK));
    ui.painter.push_transform(body.center(), vec2(1.0 + wob * 0.03, 1.0 - wob * 0.03), wob * 0.015, Vec2::ZERO);
    ui.painter.rect(body, h * 0.5, Paint::gradient(fill.lighten(0.25), fill).stroke(OUTLINE, INK));
    let coin = vec2(body.min.x + h * 0.5, body.center().y);
    ui.painter.push_transform(coin, vec2(0.65 + 0.35 * (ui.time * 1.7).cos().abs(), 1.0), 0.0, Vec2::ZERO);
    ui.painter.circle(coin, 13.0, Paint::gradient(SUN.lighten(0.3), SUN).stroke(2.0, INK));
    ui.paint_text(coin, Vec2::splat(0.5), "$", FontId::Display, 15.0, INK);
    ui.painter.pop_transform();
    ui.painter.galley(&mut ui.fonts, &gal, vec2(body.min.x + h - 2.0, body.center().y - gal.size.y * 0.5), INK);
    ui.painter.pop_transform();
}

fn hud_clock(ui: &mut Ui, g: &mut Game) {
    let clock = g.sim.clock();
    let m = clock.minute_of_day();
    let time = format!("{:02}:{:02}", m / 60, m % 60);
    let day = format!("Day {} · {}", clock.day() + 1, clock.weekday_name());
    let w = 372.0;
    let x = ui.screen.center().x - w * 0.5;
    let mut pick: Option<Option<usize>> = None;
    let (_, rect) = ui.area("hud-clock", Order::Panel, vec2(x, 14.0), w, |ui| {
        ui.card(CREAM, 10.0, Some(w), |ui| {
            ui.row(42.0, |ui| {
                ui.column(0.0, |ui| {
                    ui.text(Text::new(time).display().size(24.0));
                    ui.text(Text::new(day).size(12.0).soft());
                });
                ui.space((ui.avail_w() - 5.0 * 38.0 - 5.0 * 6.0).max(0.0));
                let r = icon_button(ui, "pause", 38.0, CORAL, g.paused, pause_icon);
                ui.tooltip(&r, "Pause (Space)");
                if r.clicked {
                    pick = Some(None);
                }
                for (i, (_, name)) in SPEEDS.iter().enumerate() {
                    let on = !g.paused && g.speed == i;
                    let r = if i == 3 {
                        ui.btn(Button::new("⚡").round().height(38.0).font_size(16.0).color(SUN).selected(on))
                    } else {
                        icon_button(ui, name, 38.0, TEAL, on, play_icon(i + 1))
                    };
                    ui.tooltip(&r, format!("Speed {name} ({})", i + 1));
                    if r.clicked {
                        pick = Some(Some(i));
                    }
                }
            });
        });
    });
    match pick {
        Some(None) => g.paused = !g.paused,
        Some(Some(i)) => {
            g.speed = i;
            g.paused = false;
        }
        None => {}
    }
    // the status line pops out of the clock like a speech bubble
    if let Some((msg, t)) = g.status.clone() {
        let age = g.time - t;
        let id = Id::new(("status", t.to_bits()));
        let s = ui.spring(id, 0, if age < 4.0 { 1.0 } else { 0.0 }, 0.0, SpringCfg::BOUNCY);
        if s > 0.01 {
            let gal = ui.fonts.layout(&msg, FontId::Bold, 14.0, Some(420.0));
            let size = gal.size + vec2(28.0, 16.0);
            let c = vec2(rect.center().x, rect.max.y + 22.0 + size.y * 0.5);
            let r = Rect::from_center_size(c, size);
            ui.painter.order = Order::Popup;
            ui.painter.push_transform(vec2(c.x, r.min.y), Vec2::splat(s.max(0.0)), 0.0, Vec2::ZERO);
            ui.painter.triangle(vec2(c.x, r.min.y - 5.0), vec2(8.0, 7.0), 0.0, 1.0, Paint::fill(SUN).stroke(OUTLINE, INK));
            ui.painter.rect(r.translate(vec2(0.0, 4.0)), 14.0, Paint::fill(INK.alpha(0.22)));
            ui.painter.rect(r, 14.0, Paint::gradient(SUN.lighten(0.35), SUN).stroke(OUTLINE, INK));
            ui.painter.triangle(vec2(c.x, r.min.y - 2.0), vec2(6.0, 5.0), 0.0, 1.0, Paint::fill(SUN.lighten(0.33)));
            ui.painter.galley(&mut ui.fonts, &gal, r.min + vec2(14.0, 8.0), INK);
            ui.painter.pop_transform();
            ui.painter.order = Order::Panel;
        }
    }
}

fn hud_right(ui: &mut Ui, g: &mut Game) {
    let size = ui.area_size("hud-right").unwrap_or(vec2(380.0, 90.0));
    let x = ui.screen.max.x - 16.0 - size.x;
    let mut menu_btn = Rect::default();
    let mut new_mode = None;
    let mut level = None;
    let mut cycle_walls = false;
    ui.area("hud-right", Order::Panel, vec2(x, 14.0), 460.0, |ui| {
        ui.row(0.0, |ui| {
            for (m, name, color, key) in [(Mode::Live, "Live", TEAL, "F5"), (Mode::Buy, "Buy", SUN, "F6"), (Mode::Build, "Build", CORAL, "F7")] {
                let allowed = m == Mode::Live || g.can_build();
                let r = ui.btn(Button::new(name).height(40.0).font_size(17.0).color(color).selected(g.mode == m).enabled(allowed).min_w(76.0));
                ui.tooltip(&r, if allowed { format!("{name} mode ({key})") } else { "Only on your own lot".into() });
                if r.clicked {
                    new_mode = Some(m);
                }
            }
            ui.space(4.0);
            let r = ui.btn(Button::new("📋").round().height(40.0).font_size(18.0).color(LILAC).selected(g.hud.game_menu));
            ui.tooltip(&r, "Game menu");
            menu_btn = r.rect;
            if r.clicked {
                g.hud.game_menu = !g.hud.game_menu;
            }
        });
        ui.space(6.0);
        ui.row(30.0, |ui| {
            let r = ui.btn(Button::new("⬇").round().small().color(SKY));
            ui.tooltip(&r, "Level down (PgDn)");
            if r.clicked {
                level = Some(g.level - 1);
            }
            let name = match g.level {
                l if l < 0 => format!("Basement {}", -l),
                0 => "Ground".into(),
                l => format!("Floor {}", l + 1),
            };
            tag(ui, &name, PAPER, 13.0);
            let r = ui.btn(Button::new("⬆").round().small().color(SKY));
            ui.tooltip(&r, "Level up (PgUp)");
            if r.clicked {
                level = Some(g.level + 1);
            }
            let label = match g.wall_mode {
                WallMode::Up => "Walls up",
                WallMode::Cutaway => "Cutaway",
                WallMode::Down => "Walls down",
            };
            let r = ui.btn(Button::new(label).small().color(PEACH));
            ui.tooltip(&r, "Cycle wall mode (V)");
            cycle_walls = r.clicked;
            let r = ui.chip("Roofs", g.show_roofs);
            ui.tooltip(&r, "Show roofs (T)");
            if r.clicked {
                g.show_roofs = !g.show_roofs;
            }
        });
    });
    if let Some(m) = new_mode {
        g.mode = m;
        g.tool = Tool::None;
        g.menu = None;
    }
    if let Some(l) = level {
        g.set_level(l);
    }
    if cycle_walls {
        g.wall_mode = match g.wall_mode {
            WallMode::Up => WallMode::Cutaway,
            WallMode::Cutaway => WallMode::Down,
            WallMode::Down => WallMode::Up,
        };
    }
    if g.mode != Mode::Live {
        let r = Rect::new(ui.screen.max.x - 16.0 - 220.0, ui.screen.max.y - 40.0, 220.0, 24.0);
        let (_, _) = ui.region(r, 0.0, |ui| tag(ui, "⌛ time is paused while you build", CREAM, 12.0));
    }
    if g.hud.game_menu {
        game_menu(ui, g, menu_btn);
    }
}

fn game_menu(ui: &mut Ui, g: &mut Game, anchor: Rect) {
    let w = 220.0;
    let pos = vec2(anchor.max.x - w, anchor.max.y + 10.0);
    let mut act: Option<&str> = None;
    let has_save = std::path::Path::new(QUICKSAVE).exists();
    let mut roofs = g.show_roofs;
    let mut mute = g.mute;
    let mut vol = g.volumes;
    let keep = popup_card(ui, "game-menu", pos, w, anchor, |ui| {
        for (key, label, color, enabled) in [
            ("save", "💾 Quick save", TEAL, true),
            ("load", "📂 Quick load", TEAL, has_save),
            ("saves", "📚 Saves…", SKY, true),
            ("new", "👪 New household…", GRASS, true),
            ("hood", "🏡 Neighborhood…", PEACH, true),
        ] {
            if ui.btn(Button::new(label).color(color).enabled(enabled).min_w(w - 20.0)).clicked {
                act = Some(key);
            }
        }
        ui.dots();
        ui.row(0.0, |ui| {
            if ui.chip("Roofs (T)", roofs).clicked {
                roofs = !roofs;
            }
            if ui.chip("Mute (M)", mute).clicked {
                mute = !mute;
            }
        });
        for (label, v) in [("Volume", &mut vol.master), ("Voices", &mut vol.voices), ("Effects", &mut vol.sfx), ("Music", &mut vol.music)] {
            ui.slider(label, v, 0.0, 1.0, 0.05, w - 20.0);
        }
    });
    g.volumes = vol;
    g.show_roofs = roofs;
    g.mute = mute;
    match act {
        Some("save") => g.save_to(std::path::Path::new(QUICKSAVE)),
        Some("load") => g.load_from(std::path::Path::new(QUICKSAVE)),
        Some("saves") => g.show_saves = true,
        Some("new") => g.open_creator(),
        Some("hood") => g.show_neighborhood = true,
        _ => {}
    }
    if act.is_some() || !keep {
        g.hud.game_menu = false;
    }
}

// ------------------------------------------------------------------------------------------------
// Household column

fn household(ui: &mut Ui, g: &mut Game) {
    let members: Vec<Entity> = g.sim.peons().into_iter().filter(|e| g.sim.world.has::<Member>(*e)).collect();
    let mut adopt_btn = Rect::default();
    ui.area("household", Order::Panel, vec2(12.0, PANEL_TOP), 96.0, |ui| {
        ui.column(2.0, |ui| {
            for e in members {
                member_bubble(ui, g, e);
            }
            ui.row(0.0, |ui| {
                ui.space(25.0);
                let r = ui.btn(Button::new("➕").round().height(42.0).font_size(18.0).color(GRASS).selected(g.hud.adopt_menu));
                ui.tooltip(&r, "Adopt a child");
                adopt_btn = r.rect;
                if r.clicked {
                    g.hud.adopt_menu = !g.hud.adopt_menu;
                }
            });
        });
    });
    if g.hud.adopt_menu {
        let mut pick = None;
        let keep = popup_card(ui, "adopt", vec2(adopt_btn.max.x + 12.0, adopt_btn.min.y - 40.0), 200.0, adopt_btn, |ui| {
            ui.text(Text::new("Adopt").display().size(18.0));
            ui.small(format!("Agency fee {}", money(sim::family::ADOPTION_FEE)));
            for s in [LifeStage::Infant, LifeStage::Toddler, LifeStage::Child, LifeStage::Teen] {
                if ui.btn(Button::new(s.label()).small().color(PINK).min_w(180.0)).clicked {
                    pick = Some(s);
                }
            }
        });
        if let Some(s) = pick {
            match g.sim.adopt(s) {
                Ok(kid) => {
                    let name = g.sim.name(kid);
                    g.say(format!("{name} joined the family!"));
                }
                Err(e) => g.say(e),
            }
        }
        if pick.is_some() || !keep {
            g.hud.adopt_menu = false;
        }
    }
}

fn member_bubble(ui: &mut Ui, g: &mut Game, e: Entity) {
    let w = &g.sim.world;
    let Some(p) = w.get::<Peon>(e).map(|p| p.clone()) else { return };
    let dead = w.has::<Dead>(e);
    let away = w.get::<AtWork>(e).map(|a| a.school);
    let pregnant = w.has::<Pregnant>(e);
    let stage = w.get::<Age>(e).map(|a| a.stage).unwrap_or(LifeStage::Adult);
    let mood = w.get::<Mood>(e).map(|m| m.value).unwrap_or(0.0);
    let look = w.get::<Appearance>(e).map(|a| (a.skin, a.hair, a.shirt)).unwrap_or(([0.9, 0.75, 0.6], [0.3, 0.2, 0.1], [0.5, 0.6, 0.8]));
    let id = ui.make_id(("member", e));
    let r = ui.alloc(vec2(92.0, 96.0));
    let resp = ui.interact(id, r, !dead);
    let selected = g.selected == Some(e);
    if resp.clicked {
        ui.kick(id, 1, 16.0);
        if selected {
            g.focus_selected();
        }
        g.selected = Some(e);
        g.menu = None;
    }
    let s = ui.spring(id, 0, if selected { 1.12 } else if resp.hovered && !dead { 1.06 } else { 1.0 }, 1.0, SpringCfg::BOUNCY);
    let wob = ui.spring(id, 1, 0.0, 0.0, SpringCfg::WOBBLY);
    let c = vec2(r.center().x, r.min.y + 38.0);
    ui.painter.push_transform(c, Vec2::splat(s), wob * 0.03, Vec2::ZERO);
    if selected {
        let pulse = 1.0 + (ui.time * 3.0).sin() * 0.03;
        ui.painter.circle(c, 40.0 * pulse, Paint::fill(SUN.alpha(0.55)).stroke(2.0, INK.alpha(0.5)));
    }
    let frac = ((mood + 100.0) / 200.0).clamp(0.0, 1.0);
    if !dead {
        ui.painter.arc(c, 34.0, 6.0, 1.0, INK.alpha(0.15));
        let shown = ui.spring(id, 2, frac, frac, SpringCfg::GENTLE);
        ui.painter.arc(c, 34.0, 6.0, shown, theme::level_color(shown));
    }
    portrait(&mut ui.painter, c, 28.0, look.0, look.1, look.2, mood, dead);
    let badge = if dead {
        Some("👻")
    } else if let Some(school) = away {
        Some(if school { "🎒" } else { "💼" })
    } else if pregnant {
        Some("🍼")
    } else {
        None
    };
    if let Some(b) = badge {
        let bc = c + vec2(22.0, 20.0);
        ui.painter.circle(bc, 11.0, Paint::fill(PAPER).stroke(2.0, INK));
        ui.paint_text(bc, Vec2::splat(0.5), b, FontId::Body, 12.0, INK);
    }
    ui.paint_text(c + vec2(0.0, 40.0), vec2(0.5, 0.0), &p.first, FontId::Bold, 13.5, INK);
    ui.painter.pop_transform();
    let status = if dead {
        "passed on".to_string()
    } else {
        match away {
            Some(true) => format!("{} · at school", stage.label()),
            Some(false) => format!("{} · at work", stage.label()),
            None => format!("{} · mood {:+.0}", stage.label(), mood),
        }
    };
    ui.tooltip(&resp, format!("{}\n{status}", p.full_name()));
}

// ------------------------------------------------------------------------------------------------
// Peon panel

enum PeonAct {
    Cancel(u64),
    FreeWill(Option<FreeWill>),
    Quit,
    Join(String),
}

fn peon_panel(ui: &mut Ui, g: &mut Game, e: Entity) {
    let data = g.sim.data();
    let w = &g.sim.world;
    let Some(p) = w.get::<Peon>(e).map(|p| p.clone()) else { return };
    let Some(age) = w.get::<Age>(e).map(|a| a.clone()) else { return };
    let needs = w.get::<Needs>(e).map(|n| n.clone()).unwrap_or_default();
    let mood = w.get::<Mood>(e).map(|m| m.clone()).unwrap_or_default();
    let pers = w.get::<Personality>(e).map(|p| p.clone()).unwrap_or_default();
    let skills = w.get::<Skills>(e).map(|s| s.clone()).unwrap_or_default();
    let job = w.get::<Job>(e).map(|j| j.clone());
    let queue = w.get::<ActionQueue>(e).map(|q| q.clone()).unwrap_or_default();
    let fw = w.get::<Brain>(e).and_then(|b| b.free_will);
    let look = w.get::<Appearance>(e).map(|a| a.clone());
    let at_work = w.get::<AtWork>(e).map(|a| a.school);
    let student = w.get::<Student>(e).map(|s| (s.letter(), s.homework_done));
    let pregnant = w.get::<Pregnant>(e).map(|pg| {
        let other = w.get::<Peon>(pg.other).map(|p| p.first.clone()).unwrap_or_else(|| "someone".into());
        (other, pg.due.saturating_sub(g.sim.clock().tick) as f32 / 60.0)
    });
    let genes = w.get::<Genes>(e).map(|g| g.clone());
    let family = w.get::<Family>(e).map(|f| {
        let names = |v: &[Entity]| v.iter().filter_map(|x| w.get::<Peon>(*x).map(|p| p.first.clone())).collect::<Vec<_>>().join(", ");
        (names(&f.parents), names(&f.children))
    });
    let rels: Vec<(String, &'static str, f32, f32)> = w.res::<Relationships>().of(e).into_iter().filter_map(|(o, r)| Some((w.get::<Peon>(o)?.first.clone(), r.label(), r.friendship, r.romance))).collect();
    let now = g.sim.clock().tick;

    let wid = Id::new("peon-panel");
    if g.hud.shown_peon != Some(e) {
        if g.hud.shown_peon.is_some() {
            ui.kick(wid, 0, 14.0);
        }
        g.hud.shown_peon = Some(e);
    }
    let wob = ui.spring(wid, 0, 0.0, 0.0, SpringCfg::WOBBLY);
    let mut act = None;
    let tab = &mut g.hud.tab;
    side_panel(ui, "peon", wob, |ui| {
        // header: face, name, mood
        ui.row(64.0, |ui| {
            let r = ui.alloc(vec2(64.0, 64.0));
            if let Some(a) = &look {
                portrait(&mut ui.painter, r.center(), 30.0, a.skin, a.hair, a.shirt, mood.value, false);
            }
            ui.column(2.0, |ui| {
                ui.text(Text::new(p.full_name()).display().size(22.0));
                let mut sub = format!("{} · day {:.0} of {:.0}", age.stage.label(), age.days_in_stage + 1.0, age.stage_len);
                if let Some(school) = at_work {
                    sub.push_str(if school { " · at school" } else { " · at work" });
                }
                ui.text(Text::new(sub).size(12.5).soft());
            });
        });
        ui.row(26.0, |ui| {
            ui.text(Text::new("Mood").bold().size(13.5));
            let frac = ((mood.value + 100.0) / 200.0).clamp(0.0, 1.0);
            let r = ui.bar("mood", frac, theme::level_color(frac), vec2(ui.avail_w() - 48.0, 20.0));
            let _ = r;
            ui.text(Text::new(format!("{:+.0}", mood.value)).display().size(16.0));
        });
        if let Some((other, due)) = &pregnant {
            ui.text(Text::new(format!("🍼 Expecting with {other}, due in {due:.0}h")).size(13.0).color(CORAL.darken(0.2)));
        }
        // action queue
        ui.wrap(|ui| {
            if let Some(c) = &queue.current {
                if c.action.source != Source::System {
                    let r = ui.btn(Button::new(format!("▶ {}  ✖", c.action.label)).small().color(TEAL));
                    ui.tooltip(&r, "Doing this now. Click to cancel.");
                    if r.clicked {
                        act = Some(PeonAct::Cancel(c.action.id));
                    }
                } else {
                    tag(ui, &format!("▶ {}", c.action.label), TEAL, 13.0);
                }
            } else {
                tag(ui, "Idle", PAPER, 13.0);
            }
            for a in &queue.queue {
                let r = ui.btn(Button::new(format!("{}  ✖", a.label)).small().color(PAPER));
                ui.tooltip(&r, "Queued. Click to cancel.");
                if r.clicked {
                    act = Some(PeonAct::Cancel(a.id));
                }
            }
        });
        ui.dots();
        ui.row(0.0, |ui| {
            for (t, name, color) in [(PeonTab::Needs, "Needs", GRASS), (PeonTab::Mood, "Mood", SUN), (PeonTab::Personality, "Self", LILAC), (PeonTab::Work, "Work", SKY), (PeonTab::People, "People", PINK)] {
                if ui.btn(Button::new(name).small().font_size(12.5).color(color).selected(*tab == t)).clicked {
                    *tab = t;
                }
            }
        });
        let h = rest_h(ui);
        ui.scroll(("tab", *tab as u8), h, |ui| match *tab {
            PeonTab::Needs => {
                for n in needs.active_needs() {
                    let v = needs.get(n);
                    ui.row(22.0, |ui| {
                        let core = n.is_core();
                        let label = if core { n.label().to_string() } else { format!("{} ✨", n.label()) };
                        let lr = ui.alloc(vec2(104.0, 18.0));
                        ui.paint_text(vec2(lr.min.x, lr.center().y), vec2(0.0, 0.5), &label, FontId::Bold, 13.5, if core { INK } else { LILAC.darken(0.35) });
                        ui.bar(("need", n.idx()), v / 100.0, theme::level_color(v / 100.0), vec2(ui.avail_w() - 4.0, 18.0));
                    });
                }
            }
            PeonTab::Mood => {
                if mood.moodlets.is_empty() {
                    ui.small("No strong feelings right now.");
                }
                for m in &mood.moodlets {
                    ui.row(28.0, |ui| {
                        let col = if m.value >= 0.0 { GRASS } else { CORAL };
                        tag(ui, &format!("{:+.0}", m.value), col, 13.0);
                        ui.text(Text::new(m.name.clone()).bold().size(13.5));
                        ui.text(Text::new(format!("{}h", m.until.saturating_sub(now).div_ceil(60))).size(12.0).soft());
                    });
                }
            }
            PeonTab::Personality => {
                ui.section("Traits", LILAC);
                ui.wrap(|ui| {
                    for t in &pers.traits {
                        if let Some(d) = data.trait_def(t) {
                            let r = tag(ui, &d.name, LILAC, 13.0);
                            ui.tooltip(&r, d.desc.clone());
                        }
                    }
                    if let Some(q) = data.quirk(&pers.quirk) {
                        let r = tag(ui, &format!("Quirk: {}", q.name), PEACH, 13.0);
                        ui.tooltip(&r, q.desc.clone());
                    }
                });
                ui.section("Nature", PINK);
                let a = pers.axes;
                for (name, v) in [("Neat", a.neat), ("Outgoing", a.outgoing), ("Active", a.active), ("Playful", a.playful), ("Nice", a.nice)] {
                    ui.row(18.0, |ui| {
                        let lr = ui.alloc(vec2(90.0, 16.0));
                        ui.paint_text(vec2(lr.min.x, lr.center().y), vec2(0.0, 0.5), name, FontId::Bold, 13.0, INK);
                        let r = ui.alloc(vec2(10.0 * 19.0, 16.0));
                        for i in 0..10 {
                            let c = vec2(r.min.x + 8.0 + i as f32 * 19.0, r.center().y);
                            let on = i < v as i32;
                            ui.painter.circle(c, 6.5, Paint::fill(if on { PINK } else { INK.alpha(0.08) }).stroke(1.5, if on { INK } else { INK.alpha(0.2) }));
                        }
                    });
                }
                ui.section("Skills", SKY);
                let mut any = false;
                for s in Skill::ALL {
                    let v = skills.get(s);
                    if v >= 0.05 {
                        any = true;
                        ui.row(20.0, |ui| {
                            let lr = ui.alloc(vec2(90.0, 16.0));
                            ui.paint_text(vec2(lr.min.x, lr.center().y), vec2(0.0, 0.5), &format!("{s:?}"), FontId::Bold, 13.0, INK);
                            ui.bar(("skill", s as u8), v / 10.0, SKY, vec2(ui.avail_w() - 40.0, 14.0));
                            ui.text(Text::new(format!("{v:.1}")).size(12.5).soft());
                        });
                    }
                }
                if !any {
                    ui.small("No skills yet. Books and hobbies help!");
                }
                if let Some(gn) = &genes {
                    ui.section("Genes", GRASS);
                    ui.row(24.0, |ui| {
                        for (name, col) in [("skin", gn.skin), ("hair", gn.hair), ("eyes", gn.eyes)] {
                            let r = ui.swatch(rgb(col), false, 18.0);
                            ui.tooltip(&r, name);
                        }
                        ui.text(Text::new(format!("height {:.2} · build {:.2}", gn.height, gn.build)).size(12.5).soft());
                    });
                }
                ui.section("Free will", TEAL);
                ui.wrap(|ui| {
                    for (f, name) in [(None, "Default"), (Some(FreeWill::Off), "Off"), (Some(FreeWill::Low), "Low"), (Some(FreeWill::Full), "Full")] {
                        if ui.chip(name, fw == f).clicked {
                            act = Some(PeonAct::FreeWill(f));
                        }
                    }
                });
            }
            PeonTab::Work => {
                if let Some((letter, hw)) = student {
                    ui.section("School", SUN);
                    ui.row(28.0, |ui| {
                        tag(ui, &format!("Grade {letter}"), SUN, 14.0);
                        ui.text(Text::new(if hw { "homework done ✔" } else { "homework due" }).size(13.0).soft());
                    });
                }
                ui.section("Career", SKY);
                match &job {
                    Some(j) => {
                        if let Some(c) = data.career(&j.career) {
                            let lvl = &c.levels[j.level];
                            ui.text(Text::new(format!("{} · {}", c.name, lvl.title)).bold().size(15.0).wrap());
                            ui.text(Text::new(format!("Level {}", j.level + 1)).size(12.5).soft());
                            let days: Vec<&str> = lvl.days.iter().map(|d| ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"][*d as usize]).collect();
                            ui.wrap(|ui| {
                                tag(ui, &format!("{}/h", money(lvl.pay)), GRASS, 12.5);
                                tag(ui, &format!("{:02}:00–{:02}:00", lvl.start_hour, (lvl.start_hour + lvl.hours) % 24), PAPER, 12.5);
                                tag(ui, &days.join(" "), PAPER, 12.5);
                            });
                            ui.row(22.0, |ui| {
                                ui.text(Text::new("Performance").bold().size(13.0));
                                let f = ((j.performance + 100.0) / 200.0).clamp(0.0, 1.0);
                                ui.bar("perf", f, theme::level_color(f), vec2(ui.avail_w() - 4.0, 16.0));
                            });
                            if let Some(next) = c.levels.get(j.level + 1) {
                                let reqs: Vec<String> = next.skills.iter().map(|(s, v)| format!("{s:?} {v:.0}")).collect();
                                if !reqs.is_empty() {
                                    ui.small(format!("Next: {} (needs {})", next.title, reqs.join(", ")));
                                }
                            }
                        }
                        if ui.btn(Button::new("Quit job").small().color(CORAL)).clicked {
                            act = Some(PeonAct::Quit);
                        }
                    }
                    None if age.stage >= LifeStage::Teen => {
                        ui.small("Unemployed. Pick something:");
                        let options: Vec<&sim::data::CareerDef> = data.careers.iter().filter(|c| age.stage >= c.min_stage).collect();
                        let labels: Vec<String> = options.iter().map(|c| format!("{} · {}", c.name, c.levels[0].title)).collect();
                        if let Some(i) = ui.dropdown("find-job", "Find a job…", &labels, ui.avail_w() - 6.0) {
                            act = Some(PeonAct::Join(options[i].id.clone()));
                        }
                    }
                    None => {
                        ui.small("Too young to work. Lucky.");
                    }
                }
            }
            PeonTab::People => {
                if let Some((parents, children)) = &family {
                    if !parents.is_empty() {
                        ui.text(Text::new(format!("Parents: {parents}")).size(13.0).wrap());
                    }
                    if !children.is_empty() {
                        ui.text(Text::new(format!("Children: {children}")).size(13.0).wrap());
                    }
                }
                if rels.is_empty() {
                    ui.small("Doesn't know anyone yet.");
                }
                for (i, (name, label, friend, romance)) in rels.iter().enumerate() {
                    ui.with_id(i, |ui| {
                        ui.row(22.0, |ui| {
                            ui.text(Text::new(name.clone()).bold().size(14.0));
                            ui.text(Text::new(*label).size(12.5).soft());
                        });
                        ui.row(14.0, |ui| {
                            ui.text(Text::new("❤").size(11.0).color(CORAL));
                            ui.bar("romance", (romance + 100.0) / 200.0, PINK, vec2(120.0, 12.0));
                            ui.text(Text::new("😊").size(11.0));
                            ui.bar("friend", (friend + 100.0) / 200.0, SKY, vec2(120.0, 12.0));
                        });
                    });
                }
            }
        });
    });
    match act {
        Some(PeonAct::Cancel(id)) => g.sim.cancel(e, id),
        Some(PeonAct::FreeWill(f)) => {
            if let Some(mut b) = g.sim.world.get_mut::<Brain>(e) {
                b.free_will = f;
            }
        }
        Some(PeonAct::Quit) => {
            g.sim.world.remove::<Job>(e);
        }
        Some(PeonAct::Join(id)) => {
            let _ = sim::career::join_career(&mut g.sim.world, e, &id);
        }
        None => {}
    }
}

// ------------------------------------------------------------------------------------------------
// Buy and Build

/// A catalog tile: icon (a colour blob or icon art), name and a price tag.
#[allow(clippy::too_many_arguments)]
fn tile(ui: &mut Ui, label: &str, price: &str, color: Option<Color>, icon: Option<hearth_ui::ImageId>, selected: bool, enabled: bool, size: Vec2) -> Resp {
    let id = ui.make_id(("tile", label));
    let slot = ui.alloc(size + vec2(0.0, LIP));
    let body = Rect::from_min_size(slot.min, size);
    let resp = ui.interact(id, slot, enabled);
    ui.bubble_r(id, body, 16.0, if selected { SUN } else { CREAM }, &resp, enabled);
    let ic = body.min + vec2(22.0, 22.0);
    if let Some(c) = color {
        let c = if enabled { c } else { c.desaturate(0.8) };
        ui.painter.rect_rotated(Rect::from_center_size(ic, Vec2::splat(22.0)), 7.0, 0.2, Paint::gradient(c.lighten(0.25), c).stroke(2.0, INK));
    }
    if let Some(im) = icon {
        let r = Rect::from_center_size(ic + vec2(2.0, 2.0), Vec2::splat(IconSize::Tile.points()));
        ui.painter.image(&ui.images, im, r, 0.0, Color::WHITE.alpha(if enabled { 1.0 } else { 0.5 }), None);
    }
    let tg = ui.fonts.layout(price, FontId::Bold, 11.5, None);
    let tr = Rect::from_min_size(vec2(body.max.x - tg.size.x - 18.0, body.min.y + 8.0), tg.size + vec2(12.0, 4.0));
    ui.painter.rect(tr, tr.height() * 0.5, Paint::fill(if enabled { GRASS } else { INK.alpha(0.15) }).stroke(1.5, INK));
    ui.painter.galley(&mut ui.fonts, &tg, tr.min + vec2(6.0, 2.0), INK);
    let ng = ui.fonts.layout(label, FontId::Bold, 13.0, Some(size.x - 18.0));
    ui.painter.galley(&mut ui.fonts, &ng, vec2(body.min.x + 10.0, body.max.y - 8.0 - ng.size.y), INK.alpha(if enabled { 1.0 } else { 0.5 }));
    ui.painter.pop_transform();
    resp
}

const CATEGORIES: [Category; 11] =
    [Category::Seating, Category::Sleeping, Category::Surfaces, Category::Kitchen, Category::Bathroom, Category::Electronics, Category::Fun, Category::Kids, Category::Decor, Category::Lighting, Category::Misc];

fn buy_panel(ui: &mut Ui, g: &mut Game) {
    let data = g.sim.data();
    let funds = g.sim.world.res::<Household>().funds;
    let mut new_tool = None;
    let cat = &mut g.hud.buy_cat;
    let icons = &g.hud.icons;
    let tool = g.tool.clone();
    // a chip with icon art (when it loaded)
    let chip = |ui: &mut Ui, label: &str, icon: Option<Icon>, on: bool| {
        let mut b = Button::new(label).small().selected(on);
        b.icon = icon.and_then(|i| icons.get(i, IconSize::Chip));
        ui.btn(b)
    };
    side_panel(ui, "buy", 0.0, |ui| {
        ui.row(36.0, |ui| {
            ui.heading("Buy");
            ui.space((ui.avail_w() - 120.0).max(0.0));
            let mut b = Button::new("Sell").small().color(CORAL).selected(tool == Tool::Sell);
            b.icon = icons.get(Icon::Sell, IconSize::Chip);
            let r = ui.btn(b);
            ui.tooltip(&r, "Sell tool: click an object (or hover + Delete)");
            if r.clicked {
                new_tool = Some(Tool::Sell);
            }
        });
        ui.small("Click to place · R rotates · Esc cancels");
        ui.wrap(|ui| {
            if chip(ui, "All", None, cat.is_none()).clicked {
                *cat = None;
            }
            for c in CATEGORIES {
                if data.objects.iter().any(|o| o.category == c) && chip(ui, &format!("{c:?}"), Some(Icon::category(c)), *cat == Some(c)).clicked {
                    *cat = Some(c);
                }
            }
        });
        ui.dots();
        let h = rest_h(ui);
        ui.scroll(("catalog", cat.map(|c| c as u8)), h, |ui| {
            ui.wrap(|ui| {
                for o in data.objects.iter().filter(|o| cat.is_none_or(|c| o.category == c)) {
                    let sel = matches!(&tool, Tool::Place { def, .. } if *def == o.id);
                    let color = o.parts.iter().max_by(|a, b| (a.size.x * a.size.y * a.size.z).total_cmp(&(b.size.x * b.size.y * b.size.z))).map(|p| rgb(p.color));
                    let r = tile(ui, &o.name, &money(o.price), color, None, sel, funds >= o.price, vec2(150.0, 84.0));
                    let mut hover = o.desc.clone();
                    let ints: Vec<&str> = o.interactions.iter().map(|i| i.name.as_str()).collect();
                    if !ints.is_empty() {
                        hover = format!("{hover}\n\nCan: {}", ints.join(", "));
                    }
                    if funds < o.price {
                        hover = format!("{hover}\n\nCan't afford it yet.");
                    }
                    ui.tooltip(&r, hover);
                    if r.clicked {
                        new_tool = Some(Tool::Place { def: o.id.clone(), rot: 0 });
                    }
                }
            });
        });
    });
    if let Some(t) = new_tool {
        g.tool = t;
    }
}

fn build_panel(ui: &mut Ui, g: &mut Game) {
    let mut new_tool = None;
    let mut auto_roof = None;
    let lot_stats = {
        let lot = g.sim.world.res::<sim::lot::Lot>();
        format!("Walls {} · floor tiles {} · stairs {} · lot value {}", lot.walls.len(), lot.floors.len(), lot.stairs.len(), money(sim::economy::lot_value(&g.sim.world)))
    };
    let tool = g.tool.clone();
    let (mut wall_style, mut floor_style, mut pitch) = (g.wall_style, g.floor_style, g.roof_pitch);
    let mut tool_mut = g.tool.clone();
    let icons = &g.hud.icons;
    side_panel(ui, "build", 0.0, |ui| {
        ui.heading("Build");
        ui.small("Drag to draw · R rotates stairs · PgUp/PgDn change level · Esc cancels");
        let h = rest_h(ui);
        ui.scroll("build", h, |ui| {
            let tools = [
                (Tool::Room, Icon::Room, "Room", format!("{}/m", money(COST_WALL)), "Walls and floor in one drag"),
                (Tool::Wall, Icon::Wall, "Wall", format!("{}/m", money(COST_WALL)), "Drag along the grid"),
                (Tool::DeleteWall, Icon::KnockDown, "Knock down", "½ back".into(), "Delete walls for half their price back"),
                (Tool::Door, Icon::Door, "Door", money(COST_DOOR), "Click a wall"),
                (Tool::Window, Icon::Window, "Window", money(COST_WINDOW), "Click a wall"),
                (Tool::Floor, Icon::Floor, "Floor", money(COST_FLOOR), "Drag a rectangle (price per tile)"),
                (Tool::DeleteFloor, Icon::RipFloor, "Rip floor", "free".into(), "Delete floor tiles"),
                (Tool::Stairs { dir: sim::lot::Dir4::PosZ }, Icon::Stairs, "Stairs", money(COST_STAIRS), "R rotates"),
                (Tool::Basement, Icon::Dig, "Dig", money(COST_DIG), "Dig a basement, per tile (basement levels only)"),
                (Tool::Roof { kind: sim::lot::RoofKind::Gable }, Icon::Roof, "Roof", money(COST_ROOF_TILE), "Per tile, on the level you're viewing"),
                (Tool::DeleteRoof, Icon::NoRoof, "No roof", "free".into(), "Delete roofs"),
                (Tool::Terrain { brush: sim::lot::TerrainBrush::Raise, radius: 2.0 }, Icon::Terrain, "Terrain", money(COST_TERRAIN), "Hold to sculpt (price per corner)"),
            ];
            ui.wrap(|ui| {
                for (t, icon, name, price, hint) in tools {
                    let sel = std::mem::discriminant(&tool) == std::mem::discriminant(&t);
                    let r = tile(ui, name, &price, None, icons.get(icon, IconSize::Tile), sel, true, vec2(98.0, 78.0));
                    ui.tooltip(&r, hint);
                    if r.clicked {
                        new_tool = Some(t);
                    }
                }
            });
            match &mut tool_mut {
                Tool::Roof { kind } => {
                    ui.section("Roof", CORAL);
                    ui.wrap(|ui| {
                        for (k, name) in [(sim::lot::RoofKind::Gable, "Gable"), (sim::lot::RoofKind::Hip, "Hip"), (sim::lot::RoofKind::Flat, "Flat")] {
                            if ui.chip(name, *kind == k).clicked {
                                *kind = k;
                            }
                        }
                    });
                    ui.slider("Pitch", &mut pitch, 0.2, 1.0, 0.0, 170.0);
                }
                Tool::Terrain { brush, radius } => {
                    ui.section("Terrain brush", GRASS);
                    ui.wrap(|ui| {
                        use sim::lot::TerrainBrush as B;
                        for (b, name) in [(B::Raise, "Raise"), (B::Lower, "Lower"), (B::Flatten, "Flatten"), (B::Smooth, "Smooth")] {
                            if ui.chip(name, *brush == b).clicked {
                                *brush = b;
                            }
                        }
                    });
                    ui.slider("Radius", radius, 0.5, 6.0, 0.0, 170.0);
                    ui.small("Ground under floors, walls and furniture stays put.");
                }
                _ => {}
            }
            ui.section("Auto-roof", PEACH);
            ui.wrap(|ui| {
                for (k, name) in [(sim::lot::RoofKind::Gable, "Gable"), (sim::lot::RoofKind::Hip, "Hip"), (sim::lot::RoofKind::Flat, "Flat")] {
                    let r = ui.btn(Button::new(name).small().color(PEACH));
                    ui.tooltip(&r, "Replace all roofs with automatic ones over the top floors");
                    if r.clicked {
                        auto_roof = Some(k);
                    }
                }
            });
            ui.section("Wall covering", SKY);
            ui.wrap(|ui| {
                let names = ["Cream plaster", "White plaster", "Sky plaster", "Brick", "Sage plaster", "Subway tile", "Striped wallpaper", "Beadboard"];
                for (s, name) in (0u16..).zip(names) {
                    let r = ui.swatch(rgb(crate::scene::wall_color(s)), wall_style == s, 26.0);
                    ui.tooltip(&r, name);
                    if r.clicked {
                        wall_style = s;
                    }
                }
            });
            ui.section("Floor", SUN);
            ui.wrap(|ui| {
                for (s, name) in [(0u16, "Wood"), (6, "Parquet"), (1, "Tile"), (4, "Marble"), (2, "Carpet"), (3, "Concrete"), (5, "Stone")] {
                    if ui.chip(name, floor_style == s).clicked {
                        floor_style = s;
                    }
                }
            });
            ui.dots();
            ui.small(lot_stats.clone());
        });
    });
    g.wall_style = wall_style;
    g.floor_style = floor_style;
    g.roof_pitch = pitch;
    if std::mem::discriminant(&tool_mut) == std::mem::discriminant(&g.tool) {
        g.tool = tool_mut;
    }
    if let Some(t) = new_tool {
        g.tool = t;
        g.drag_start = None;
    }
    if let Some(k) = auto_roof {
        let pitch = g.roof_pitch;
        let n = g.sim.lot_mut().auto_roof(k, pitch);
        g.say(format!("{n} roof(s) built."));
    }
}

// ------------------------------------------------------------------------------------------------
// World bubbles, pie menu, events, toasts

fn labels(ui: &mut Ui, cx: &UiCtx) {
    let ppp = ui.input.ppp;
    let screen_w = ui.screen.width();
    ui.area("labels", Order::Background, Vec2::ZERO, screen_w, |ui| {
        for l in cx.labels {
            let Some((x, y)) = cx.camera.to_screen(l.world, cx.screen.0, cx.screen.1) else { continue };
            let pos = vec2(x, y) / ppp;
            let id = Id::new(("label", l.kind as u8, &l.text));
            let pop = ui.spring(id, 0, 1.0, 0.2, SpringCfg::BOUNCY);
            let (font, size, bg, fg) = match l.kind {
                LabelKind::Speech => (FontId::Bold, 14.0, PAPER, INK),
                LabelKind::Thought => (FontId::Body, 13.0, Color::hex(0xEFE8FF), INK),
                LabelKind::Name => (FontId::Bold, 12.5, INK, CREAM),
                LabelKind::Alert => (FontId::Bold, 13.0, CORAL, PAPER),
            };
            let gal = ui.fonts.layout(&l.text, font, size, Some(260.0));
            let pad = vec2(10.0, 5.0);
            let lift = if l.kind == LabelKind::Name { 0.0 } else { 10.0 };
            let r = Rect::from_min_size(pos - vec2(gal.size.x * 0.5 + pad.x, gal.size.y + pad.y * 2.0 + lift), gal.size + pad * 2.0);
            let rot = if l.kind == LabelKind::Alert { (ui.time * 9.0).sin() * 0.05 } else { 0.0 };
            ui.painter.push_transform(pos, Vec2::splat(pop), rot, Vec2::ZERO);
            let rad = (r.height() * 0.5).min(14.0);
            ui.painter.rect(r.translate(vec2(0.0, 3.0)), rad, Paint::fill(INK.alpha(0.2)));
            match l.kind {
                LabelKind::Speech => {
                    ui.painter.triangle(vec2(pos.x, r.max.y + 4.0), vec2(6.0, 6.0), PI, 0.5, Paint::fill(bg).stroke(2.0, INK));
                    ui.painter.rect(r, rad, Paint::fill(bg).stroke(2.0, INK));
                    ui.painter.triangle(vec2(pos.x, r.max.y + 1.0), vec2(4.5, 4.5), PI, 0.5, Paint::fill(bg));
                }
                LabelKind::Thought => {
                    ui.painter.circle(vec2(pos.x - 4.0, r.max.y + 5.0), 4.0, Paint::fill(bg).stroke(2.0, INK));
                    ui.painter.circle(vec2(pos.x - 1.0, r.max.y + 11.0), 2.5, Paint::fill(bg).stroke(1.5, INK));
                    ui.painter.rect(r, rad, Paint::fill(bg).stroke(2.0, INK));
                }
                _ => ui.painter.rect(r, rad, Paint::fill(bg).stroke(2.0, INK)),
            }
            ui.painter.galley(&mut ui.fonts, &gal, r.min + pad, fg);
            ui.painter.pop_transform();
        }
    });
}

fn pie_menu(ui: &mut Ui, g: &mut Game) {
    let Some(menu) = g.menu.clone() else { return };
    let ppp = ui.input.ppp;
    let key = Id::new(("pie", &menu.title, menu.screen.0.to_bits(), menu.screen.1.to_bits()));
    let opened = match ui.get_f(key, 0) {
        Some(t) => t,
        None => {
            ui.set_f(key, 0, ui.time);
            ui.time
        }
    };
    let n = menu.options.len().max(1);
    let radius = 64.0 + (n as f32 * 10.0).min(80.0);
    let s = ui.screen;
    // keep clear of the side panel
    let right = s.max.x - if g.selected.is_some() { PANEL_W + 30.0 } else { 0.0 };
    let c = vec2((menu.screen.0 / ppp).clamp(radius + 140.0, (right - radius - 140.0).max(radius + 140.0)), (menu.screen.1 / ppp).clamp(radius + 40.0, (s.max.y - radius - 40.0).max(radius + 40.0)));
    let mut chosen = None;
    let mut close = false;
    let since = ui.time - opened;
    ui.area("pie", Order::Popup, Vec2::ZERO, s.width(), |ui| {
        let hub = ui.spring(key, 1, 1.0, 0.0, SpringCfg::BOUNCY);
        // spokes
        for i in 0..menu.options.len() {
            let a = -PI * 0.5 + i as f32 * 2.0 * PI / n as f32;
            let p = c + vec2(a.cos(), a.sin()) * radius * hub;
            ui.painter.line(c, p, 3.0, INK.alpha(0.18));
        }
        // centre: title + cancel
        let tg = ui.fonts.layout(&menu.title, FontId::Display, 16.0, Some(200.0));
        let tr = Rect::from_center_size(c - vec2(0.0, 6.0), tg.size + vec2(24.0, 12.0));
        ui.painter.push_transform(c, Vec2::splat(hub), 0.0, Vec2::ZERO);
        ui.painter.rect(tr.translate(vec2(0.0, 4.0)), 14.0, Paint::fill(INK.alpha(0.25)));
        ui.painter.rect(tr, 14.0, Paint::fill(INK).stroke(OUTLINE, INK));
        ui.painter.galley(&mut ui.fonts, &tg, tr.min + vec2(12.0, 6.0), CREAM);
        ui.painter.pop_transform();
        ui.block(tr);
        let cr = Rect::from_center_size(vec2(c.x, tr.max.y + 20.0), vec2(26.0, 26.0 + LIP));
        ui.region(cr, 0.0, |ui| {
            let r = ui.btn(Button::new("✖").round().height(26.0).font_size(11.0).color(CORAL));
            ui.tooltip(&r, "Never mind (Esc)");
            close |= r.clicked;
        });
        if menu.options.is_empty() {
            let r = Rect::from_center_size(c + vec2(0.0, -tr.height() - 18.0), vec2(160.0, 26.0));
            ui.region(r, 0.0, |ui| tag(ui, "Nothing to do here.", PAPER, 13.0));
        }
        for (i, (label, kind, ok)) in menu.options.iter().enumerate() {
            let a = -PI * 0.5 + i as f32 * 2.0 * PI / n as f32;
            let dir = vec2(a.cos(), a.sin());
            let pid = key.with(i);
            let pop = ui.spring(pid, 0, if since > i as f32 * 0.035 || ui.instant { 1.0 } else { 0.0 }, 0.0, SpringCfg::BOUNCY);
            let p = c + dir * radius * pop.max(0.0);
            let lw = ui.fonts.layout(label, FontId::Bold, 15.0, None).size.x + 34.0;
            let h = 34.0;
            let x = if dir.x > 0.35 {
                p.x - 16.0
            } else if dir.x < -0.35 {
                p.x - lw + 16.0
            } else {
                p.x - lw * 0.5
            };
            let r = Rect::new(x, p.y - h * 0.5, lw, h + LIP);
            ui.painter.push_transform(p, Vec2::splat(pop.max(0.0)), 0.0, Vec2::ZERO);
            let colors = [TEAL, SUN, PINK, SKY, GRASS, PEACH, LILAC];
            ui.region(r, 0.0, |ui| {
                let resp = ui.btn(Button::new(label.clone()).color(colors[i % colors.len()]).enabled(ok.is_ok()));
                if let Err(why) = ok {
                    ui.tooltip(&resp, why.clone());
                }
                if resp.clicked {
                    chosen = Some(kind.clone());
                }
            });
            ui.painter.pop_transform();
            ui.block(r);
        }
    });
    if let Some(kind) = chosen {
        if let Err(why) = g.sim.order(menu.peon, kind) {
            g.say(why);
        }
        close = true;
    }
    if close {
        g.menu = None;
    }
}

fn career_events(ui: &mut Ui, g: &mut Game) {
    let items = g.sim.world.res::<PendingChoices>().items.clone();
    let Some(item) = items.first().cloned() else { return };
    let name = g.sim.name(item.peon);
    let career = g.sim.data().career(&item.career).map(|c| c.name.clone()).unwrap_or_default();
    let mut pick = None;
    let center = ui.screen.center() - vec2(0.0, 60.0);
    ui.window(("career", item.peon), &format!("{name} at work ({career})"), center, 440.0, false, |ui| {
        ui.text(Text::new(item.text.clone()).size(15.5).wrap());
        ui.space(4.0);
        let colors = [TEAL, SUN, PINK, SKY];
        for (i, c) in item.choices.iter().enumerate() {
            if ui.btn(Button::new(c.clone()).color(colors[i % colors.len()]).min_w(ui.avail_w())).clicked {
                pick = Some(i);
            }
        }
        ui.small("If you ignore this, they'll decide on their own when the shift ends.");
    });
    if let Some(i) = pick {
        g.sim.resolve_choice(0, i);
    }
}

fn toasts(ui: &mut Ui, g: &mut Game) {
    let x = if g.creator.is_some() { 510.0 } else { 116.0 };
    let size = ui.area_size("toasts").unwrap_or(Vec2::ZERO);
    let y = ui.screen.max.y - 16.0 - size.y;
    let toasts = g.toasts.clone();
    let now = g.time;
    ui.area("toasts", Order::Panel, vec2(x, y), 480.0, |ui| {
        ui.column(10.0, |ui| {
            for t in &toasts {
                let age = now - t.born;
                let id = Id::new(("toast", t.born.to_bits(), &t.text));
                let s = ui.spring(id, 0, if age > 8.3 { 0.0 } else { 1.0 }, 0.0, SpringCfg::BOUNCY).max(0.0);
                let gal = ui.fonts.layout(&t.text, FontId::Body, 14.0, Some(400.0));
                let w = gal.size.x + 58.0;
                let at = ui.cursor();
                ui.painter.push_transform(at + vec2(0.0, 20.0), Vec2::splat(s), (1.0 - s) * -0.3, vec2((1.0 - s) * -80.0, 0.0));
                let col = log_color(t.kind);
                ui.card(CREAM, 10.0, Some(w), |ui| {
                    ui.row(0.0, |ui| {
                        let r = ui.alloc(vec2(16.0, gal.size.y.min(20.0)));
                        ui.painter.circle(r.center(), 7.0, Paint::gradient(col.lighten(0.3), col).stroke(2.0, INK));
                        let tr = ui.alloc(gal.size);
                        ui.painter.galley(&mut ui.fonts, &gal, tr.min, INK);
                    });
                });
                ui.painter.pop_transform();
            }
        });
    });
}

// ------------------------------------------------------------------------------------------------
// Windows

fn saves_window(ui: &mut Ui, g: &mut Game) {
    use crate::saves;
    enum Act {
        Save(String),
        Load(std::path::PathBuf),
        Delete(std::path::PathBuf),
    }
    let mut act = None;
    let slots = saves::list();
    let center = ui.screen.center();
    let name = &mut g.save_name;
    let keep = ui.window("saves", "💾 Saves", center, 460.0, true, |ui| {
        ui.row(36.0, |ui| {
            ui.text_field("save-name", name, "name your save", 260.0);
            let ok = saves::sanitize(name);
            if ui.btn(Button::new("Save").color(TEAL).enabled(ok.is_some())).clicked
                && let Some(n) = ok
            {
                act = Some(Act::Save(n));
            }
        });
        ui.dots();
        if slots.is_empty() {
            ui.small("No saves yet.");
        }
        ui.scroll("slots", 320.0, |ui| {
            for s in &slots {
                ui.with_id(&s.name, |ui| {
                    ui.row(34.0, |ui| {
                        ui.column(0.0, |ui| {
                            ui.text(Text::new(if s.name == saves::AUTOSAVE { format!("{} (auto)", s.name) } else { s.name.clone() }).bold().size(14.5));
                            ui.text(Text::new(saves::age_label(s.modified)).size(12.0).soft());
                        });
                        let bw = if s.name != saves::AUTOSAVE { 210.0 } else { 104.0 };
                        ui.space((ui.avail_w() - bw).max(0.0));
                        if ui.btn(Button::new("Load").small().color(SKY)).clicked {
                            act = Some(Act::Load(s.path.clone()));
                        }
                        if s.name != saves::AUTOSAVE && ui.btn(Button::new("Overwrite").small().color(SUN)).clicked {
                            act = Some(Act::Save(s.name.clone()));
                        }
                        let r = ui.btn(Button::new("✖").small().round().color(CORAL));
                        ui.tooltip(&r, "Delete");
                        if r.clicked {
                            act = Some(Act::Delete(s.path.clone()));
                        }
                    });
                });
            }
        });
    });
    if !keep {
        g.show_saves = false;
    }
    match act {
        Some(Act::Save(n)) => {
            g.save_to(&saves::path_for(&n));
            g.save_name.clear();
        }
        Some(Act::Load(p)) => {
            g.load_from(&p);
            g.show_saves = false;
        }
        Some(Act::Delete(p)) => match std::fs::remove_file(&p) {
            Ok(()) => g.say(format!("Deleted {}", p.display())),
            Err(e) => g.say(format!("Delete failed: {e}")),
        },
        None => {}
    }
}

/// What the player asked for in the neighborhood window.
enum HoodAction {
    Play(usize),
    Visit(usize),
    GoOut(usize),
    GoHome,
    Move(usize),
    NewHousehold,
    Invite(Entity),
}

fn neighborhood_window(ui: &mut Ui, g: &mut Game) {
    use sim::neighborhood::{self as nh, LotKind};
    let mut action = None;
    let w = &g.sim.world;
    let hood = w.res::<nh::Neighborhood>().clone();
    let hh = w.res::<Household>().clone();
    let members: Vec<Entity> = g.sim.living_peons().into_iter().filter(|m| w.has::<Member>(*m)).collect();
    let center = ui.screen.center();
    let keep = ui.window("hood", &format!("🏡 {}", hood.name), center, 560.0, true, |ui| {
        ui.small("Play any household on the street, visit the neighbors, move house, or head out to the park, café or gym. Everyone shares one clock: life goes on next door while you're away.");
        let h = (ui.screen.max.y - ui.cursor().y - 60.0).clamp(160.0, 560.0);
        ui.scroll("lots", h, |ui| {
            for (i, lot) in hood.lots.iter().enumerate() {
                ui.with_id(i, |ui| {
                    let here = i == hood.active;
                    let residents = nh::residents(w, i);
                    let fill = if here { theme::SUN.mix(CREAM, 0.6) } else { PAPER };
                    ui.card(fill, 10.0, None, |ui| {
                        ui.row(26.0, |ui| {
                            ui.text(Text::new(lot.title()).display().size(16.0));
                            if here {
                                tag(ui, "you are here", SUN, 11.5);
                            }
                        });
                        match (lot.kind, &residents) {
                            (LotKind::Community, _) => {
                                ui.text(Text::new("Community lot").size(13.0).soft());
                                if here {
                                    if ui.btn(Button::new("Go home").small().color(SKY)).clicked {
                                        action = Some(HoodAction::GoHome);
                                    }
                                } else {
                                    let r = ui.btn(Button::new("Go out").small().color(GRASS));
                                    ui.tooltip(&r, "Take the household here for a few hours");
                                    if r.clicked {
                                        action = Some(HoodAction::GoOut(i));
                                    }
                                }
                            }
                            (LotKind::Residential, Some(r)) => {
                                let firsts: Vec<String> = r.members.iter().filter(|e| !w.has::<Dead>(**e)).filter_map(|e| w.get::<Peon>(*e).map(|p| p.first.clone())).collect();
                                ui.text(Text::new(format!("{} ({})", r.name, firsts.join(", "))).size(13.5).wrap());
                                ui.row(0.0, |ui| {
                                    if r.played {
                                        tag(ui, if here { "home sweet home" } else { "your home" }, GRASS, 12.0);
                                    } else {
                                        if here {
                                            tag(ui, "visiting", PAPER, 12.0);
                                            if ui.btn(Button::new("Go home").small().color(SKY)).clicked {
                                                action = Some(HoodAction::GoHome);
                                            }
                                        } else {
                                            let b = ui.btn(Button::new("Visit").small().color(PINK));
                                            ui.tooltip(&b, "Take the household over (8:00–22:00)");
                                            if b.clicked {
                                                action = Some(HoodAction::Visit(i));
                                            }
                                        }
                                        let b = ui.btn(Button::new("Play them").small().color(LILAC));
                                        ui.tooltip(&b, format!("{} in the bank", money(r.funds)));
                                        if b.clicked {
                                            action = Some(HoodAction::Play(i));
                                        }
                                    }
                                });
                            }
                            (LotKind::Residential, None) => {
                                let value = nh::build_value(w, i);
                                let price = nh::price(w, i);
                                ui.text(Text::new(if value > 0 { format!("For sale: furnished house, {}", money(price)) } else { format!("For sale: empty plot, {}", money(price)) }).size(13.5).color(GRASS.darken(0.35)));
                                let sale = hh.home.map(|h| nh::price(w, h)).unwrap_or(0);
                                let afford = hh.funds + sale >= price;
                                ui.row(0.0, |ui| {
                                    let b = ui.btn(Button::new("Move here").small().color(TEAL).enabled(afford));
                                    ui.tooltip(&b, format!("Sell your place for {} and buy this for {}", money(sale), money(price)));
                                    if b.clicked {
                                        action = Some(HoodAction::Move(i));
                                    }
                                    if ui.btn(Button::new("New household…").small().color(GRASS)).clicked {
                                        action = Some(HoodAction::NewHousehold);
                                    }
                                });
                            }
                        }
                    });
                });
            }
            ui.section("Neighbors", PINK);
            for e in nh::neighbors(w) {
                let Some(p) = w.get::<Peon>(e).map(|p| p.full_name()) else { continue };
                let address = nh::home_of(w, e).map(|l| hood.lots[l].address.clone()).unwrap_or_default();
                let status = if w.has::<Dead>(e) {
                    "passed on"
                } else if w.has::<Visitor>(e) {
                    "here now"
                } else {
                    "at home"
                };
                let best = members.iter().map(|m| w.res::<Relationships>().get(*m, e)).max_by(|a, b| a.friendship.total_cmp(&b.friendship));
                let rel = best.map(|r| r.label()).unwrap_or("Stranger");
                ui.with_id(e, |ui| {
                    ui.row(30.0, |ui| {
                        ui.column(0.0, |ui| {
                            ui.text(Text::new(p).bold().size(13.5));
                            ui.text(Text::new(format!("{address} · {status} · {rel}")).size(12.0).soft());
                        });
                        let grown = w.get::<Age>(e).is_some_and(|a| a.stage >= LifeStage::Teen);
                        if w.has::<Offlot>(e) && grown && !w.has::<Dead>(e) {
                            ui.space((ui.avail_w() - 110.0).max(0.0));
                            if ui.btn(Button::new("Invite over").small().color(PINK)).clicked {
                                action = Some(HoodAction::Invite(e));
                            }
                        }
                    });
                });
            }
        });
    });
    if !keep {
        g.show_neighborhood = false;
    }
    let Some(action) = action else { return };
    let result = match action {
        HoodAction::Play(l) => g.sim.play_household(l).map(|_| "Switched households."),
        HoodAction::Visit(l) => g.sim.visit(l).map(|_| "They're walking over."),
        HoodAction::GoOut(l) => g.sim.go_out(l).map(|_| "Off they go!"),
        HoodAction::GoHome => g.sim.go_home().map(|_| "Heading home."),
        HoodAction::Move(l) => g.sim.move_household(l).map(|_| "Moved in!"),
        HoodAction::NewHousehold => {
            g.show_neighborhood = false;
            g.open_creator();
            Ok("Create the new household, then pick their lot.")
        }
        HoodAction::Invite(e) => {
            g.sim.invite(e);
            Ok("Invitation sent. They'll be right over.")
        }
    };
    match result {
        Ok(msg) => g.say(msg),
        Err(e) => g.say(e),
    }
    g.check_lot_change();
}

// ------------------------------------------------------------------------------------------------
// Create-a-Household

fn creator_screen(ui: &mut Ui, g: &mut Game) {
    let Some(c) = g.creator.as_mut() else { return };
    match crate::creator::screen(ui, c) {
        crate::creator::CreatorAction::MoveIn => {
            let c = g.creator.take().expect("creator open");
            match c.move_in_to(&mut g.sim) {
                Ok(()) => {
                    g.check_lot_change();
                    g.say("Welcome to the neighborhood!");
                }
                Err(e) => {
                    let mut c = c;
                    c.error = Some(e);
                    g.creator = Some(c);
                }
            }
        }
        crate::creator::CreatorAction::Cancel => g.creator = None,
        crate::creator::CreatorAction::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_has_separators() {
        assert_eq!(money(0), "$0");
        assert_eq!(money(950), "$950");
        assert_eq!(money(20_028), "$20,028");
        assert_eq!(money(-1_234_567), "-$1,234,567");
    }
}
