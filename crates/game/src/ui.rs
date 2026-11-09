//! egui interface: top bar, household, peon details, catalogs, menus, notifications and debug windows.

use egui::{Align2, Color32, FontId, RichText, Ui};
use hearth_render::Camera;
use sim::career::PendingChoices;
use sim::components::*;
use sim::data::{Category, LifeStage, Need, Skill};
use sim::economy::Household;
use sim::log::{LogKind, SimLog};
use sim::objects::ObjectInstance;
use sim::social::Relationships;
use sim::time::{FreeWill, Settings};

use crate::scene::{Label, LabelKind, WallMode};
use crate::state::*;

pub struct UiCtx<'a> {
    pub camera: &'a Camera,
    pub labels: &'a [Label],
    pub render_stats: hearth_render::RenderStats,
    pub screen: (f32, f32),
}

fn need_color(v: f32) -> Color32 {
    if v > 60.0 {
        Color32::from_rgb(90, 190, 90)
    } else if v > 30.0 {
        Color32::from_rgb(220, 190, 60)
    } else {
        Color32::from_rgb(220, 70, 60)
    }
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

pub fn draw(ui: &mut Ui, g: &mut Game, cx: &UiCtx) {
    top_bar(ui, g);
    if let Some(c) = g.creator.as_mut() {
        match crate::creator::window(ui, c) {
            crate::creator::CreatorAction::MoveIn => {
                let c = g.creator.take().unwrap();
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
        labels(ui, cx);
        toasts(ui, g, cx);
        return;
    }
    if g.show_neighborhood {
        neighborhood_window(ui, g);
    }
    household_panel(ui, g);
    match g.mode {
        Mode::Live => {
            if let Some(sel) = g.selected {
                peon_panel(ui, g, sel);
            }
        }
        Mode::Buy => buy_panel(ui, g),
        Mode::Build => build_panel(ui, g),
    }
    labels(ui, cx);
    pie_menu(ui, g);
    career_events(ui, g);
    toasts(ui, g, cx);
    debug_windows(ui, g, cx);
}

fn top_bar(ui: &mut Ui, g: &mut Game) {
    egui::Panel::top("top").show(ui, |ui| {
        ui.horizontal(|ui| {
            let hh = g.sim.world.res::<Household>().clone();
            ui.label(RichText::new(&hh.name).strong());
            let here = {
                let hood = g.sim.world.res::<sim::neighborhood::Neighborhood>();
                hood.lots[hood.active].title()
            };
            let home = sim::neighborhood::at_home(&g.sim.world);
            ui.label(RichText::new(if home { here } else { format!("out at {here}") }).weak());
            if !home && ui.small_button("🏠 Go Home").clicked() {
                match g.sim.go_home() {
                    Ok(()) => g.check_lot_change(),
                    Err(e) => g.say(e),
                }
            }
            ui.separator();
            ui.label(RichText::new(format!("${}", hh.funds)).color(if hh.funds < 0 { Color32::RED } else { Color32::from_rgb(120, 220, 120) }).strong());
            ui.separator();
            ui.label(RichText::new(g.sim.clock().label()).monospace());
            ui.separator();
            if ui.selectable_label(g.paused, "⏸").on_hover_text("Pause (Space)").clicked() {
                g.paused = !g.paused;
            }
            for (i, (_, name)) in SPEEDS.iter().enumerate() {
                if ui.selectable_label(!g.paused && g.speed == i, *name).on_hover_text(format!("Speed ({})", i + 1)).clicked() {
                    g.speed = i;
                    g.paused = false;
                }
            }
            ui.separator();
            for (m, name, key) in [(Mode::Live, "Live", "F5"), (Mode::Buy, "Buy", "F6"), (Mode::Build, "Build", "F7")] {
                let allowed = m == Mode::Live || g.can_build();
                let hover = if allowed { key.to_string() } else { "Only on your own lot".into() };
                if ui.add_enabled(allowed, egui::Button::selectable(g.mode == m, name)).on_hover_text(hover).on_disabled_hover_text("Only on your own lot").clicked() {
                    g.mode = m;
                    g.tool = Tool::None;
                    g.menu = None;
                }
            }
            ui.separator();
            ui.menu_button("☰ Game", |ui| {
                if ui.button("Save (Ctrl+S)").clicked() {
                    g.save_to(std::path::Path::new(QUICKSAVE));
                    ui.close();
                }
                let has_save = std::path::Path::new(QUICKSAVE).exists();
                if ui.add_enabled(has_save, egui::Button::new("Load (Ctrl+O)")).clicked() {
                    g.load_from(std::path::Path::new(QUICKSAVE));
                    ui.close();
                }
                ui.separator();
                if ui.button("New household…").clicked() {
                    g.open_creator();
                    ui.close();
                }
                if ui.button("Neighborhood…").clicked() {
                    g.show_neighborhood = true;
                    ui.close();
                }
                ui.separator();
                ui.checkbox(&mut g.show_roofs, "Show roofs (T)");
                ui.checkbox(&mut g.mute, "Mute voices (M)");
            });
            ui.separator();
            if ui.button("⬇").on_hover_text("Level down (PgDn)").clicked() {
                g.set_level(g.level - 1);
            }
            ui.label(match g.level {
                l if l < 0 => format!("Basement {}", -l),
                0 => "Ground".into(),
                l => format!("Floor {}", l + 1),
            });
            if ui.button("⬆").on_hover_text("Level up (PgUp)").clicked() {
                g.set_level(g.level + 1);
            }
            ui.separator();
            let label = match g.wall_mode {
                WallMode::Up => "Walls: Up",
                WallMode::Cutaway => "Walls: Cutaway",
                WallMode::Down => "Walls: Down",
            };
            if ui.button(label).on_hover_text("Cycle wall mode (V)").clicked() {
                g.wall_mode = match g.wall_mode {
                    WallMode::Up => WallMode::Cutaway,
                    WallMode::Cutaway => WallMode::Down,
                    WallMode::Down => WallMode::Up,
                };
            }
            if g.mode != Mode::Live {
                ui.label(RichText::new("(time paused while building)").weak());
            }
            if let Some((msg, t)) = &g.status
                && g.time - t < 4.0 {
                    ui.separator();
                    ui.label(RichText::new(msg).color(Color32::YELLOW));
                }
        });
    });
}

fn household_panel(ui: &mut Ui, g: &mut Game) {
    egui::Panel::left("household").resizable(false).exact_size(150.0).show(ui, |ui| {
        ui.heading("Household");
        for e in g.sim.peons() {
            let w = &g.sim.world;
            if !w.has::<Member>(e) {
                continue;
            }
            let Some(p) = w.get::<Peon>(e).map(|p| p.clone()) else { continue };
            let dead = w.has::<Dead>(e);
            let away = w.has::<AtWork>(e);
            let stage = w.get::<Age>(e).map(|a| a.stage).unwrap_or(LifeStage::Adult);
            let mood = w.get::<Mood>(e).map(|m| m.value).unwrap_or(0.0);
            let mut text = format!("{}\n{}", p.first, stage.label());
            if dead {
                text = format!("✝ {}\n(deceased)", p.first);
            } else if away {
                let school = w.get::<AtWork>(e).is_some_and(|a| a.school);
                text.push_str(if school { " · at school" } else { " · at work" });
            } else if w.has::<Pregnant>(e) {
                text.push_str(" · 🍼");
            }
            let selected = g.selected == Some(e);
            let resp = ui.add_enabled(!dead, egui::Button::selectable(selected, text).min_size(egui::vec2(135.0, 36.0)));
            if !dead {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(135.0, 4.0), egui::Sense::hover());
                let frac = ((mood + 100.0) / 200.0).clamp(0.0, 1.0);
                ui.painter().rect_filled(rect, 1.0, Color32::from_gray(50));
                let mut fill = rect;
                fill.set_width(rect.width() * frac);
                ui.painter().rect_filled(fill, 1.0, need_color(frac * 100.0));
            }
            if resp.clicked() {
                if selected {
                    g.focus_selected();
                }
                g.selected = Some(e);
                g.menu = None;
            }
            ui.add_space(4.0);
        }
        ui.separator();
        ui.menu_button("Adopt…", |ui| {
            ui.label(RichText::new(format!("Agency fee ${}", sim::family::ADOPTION_FEE)).small());
            for s in [LifeStage::Infant, LifeStage::Toddler, LifeStage::Child, LifeStage::Teen] {
                if ui.button(s.label()).clicked() {
                    match g.sim.adopt(s) {
                        Ok(kid) => {
                            let name = g.sim.name(kid);
                            g.say(format!("{name} joined the family!"));
                        }
                        Err(e) => g.say(e),
                    }
                    ui.close();
                }
            }
        });
    });
}

/// What the player asked for in the neighborhood window.
enum HoodAction {
    Play(usize),
    GoOut(usize),
    GoHome,
    Move(usize),
    NewHousehold,
    Invite(sim::Entity),
}

fn neighborhood_window(ui: &mut Ui, g: &mut Game) {
    use sim::neighborhood::{self as nh, LotKind};
    let mut open = true;
    let mut action = None;
    egui::Window::new("Neighborhood").open(&mut open).default_width(460.0).show(ui.ctx(), |ui| {
        let w = &g.sim.world;
        let hood = w.res::<nh::Neighborhood>().clone();
        let hh = w.res::<Household>().clone();
        ui.label(RichText::new(&hood.name).strong());
        ui.label(RichText::new("Play any household on the street, move house, or head out to the park. Everyone shares one clock: life goes on next door while you're away.").small().weak());
        ui.separator();
        egui::Grid::new("lots").num_columns(3).striped(true).spacing([10.0, 6.0]).show(ui, |ui| {
            for (i, lot) in hood.lots.iter().enumerate() {
                let here = i == hood.active;
                let mut title = RichText::new(lot.title());
                if here {
                    title = title.strong();
                }
                ui.label(title);
                let residents = nh::residents(w, i);
                let names = |r: &nh::Residents| {
                    let firsts: Vec<String> = r.members.iter().filter(|e| !w.has::<Dead>(**e)).filter_map(|e| w.get::<Peon>(*e).map(|p| p.first.clone())).collect();
                    format!("{} ({})", r.name, firsts.join(", "))
                };
                match (lot.kind, &residents) {
                    (LotKind::Community, _) => {
                        ui.label(RichText::new("community lot").italics());
                        ui.horizontal(|ui| {
                            if here {
                                if ui.button("Go home").clicked() {
                                    action = Some(HoodAction::GoHome);
                                }
                            } else if ui.button("Go out").on_hover_text("Take the household here for a few hours").clicked() {
                                action = Some(HoodAction::GoOut(i));
                            }
                        });
                    }
                    (LotKind::Residential, Some(r)) => {
                        ui.label(names(r));
                        ui.horizontal(|ui| {
                            if r.played {
                                ui.label(RichText::new(if here { "you are here" } else { "your home" }).weak());
                            } else if ui.button("Play").on_hover_text(format!("${} in the bank", r.funds)).clicked() {
                                action = Some(HoodAction::Play(i));
                            }
                        });
                    }
                    (LotKind::Residential, None) => {
                        let value = nh::build_value(w, i);
                        let price = nh::price(w, i);
                        ui.label(RichText::new(if value > 0 { format!("for sale: furnished house, ${price}") } else { format!("for sale: empty plot, ${price}") }).color(Color32::from_rgb(160, 210, 160)));
                        ui.horizontal(|ui| {
                            let sale = hh.home.map(|h| nh::price(w, h)).unwrap_or(0);
                            let afford = hh.funds + sale >= price;
                            if ui.add_enabled(afford, egui::Button::new("Move here")).on_hover_text(format!("Sell your place for ${sale} and buy this for ${price}")).clicked() {
                                action = Some(HoodAction::Move(i));
                            }
                            if ui.button("New household…").clicked() {
                                action = Some(HoodAction::NewHousehold);
                            }
                        });
                    }
                }
                ui.end_row();
            }
        });
        ui.separator();
        ui.label(RichText::new("Neighbors").strong());
        let members: Vec<sim::Entity> = g.sim.living_peons().into_iter().filter(|m| w.has::<Member>(*m)).collect();
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
            ui.horizontal(|ui| {
                ui.label(format!("{p} — {address} ({status})"));
                ui.label(RichText::new(rel).weak());
                let grown = w.get::<Age>(e).is_some_and(|a| a.stage >= LifeStage::Teen);
                if w.has::<Offlot>(e) && grown && !w.has::<Dead>(e) && ui.small_button("Invite over").clicked() {
                    action = Some(HoodAction::Invite(e));
                }
            });
        }
    });
    if !open {
        g.show_neighborhood = false;
    }
    let Some(action) = action else { return };
    let result = match action {
        HoodAction::Play(l) => g.sim.play_household(l).map(|_| "Switched households."),
        HoodAction::GoOut(l) => g.sim.go_out(l).map(|_| "Off they go!"),
        HoodAction::GoHome => g.sim.go_home().map(|_| "Home sweet home."),
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

fn peon_panel(ui: &mut Ui, g: &mut Game, e: sim::Entity) {
    let data = g.sim.data();
    egui::Panel::right("peon").resizable(false).exact_size(300.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let w = &g.sim.world;
            let Some(p) = w.get::<Peon>(e).map(|p| p.clone()) else { return };
            let age = w.get::<Age>(e).unwrap().clone();
            let needs = w.get::<Needs>(e).unwrap().clone();
            let mood = w.get::<Mood>(e).unwrap().clone();
            let pers = w.get::<Personality>(e).unwrap().clone();
            let skills = w.get::<Skills>(e).unwrap().clone();
            let job = w.get::<Job>(e).map(|j| j.clone());
            let queue = w.get::<ActionQueue>(e).unwrap().clone();
            let brain_fw = w.get::<Brain>(e).and_then(|b| b.free_will);
            ui.heading(p.full_name());
            ui.label(format!("{} · day {:.1} of {:.0} in stage", age.stage.label(), age.days_in_stage, age.stage_len));
            if let Some(a) = w.get::<AtWork>(e) {
                ui.label(RichText::new(if a.school { "At school" } else { "At work" }).italics());
            }
            if let Some(pg) = w.get::<Pregnant>(e) {
                let now = g.sim.clock().tick;
                let other = w.get::<Peon>(pg.other).map(|p| p.first.clone()).unwrap_or_else(|| "someone".into());
                ui.label(RichText::new(format!("🍼 Expecting with {other} — due in {:.0}h", pg.due.saturating_sub(now) as f32 / 60.0)).color(Color32::from_rgb(250, 190, 220)));
            }
            if let Some(st) = w.get::<Student>(e) {
                ui.label(format!("School grade: {} {}", st.letter(), if st.homework_done { "· homework done ✔" } else { "· homework due" }));
            }
            if let Some(gn) = w.get::<Genes>(e) {
                let c = |x: [f32; 3]| Color32::from_rgb((x[0] * 255.0) as u8, (x[1] * 255.0) as u8, (x[2] * 255.0) as u8);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Genes:").small());
                    for (name, col) in [("skin", gn.skin), ("hair", gn.hair), ("eyes", gn.eyes)] {
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, c(col));
                        resp.on_hover_text(name);
                    }
                    ui.label(RichText::new(format!("height {:.2} · build {:.2}", gn.height, gn.build)).small());
                });
            }
            if let Some(f) = w.get::<Family>(e) {
                let names = |v: &[sim::Entity]| v.iter().filter_map(|x| w.get::<Peon>(*x).map(|p| p.first.clone())).collect::<Vec<_>>().join(", ");
                if !f.parents.is_empty() {
                    ui.label(RichText::new(format!("Parents: {}", names(&f.parents))).small());
                }
                if !f.children.is_empty() {
                    ui.label(RichText::new(format!("Children: {}", names(&f.children))).small());
                }
            }
            ui.horizontal(|ui| {
                ui.label("Mood");
                let frac = ((mood.value + 100.0) / 200.0).clamp(0.0, 1.0);
                ui.add(egui::ProgressBar::new(frac).fill(need_color(frac * 100.0)).text(format!("{:+.0}", mood.value)).desired_width(200.0));
            });

            ui.separator();
            ui.label(RichText::new("Needs").strong());
            for n in needs.active_needs() {
                let v = needs.get(n);
                ui.horizontal(|ui| {
                    let label = if n.is_core() { RichText::new(n.label()) } else { RichText::new(format!("{} ✨", n.label())).italics() };
                    ui.add_sized([90.0, 16.0], egui::Label::new(label));
                    ui.add(egui::ProgressBar::new(v / 100.0).fill(need_color(v)).desired_width(170.0).desired_height(12.0));
                });
            }

            ui.separator();
            ui.label(RichText::new("Doing").strong());
            if let Some(c) = &queue.current {
                ui.horizontal(|ui| {
                    ui.label(format!("▶ {}", c.action.label));
                    if c.action.source != Source::System && ui.small_button("x").clicked() {
                        g.sim.cancel(e, c.action.id);
                    }
                });
            } else {
                ui.label(RichText::new("(nothing)").weak());
            }
            for a in &queue.queue {
                ui.horizontal(|ui| {
                    ui.label(format!("· {}", a.label));
                    if ui.small_button("x").clicked() {
                        g.sim.cancel(e, a.id);
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.label("Free will:");
                for (fw, name) in [(None, "Default"), (Some(FreeWill::Off), "Off"), (Some(FreeWill::Low), "Low"), (Some(FreeWill::Full), "Full")] {
                    if ui.selectable_label(brain_fw == fw, name).clicked()
                        && let Some(mut b) = g.sim.world.get_mut::<Brain>(e) {
                            b.free_will = fw;
                        }
                }
            });

            if !mood.moodlets.is_empty() {
                ui.separator();
                ui.label(RichText::new("Moodlets").strong());
                let now = g.sim.clock().tick;
                for m in &mood.moodlets {
                    let col = if m.value >= 0.0 { Color32::from_rgb(130, 220, 130) } else { Color32::from_rgb(240, 120, 110) };
                    ui.label(RichText::new(format!("{:+.0} {} ({}h)", m.value, m.name, m.until.saturating_sub(now).div_ceil(60))).color(col));
                }
            }

            ui.separator();
            ui.label(RichText::new("Personality").strong());
            let traits: Vec<String> = pers.traits.iter().map(|t| data.trait_def(t).map(|d| d.name.clone()).unwrap_or(t.clone())).collect();
            ui.label(format!("Traits: {}", traits.join(", ")));
            for t in &pers.traits {
                if let Some(d) = data.trait_def(t) {
                    ui.label(RichText::new(format!("  {}: {}", d.name, d.desc)).small().weak());
                }
            }
            if let Some(q) = data.quirk(&pers.quirk) {
                ui.label(format!("Quirk: {}", q.name)).on_hover_text(&q.desc);
            }
            let a = pers.axes;
            ui.label(RichText::new(format!("Neat {} · Outgoing {} · Active {} · Playful {} · Nice {}", a.neat, a.outgoing, a.active, a.playful, a.nice)).small());

            ui.separator();
            ui.label(RichText::new("Skills").strong());
            ui.horizontal_wrapped(|ui| {
                for s in Skill::ALL {
                    let v = skills.get(s);
                    if v >= 0.05 {
                        ui.label(format!("{:?} {:.1}", s, v));
                    }
                }
            });

            ui.separator();
            ui.label(RichText::new("Career").strong());
            match &job {
                Some(j) => {
                    if let Some(c) = data.career(&j.career) {
                        let lvl = &c.levels[j.level];
                        ui.label(format!("{} — {} (level {})", c.name, lvl.title, j.level + 1));
                        let days: Vec<&str> = lvl.days.iter().map(|d| ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"][*d as usize]).collect();
                        ui.label(RichText::new(format!("${}/h · {:02}:00–{:02}:00 · {}", lvl.pay, lvl.start_hour, (lvl.start_hour + lvl.hours) % 24, days.join(" "))).small());
                        ui.add(egui::ProgressBar::new(((j.performance + 100.0) / 200.0).clamp(0.0, 1.0)).text(format!("performance {:.0}", j.performance)));
                        if let Some(next) = c.levels.get(j.level + 1) {
                            let reqs: Vec<String> = next.skills.iter().map(|(s, v)| format!("{:?} {:.0}", s, v)).collect();
                            if !reqs.is_empty() {
                                ui.label(RichText::new(format!("Next: {} (needs {})", next.title, reqs.join(", "))).small().weak());
                            }
                        }
                    }
                    if ui.small_button("Quit job").clicked() {
                        g.sim.world.remove::<Job>(e);
                    }
                }
                None => {
                    if age.stage >= LifeStage::Teen {
                        egui::ComboBox::from_id_salt("find_job").selected_text("Find a job…").show_ui(ui, |ui| {
                            for c in &data.careers {
                                if age.stage >= c.min_stage && ui.selectable_label(false, format!("{} — {}", c.name, c.levels[0].title)).on_hover_text(&c.desc).clicked() {
                                    let _ = sim::career::join_career(&mut g.sim.world, e, &c.id);
                                }
                            }
                        });
                    } else {
                        ui.label(RichText::new("Too young to work. Lucky.").weak());
                    }
                }
            }

            ui.separator();
            ui.label(RichText::new("Relationships").strong());
            let rels = g.sim.world.res::<Relationships>().of(e);
            for (other, r) in rels {
                let Some(name) = g.sim.world.get::<Peon>(other).map(|p| p.first.clone()) else { continue };
                ui.label(format!("{name}: {} (friend {:.0}, romance {:.0})", r.label(), r.friendship, r.romance));
            }
        });
    });
}

