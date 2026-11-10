//! Developer windows (F1 inspector, F2 autonomy, F3 overlays, F4 profiler, F8 settings, L story log).
//! These stay on egui: they're tools, not part of the game's look. The player-facing interface
//! lives in [`crate::ui`].

use egui::{Color32, RichText};
use sim::components::*;
use sim::data::Need;
use sim::economy::Household;
use sim::log::{LogKind, SimLog};
use sim::objects::ObjectInstance;
use sim::time::{FreeWill, Settings};

use crate::state::*;

pub struct DebugCtx {
    pub render_stats: hearth_render::RenderStats,
}

fn kind_color(k: LogKind) -> Color32 {
    match k {
        LogKind::Funny => Color32::from_rgb(255, 220, 120),
        LogKind::Social => Color32::from_rgb(240, 160, 220),
        LogKind::Career => Color32::from_rgb(140, 200, 255),
        LogKind::Life => Color32::from_rgb(170, 240, 170),
        LogKind::Death => Color32::from_rgb(200, 200, 200),
        LogKind::Money => Color32::from_rgb(140, 230, 140),
        LogKind::Warning => Color32::from_rgb(255, 120, 100),
        _ => Color32::from_rgb(220, 220, 220),
    }
}

/// Whether any developer window is open (egui only runs then).
pub fn any_open(g: &Game) -> bool {
    let d = g.debug;
    d.inspector || d.autonomy || d.overlays || d.profiler || d.settings || d.log
}