fn buy_panel(ui: &mut Ui, g: &mut Game) {
    let data = g.sim.data();
    egui::Panel::right("buy").resizable(false).exact_size(300.0).show(ui, |ui| {
        ui.heading("Buy");
        ui.label(RichText::new("Click to place · R rotates · Esc cancels").small().weak());
        if ui.selectable_label(g.tool == Tool::Sell, "💲 Sell tool (click an object)").clicked() {
            g.tool = Tool::Sell;
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            let cats = [
                Category::Seating,
                Category::Sleeping,
                Category::Surfaces,
                Category::Kitchen,
                Category::Bathroom,
                Category::Electronics,
                Category::Fun,
                Category::Kids,
                Category::Decor,
                Category::Lighting,
                Category::Misc,
            ];
            for cat in cats {
                let items: Vec<_> = data.objects.iter().filter(|o| o.category == cat).collect();
                if items.is_empty() {
                    continue;
                }
                egui::CollapsingHeader::new(format!("{cat:?}")).default_open(true).show(ui, |ui| {
                    for o in items {
                        let sel = matches!(&g.tool, Tool::Place { def, .. } if *def == o.id);
                        let funds = g.sim.world.res::<Household>().funds;
                        let txt = format!("{} — ${}", o.name, o.price);
                        let mut hover = o.desc.clone();
                        let ints: Vec<&str> = o.interactions.iter().map(|i| i.name.as_str()).collect();
                        if !ints.is_empty() {
                            hover = format!("{hover}\n\nCan: {}", ints.join(", "));
                        }
                        let resp = ui.add_enabled(funds >= o.price, egui::Button::selectable(sel, txt));
                        let resp = if hover.trim().is_empty() { resp } else { resp.on_hover_text(hover) };
                        if resp.clicked() {
                            g.tool = Tool::Place { def: o.id.clone(), rot: 0 };
                        }
                    }
                });
            }
        });
    });
}

fn build_panel(ui: &mut Ui, g: &mut Game) {
    egui::Panel::right("build").resizable(false).exact_size(300.0).show(ui, |ui| {
        ui.heading("Build");
        ui.label(RichText::new("Drag to draw · R rotates stairs · PgUp/PgDn change level · Esc cancels").small().weak());
        ui.separator();
        let tools = [
            (Tool::Room, format!("Room (walls ${COST_WALL}/m + floor ${COST_FLOOR}/tile)")),
            (Tool::Wall, format!("Wall (${COST_WALL}/m)")),
            (Tool::DeleteWall, "Delete walls (50% refund)".to_string()),
            (Tool::Door, format!("Door (${COST_DOOR})")),
            (Tool::Window, format!("Window (${COST_WINDOW})")),
            (Tool::Floor, format!("Floor (${COST_FLOOR}/tile)")),
            (Tool::DeleteFloor, "Delete floor".to_string()),
            (Tool::Stairs { dir: sim::lot::Dir4::PosZ }, format!("Stairs (${COST_STAIRS})")),
            (Tool::Basement, format!("Dig basement (${COST_DIG}/tile, basement levels only)")),
            (Tool::Roof { kind: sim::lot::RoofKind::Gable }, format!("Roof (${COST_ROOF_TILE}/tile, on the level you're viewing)")),
            (Tool::DeleteRoof, "Delete roof".to_string()),
            (Tool::Terrain { brush: sim::lot::TerrainBrush::Raise, radius: 2.0 }, format!("Terrain (${COST_TERRAIN}/corner, hold to sculpt)")),
        ];
        for (t, label) in tools {
            let sel = std::mem::discriminant(&g.tool) == std::mem::discriminant(&t);
            if ui.selectable_label(sel, label).clicked() {
                g.tool = t;
                g.drag_start = None;
            }
        }
        match &mut g.tool {
            Tool::Roof { kind } => {
                ui.horizontal(|ui| {
                    for (k, name) in [(sim::lot::RoofKind::Gable, "Gable"), (sim::lot::RoofKind::Hip, "Hip"), (sim::lot::RoofKind::Flat, "Flat")] {
                        if ui.selectable_label(*kind == k, name).clicked() {
                            *kind = k;
                        }
                    }
                });
                ui.add(egui::Slider::new(&mut g.roof_pitch, 0.2..=1.0).text("pitch"));
            }
            Tool::Terrain { brush, radius } => {
                ui.horizontal(|ui| {
                    use sim::lot::TerrainBrush as B;
                    for (b, name) in [(B::Raise, "Raise"), (B::Lower, "Lower"), (B::Flatten, "Flatten"), (B::Smooth, "Smooth")] {
                        if ui.selectable_label(*brush == b, name).clicked() {
                            *brush = b;
                        }
                    }
                });
                ui.add(egui::Slider::new(radius, 0.5..=6.0).text("radius"));
                ui.label(RichText::new("Ground under floors, walls and furniture stays put.").small().weak());
            }
            _ => {}
        }
        ui.horizontal(|ui| {
            for (k, name) in [(sim::lot::RoofKind::Gable, "Auto-roof: gable"), (sim::lot::RoofKind::Hip, "hip"), (sim::lot::RoofKind::Flat, "flat")] {
                if ui.button(name).on_hover_text("Replace all roofs with automatic ones over the top floors").clicked() {
                    let pitch = g.roof_pitch;
                    let n = g.sim.lot_mut().auto_roof(k, pitch);
                    g.say(format!("{n} roof(s) built."));
                }
            }
        });
        ui.separator();
        ui.label("Wall covering");
        ui.horizontal(|ui| {
            for s in 0..5u16 {
                let c = crate::scene::wall_color(s);
                let col = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
                let b = egui::Button::new("   ").fill(col).stroke(if g.wall_style == s { egui::Stroke::new(2.0, Color32::WHITE) } else { egui::Stroke::NONE });
                if ui.add(b).clicked() {
                    g.wall_style = s;
                }
            }
        });
        ui.label("Floor");
        ui.horizontal(|ui| {
            for (s, name) in [(0u16, "Wood"), (1, "Tile"), (2, "Carpet"), (3, "Concrete")] {
                if ui.selectable_label(g.floor_style == s, name).clicked() {
                    g.floor_style = s;
                }
            }
        });
        ui.separator();
        let lot = g.sim.world.res::<sim::lot::Lot>();
        ui.label(RichText::new(format!("Walls {} · floor tiles {} · stairs {} · lot value ${}", lot.walls.len(), lot.floors.len(), lot.stairs.len(), sim::economy::lot_value(&g.sim.world))).small());
    });
}