pub fn draw(ui: &mut egui::Ui, g: &mut Game, cx: &DebugCtx) {
    let ctx = ui.ctx().clone();
    if g.debug.inspector {
        let mut open = true;
        egui::Window::new("Inspector (F1)").open(&mut open).default_width(420.0).show(&ctx, |ui| {
            let w = &g.sim.world;
            ui.label(format!("{} entities · {} archetypes", w.entity_count(), w.archetype_count()));
            ui.horizontal(|ui| {
                if ui.button("Selected peon").clicked() {
                    g.inspect_entity = g.selected;
                }
                if ui.button("Hovered").clicked() {
                    g.inspect_entity = g.hovered;
                }
            });
            egui::ScrollArea::vertical().id_salt("ents").max_height(140.0).show(ui, |ui| {
                let mut ents = w.entities();
                ents.sort();
                for e in ents {
                    let name = w
                        .get::<Peon>(e)
                        .map(|p| p.full_name())
                        .or_else(|| w.get::<ObjectInstance>(e).map(|o| g.sim.data().object(o.def).id.clone()))
                        .unwrap_or_else(|| "?".into());
                    if ui.selectable_label(g.inspect_entity == Some(e), format!("{e} {name}")).clicked() {
                        g.inspect_entity = Some(e);
                    }
                }
            });
            ui.separator();
            if let Some(e) = g.inspect_entity {
                egui::ScrollArea::vertical().id_salt("comps").show(ui, |ui| {
                    for (name, dump) in w.inspect(e) {
                        let short = name.rsplit("::").next().unwrap_or(name);
                        egui::CollapsingHeader::new(short).id_salt(name).show(ui, |ui| {
                            ui.label(RichText::new(dump.unwrap_or_else(|| "(no Debug registered)".into())).monospace().small());
                        });
                    }
                });
            }
        });
        g.debug.inspector = open;
    }
    if g.debug.autonomy {
        let mut open = true;
        egui::Window::new("Autonomy (F2)").open(&mut open).default_width(360.0).show(&ctx, |ui| {
            if let Some(e) = g.selected {
                let w = &g.sim.world;
                if let Some(b) = w.get::<Brain>(e) {
                    ui.label(format!("{} — last think @ tick {}", g.sim.name(e), b.last_think_tick));
                    if let Some(n) = b.unfixable {
                        ui.colored_label(Color32::RED, format!("Nothing can fix critical {}", n.label()));
                    }
                    let max = b.last_scores.first().map(|s| s.score).unwrap_or(1.0).max(0.001);
                    for s in &b.last_scores {
                        ui.horizontal(|ui| {
                            ui.add(egui::ProgressBar::new(s.score / max).desired_width(90.0).text(format!("{:.1}", s.score)));
                            ui.label(&s.label);
                        });
                    }
                    ui.separator();
                    ui.label(RichText::new("Recent (boredom):").small());
                    for (l, t) in &b.recent {
                        ui.label(RichText::new(format!("{l} @ {t}")).small().weak());
                    }
                }
            } else {
                ui.label("Select a peon.");
            }
        });
        g.debug.autonomy = open;
    }
    if g.debug.overlays {
        let mut open = true;
        egui::Window::new("Overlays (F3)").open(&mut open).show(&ctx, |ui| {
            ui.label("Room ids, occupancy and paths are drawn while this window is open.");
            if let Some(Pick::Floor { level, pos }) = &g.hover_pick {
                let t = sim::lot::Lot::tile_of(*pos);
                let lot = g.sim.world.res::<sim::lot::Lot>();
                ui.label(format!("tile ({}, {}) level {}", t.x, t.y, level));
                ui.label(format!("walkable {} · ground {} · floor {}", lot.walkable(*level, t), lot.has_ground(*level, t), lot.has_floor(*level, t)));
                if let Some(r) = lot.room_at(*level, t) {
                    ui.label(format!("room {} indoor {} tiles {} beauty {:.1} dirt {:.2} score {:.0}", r.id, r.indoor, r.tiles, r.beauty, r.dirt, r.score));
                }
            }
        });
        g.debug.overlays = open;
    }
    if g.debug.profiler {
        let mut open = true;
        egui::Window::new("Profiler (F4)").open(&mut open).show(&ctx, |ui| {
            ui.label(format!("frame {:.2} ms · sim step {:.2} ms", g.frame_ms, g.sim_ms));
            ui.label(format!("draws {} · batches {} · meshes {}", cx.render_stats.draws, cx.render_stats.batches, cx.render_stats.meshes));
            ui.separator();
            egui::Grid::new("timings").striped(true).show(ui, |ui| {
                for t in g.sim.timings() {
                    ui.label(&t.stage);
                    ui.label(&t.system);
                    ui.label(format!("{:.1} µs", t.avg.as_secs_f64() * 1e6));
                    ui.end_row();
                }
            });
        });
        g.debug.profiler = open;
    }
    if g.debug.settings {
        let mut open = true;
        egui::Window::new("Sim settings (F8)").open(&mut open).show(&ctx, |ui| {
            let mut s = g.sim.world.res::<Settings>().clone();
            ui.horizontal(|ui| {
                ui.label("Free will:");
                ui.radio_value(&mut s.free_will, FreeWill::Off, "Off");
                ui.radio_value(&mut s.free_will, FreeWill::Low, "Low");
                ui.radio_value(&mut s.free_will, FreeWill::Full, "Full");
            });
            ui.add(egui::Slider::new(&mut s.lifespan_scale, 0.25..=4.0).text("lifespan scale"));
            ui.checkbox(&mut s.aging, "Aging");
            ui.checkbox(&mut s.bills, "Bills");
            ui.checkbox(&mut s.accidents, "Accidents");
            ui.checkbox(&mut s.auto_career_choices, "Auto-resolve career events");
            ui.checkbox(&mut s.trace_actions, "Trace actions to log");
            *g.sim.world.res_mut::<Settings>() = s;
            ui.separator();
            ui.label("Lighting (GPU cost)");
            let before = g.graphics;
            ui.checkbox(&mut g.graphics.ambient_occlusion, "Ambient occlusion");
            ui.add_enabled_ui(g.graphics.ambient_occlusion, |ui| {
                ui.horizontal(|ui| {
                    ui.label("AO quality");
                    for quality in hearth_render::AoQuality::ALL {
                        ui.selectable_value(&mut g.graphics.ao_quality, quality, quality.label());
                    }
                });
            });
            ui.checkbox(&mut g.graphics.indirect_light, "Indirect light / color bounce (costly)");
            ui.checkbox(&mut g.graphics.shadows, "Sun shadows");
            ui.add_enabled(g.graphics.shadows, egui::Checkbox::new(&mut g.graphics.soft_shadows, "Soft shadow filtering"));
            ui.small("Indirect light uses High AO quality. Nearby geometry bounds approximate complex models.");
            if before != g.graphics { g.graphics.save(); }
            ui.separator();
            ui.label("Cheats");
            ui.horizontal(|ui| {
                if ui.button("+$10,000").clicked() {
                    g.sim.world.res_mut::<Household>().funds += 10_000;
                }
                if ui.button("Max needs (selected)").clicked()
                    && let Some(e) = g.selected
                        && let Some(mut n) = g.sim.world.get_mut::<Needs>(e) {
                            for k in Need::ALL {
                                n.set(k, 100.0);
                            }
                        }
                if ui.button("Age up (selected)").clicked()
                    && let Some(e) = g.selected {
                        sim::life::grow_up(&mut g.sim.world, e);
                    }
            });
            if ui.button("Reload data from assets/data").clicked() {
                match sim::data::GameData::load_dir(std::path::Path::new("assets/data")) {
                    Ok(d) => {
                        g.sim.world.insert_resource(sim::sim::Data(std::sync::Arc::new(d)));
                        g.say("Data reloaded.");
                    }
                    Err(e) => g.say(format!("Reload failed: {e}")),
                }
            }
        });
        g.debug.settings = open;
    }
    if g.debug.log {
        let mut open = true;
        egui::Window::new("Story log (L)").open(&mut open).default_width(520.0).default_height(400.0).show(&ctx, |ui| {
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                let log = g.sim.world.res::<SimLog>();
                for e in log.entries.iter().filter(|e| e.kind != LogKind::Debug) {
                    let c = sim::time::Clock { tick: e.tick };
                    ui.label(RichText::new(format!("[{}] {}", c.label(), e.text)).color(kind_color(e.kind)));
                }
            });
        });
        g.debug.log = open;
    }
}