fn labels(ui: &mut Ui, cx: &UiCtx) {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("labels")));
    let ppp = ui.ctx().pixels_per_point();
    for l in cx.labels {
        let Some((x, y)) = cx.camera.to_screen(l.world, cx.screen.0, cx.screen.1) else { continue };
        let pos = egui::pos2(x / ppp, y / ppp);
        let (bg, fg, size) = match l.kind {
            LabelKind::Speech => (Color32::from_rgba_unmultiplied(255, 255, 255, 230), Color32::from_rgb(30, 30, 40), 13.0),
            LabelKind::Thought => (Color32::from_rgba_unmultiplied(230, 240, 255, 210), Color32::from_rgb(40, 50, 90), 12.0),
            LabelKind::Name => (Color32::from_rgba_unmultiplied(20, 20, 30, 200), Color32::WHITE, 12.0),
            LabelKind::Alert => (Color32::from_rgba_unmultiplied(200, 50, 40, 220), Color32::WHITE, 12.0),
        };
        let galley = painter.layout_no_wrap(l.text.clone(), FontId::proportional(size), fg);
        let rect = Align2::CENTER_BOTTOM.anchor_size(pos, galley.size()).expand2(egui::vec2(6.0, 3.0));
        painter.rect_filled(rect, 6.0, bg);
        if l.kind == LabelKind::Speech {
            // little tail
            let tip = egui::pos2(pos.x, pos.y + 8.0);
            painter.add(egui::Shape::convex_polygon(vec![egui::pos2(pos.x - 5.0, rect.bottom()), egui::pos2(pos.x + 5.0, rect.bottom()), tip], bg, egui::Stroke::NONE));
        }
        painter.galley(rect.min + egui::vec2(6.0, 3.0), galley, fg);
    }
}

fn pie_menu(ui: &mut Ui, g: &mut Game) {
    let Some(menu) = g.menu.clone() else { return };
    let ppp = ui.ctx().pixels_per_point();
    let mut close = false;
    egui::Area::new(egui::Id::new("pie"))
        .fixed_pos(egui::pos2(menu.screen.0 / ppp, menu.screen.1 / ppp))
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(&menu.title).strong());
                ui.separator();
                if menu.options.is_empty() {
                    ui.label(RichText::new("Nothing to do here.").weak());
                }
                for (label, kind, ok) in &menu.options {
                    let resp = ui.add_enabled(ok.is_ok(), egui::Button::new(label));
                    let resp = match ok {
                        Err(why) => resp.on_disabled_hover_text(why),
                        Ok(()) => resp,
                    };
                    if resp.clicked() {
                        match g.sim.order(menu.peon, kind.clone()) {
                            Ok(()) => {}
                            Err(why) => g.say(why),
                        }
                        close = true;
                    }
                }
                if ui.small_button("Cancel").clicked() {
                    close = true;
                }
            });
        });
    if close {
        g.menu = None;
    }
}

fn career_events(ui: &mut Ui, g: &mut Game) {
    let items = g.sim.world.res::<PendingChoices>().items.clone();
    let Some(item) = items.first().cloned() else { return };
    let name = g.sim.name(item.peon);
    let career = g.sim.data().career(&item.career).map(|c| c.name.clone()).unwrap_or_default();
    egui::Window::new(format!("{name} at work ({career})"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, -80.0))
        .show(ui.ctx(), |ui| {
            ui.set_max_width(420.0);
            ui.label(RichText::new(&item.text).size(15.0));
            ui.add_space(8.0);
            for (i, c) in item.choices.iter().enumerate() {
                if ui.button(c).clicked() {
                    g.sim.resolve_choice(0, i);
                }
            }
            ui.add_space(4.0);
            ui.label(RichText::new("(If you ignore this, they'll decide on their own when the shift ends.)").small().weak());
        });
}

fn toasts(ui: &mut Ui, g: &mut Game, _cx: &UiCtx) {
    egui::Area::new(egui::Id::new("toasts")).anchor(Align2::LEFT_BOTTOM, egui::vec2(160.0, -10.0)).show(ui.ctx(), |ui| {
        ui.set_max_width(460.0);
        for t in &g.toasts {
            let age = g.time - t.born;
            let alpha = ((9.0 - age) / 2.0).clamp(0.0, 1.0);
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(20, 22, 30, (200.0 * alpha) as u8))
                .corner_radius(6.0)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    let c = kind_color(t.kind);
                    ui.label(RichText::new(&t.text).color(Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (255.0 * alpha) as u8)));
                });
            ui.add_space(3.0);
        }
    });
}

fn debug_windows(ui: &mut Ui, g: &mut Game, cx: &UiCtx) {
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
