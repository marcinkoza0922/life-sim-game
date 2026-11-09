//! Create-a-Household: edit names, life stages, genes/looks, personality and careers, then move
//! the family into a house. The line-up preview draws the rigged bodies from [`crate::peons`]
//! (looks, height, build and hair style all apply live).

use std::sync::Arc;

use egui::{Color32, RichText, Ui};
use glam::Vec3;
use sim::components::{Appearance, Axes, Genes};
use sim::data::{GameData, LifeStage};
use sim::genetics::{self, EYE_COLORS};
use sim::lot::Lot;
use sim::{Entity, PeonSpec, Sim};

pub const SKIN_TONES: [[f32; 3]; 7] = [
    [0.98, 0.84, 0.74],
    [0.96, 0.8, 0.69],
    [0.87, 0.67, 0.5],
    [0.72, 0.5, 0.36],
    [0.55, 0.37, 0.25],
    [0.42, 0.28, 0.19],
    [0.3, 0.2, 0.14],
];
pub const HAIR_COLORS: [[f32; 3]; 8] = [
    [0.1, 0.07, 0.05],
    [0.35, 0.22, 0.1],
    [0.55, 0.38, 0.2],
    [0.75, 0.6, 0.3],
    [0.6, 0.25, 0.1],
    [0.85, 0.85, 0.85],
    [0.3, 0.6, 0.55],
    [0.75, 0.35, 0.6],
];
pub const STAGES: [LifeStage; 7] = LifeStage::ALL;
pub const AXIS_BUDGET: u32 = 25;
pub const START_FUNDS: i64 = 20_000;

/// How many traits a peon of this stage starts with (more come with birthdays).
pub fn max_traits(stage: LifeStage) -> usize {
    if stage <= LifeStage::Toddler {
        1
    } else if stage <= LifeStage::Teen {
        2
    } else {
        3
    }
}

pub struct Creator {
    pub family_name: String,
    pub members: Vec<PeonSpec>,
    pub editing: usize,
    /// Marry the first two adults on move-in.
    pub couple: bool,
    pub error: Option<String>,
    scratch: Sim,
    data: Arc<GameData>,
}

impl Creator {
    pub fn new(data: Arc<GameData>, seed: u64) -> Creator {
        let mut scratch = Sim::new(seed, (*data).clone());
        let first = scratch.random_spec(LifeStage::YoungAdult, None);
        let family_name = first.last.clone();
        let mut c = Creator { family_name, members: Vec::new(), editing: 0, couple: true, error: None, scratch, data };
        c.push(first);
        c
    }

    fn push(&mut self, mut spec: PeonSpec) {
        spec.last = self.family_name.clone();
        // make the scratch world remember the name so the next random pick avoids it
        let e = self.scratch.spawn_peon(spec.clone(), Vec3::ZERO);
        let _ = e;
        self.members.push(spec);
        self.editing = self.members.len() - 1;
    }

    pub fn add_member(&mut self, stage: LifeStage) {
        if self.members.len() >= sim::family::MAX_HOUSEHOLD {
            self.error = Some(format!("A household can have at most {} members.", sim::family::MAX_HOUSEHOLD));
            return;
        }
        let mut spec = self.scratch.random_spec(stage, Some(&self.family_name));
        // kids added after two adults look like them
        let adults: Vec<Genes> = self.members.iter().filter(|m| m.stage.is_adult()).filter_map(|m| m.genes.clone()).collect();
        if stage < LifeStage::YoungAdult && adults.len() >= 2 {
            let mut rng = sim::rng::SimRng::new(self.members.len() as u64 * 7919 + spec.first.len() as u64);
            let g = genetics::inherit(&mut rng, &adults[0], &adults[1]);
            spec.appearance = genetics::express(&g, stage, spec.appearance.shirt, spec.appearance.pants, spec.appearance.hair_style);
            spec.genes = Some(g);
        }
        self.push(spec);
    }

    pub fn randomize(&mut self, i: usize) {
        let stage = self.members[i].stage;
        let mut spec = self.scratch.random_spec(stage, Some(&self.family_name));
        spec.first = self.members[i].first.clone();
        self.members[i] = spec;
    }

    pub fn remove(&mut self, i: usize) {
        if self.members.len() > 1 {
            self.members.remove(i);
            self.editing = self.editing.min(self.members.len() - 1);
        }
    }

    /// Sets the stage, keeping genes and adjusting stage-dependent bits (traits, career, grey hair).
    pub fn set_stage(&mut self, i: usize, stage: LifeStage) {
        let m = &mut self.members[i];
        m.stage = stage;
        m.traits.truncate(max_traits(stage));
        if !stage.is_adult() || stage == LifeStage::Elder {
            m.career = None;
        }
        refresh_appearance(m);
    }

    pub fn validate(&self) -> Result<(), String> {
        validate(&self.data, &self.family_name, &self.members)
    }

    /// Builds a playable sim with the starter house and this household.
    pub fn move_in(&self, seed: u64) -> Result<Sim, String> {
        self.validate()?;
        Ok(move_in(seed, (*self.data).clone(), &self.family_name, &self.members, self.couple))
    }

    /// Bodies for the preview line-up (stage, appearance).
    pub fn preview(&self) -> Vec<(LifeStage, Appearance, bool)> {
        self.members.iter().enumerate().map(|(i, m)| (m.stage, m.appearance.clone(), i == self.editing)).collect()
    }
}

/// Recomputes the rendered look from genes (hair goes grey for elders).
pub fn refresh_appearance(m: &mut PeonSpec) {
    let g = m.genes.clone().unwrap_or_else(|| genetics::genes_from_appearance(&m.appearance));
    m.appearance = genetics::express(&g, m.stage, m.appearance.shirt, m.appearance.pants, m.appearance.hair_style);
    m.genes = Some(g);
}

pub fn validate(data: &GameData, family: &str, members: &[PeonSpec]) -> Result<(), String> {
    if family.trim().is_empty() {
        return Err("The family needs a last name.".into());
    }
    if members.is_empty() {
        return Err("Add at least one person.".into());
    }
    if members.len() > sim::family::MAX_HOUSEHOLD {
        return Err(format!("At most {} people per household.", sim::family::MAX_HOUSEHOLD));
    }
    if !members.iter().any(|m| m.stage.is_adult()) {
        return Err("Someone has to pay the bills: add a young adult, adult or elder.".into());
    }
    let mut names = std::collections::HashSet::new();
    for m in members {
        if m.first.trim().is_empty() {
            return Err("Everyone needs a first name.".into());
        }
        if !names.insert(m.first.trim().to_lowercase()) {
            return Err(format!("Two people are called {}. That will get confusing at dinner.", m.first));
        }
        if m.traits.len() > max_traits(m.stage) {
            return Err(format!("{} has too many traits for a {}.", m.first, m.stage.label()));
        }
        for t in &m.traits {
            let Some(def) = data.trait_def(t) else { return Err(format!("unknown trait {t}")) };
            if m.traits.iter().any(|o| def.conflicts.contains(o)) {
                return Err(format!("{} can't be both {} and something it conflicts with.", m.first, def.name));
            }
        }
        let a = m.axes;
        let sum = a.neat as u32 + a.outgoing as u32 + a.active as u32 + a.playful as u32 + a.nice as u32;
        if sum > AXIS_BUDGET {
            return Err(format!("{}'s personality uses {sum} of {AXIS_BUDGET} points.", m.first));
        }
        if data.quirk(&m.quirk).is_none() {
            return Err(format!("{} needs a quirk.", m.first));
        }
    }
    Ok(())
}

/// A starter-house sim with the given household moved in.
pub fn move_in(seed: u64, data: GameData, family: &str, members: &[PeonSpec], couple: bool) -> Sim {
    let mut sim = Sim::new(seed, data);
    sim::starter::build_house(&mut sim);
    let spots = [(11, 16), (13, 16), (12, 15), (14, 15), (10, 15), (15, 16), (11, 17), (13, 17)];
    let mut spawned: Vec<(Entity, LifeStage)> = Vec::new();
    for (i, m) in members.iter().enumerate() {
        let mut spec = m.clone();
        spec.first = spec.first.trim().to_string();
        spec.last = family.trim().to_string();
        let (x, z) = spots[i % spots.len()];
        let e = sim.spawn_peon(spec, Lot::tile_center(0, glam::IVec2::new(x, z)));
        spawned.push((e, m.stage));
    }
    let adults: Vec<Entity> = spawned.iter().filter(|(_, s)| s.is_adult()).map(|(e, _)| *e).collect();
    if couple && adults.len() >= 2 {
        sim.marry(adults[0], adults[1]);
    }
    let parents: Vec<Entity> = adults.iter().take(if couple { 2 } else { 1 }).copied().collect();
    let kids: Vec<Entity> = spawned.iter().filter(|(_, s)| !s.is_adult()).map(|(e, _)| *e).collect();
    for &k in &kids {
        for &p in &parents {
            sim.set_parent(p, k);
        }
    }
    for (i, &a) in kids.iter().enumerate() {
        for &b in &kids[i + 1..] {
            sim.world.res_mut::<sim::social::Relationships>().add(a, b, 30.0, 0.0);
        }
    }
    {
        let mut hh = sim.world.res_mut::<sim::economy::Household>();
        hh.name = format!("The {} Family", family.trim());
        hh.funds = START_FUNDS;
    }
    sim
}

// ------------------------------------------------------------------------------------------------
// UI

fn c32(c: [f32; 3]) -> Color32 {
    Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

fn swatches(ui: &mut Ui, palette: &[[f32; 3]], current: &mut [f32; 3]) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for c in palette {
            let selected = (current[0] - c[0]).abs() + (current[1] - c[1]).abs() + (current[2] - c[2]).abs() < 0.02;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 3.0, c32(*c));
            if selected {
                ui.painter().rect_stroke(rect, 3.0, egui::Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
            }
            if resp.clicked() {
                *current = *c;
                changed = true;
            }
        }
    });
    changed
}

fn color_edit(ui: &mut Ui, label: &str, c: &mut [f32; 3]) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.color_edit_button_rgb(c);
    });
}

/// What the player chose this frame.
pub enum CreatorAction {
    None,
    Cancel,
    MoveIn,
}

pub fn window(ui: &mut Ui, c: &mut Creator) -> CreatorAction {
    let mut action = CreatorAction::None;
    let data = c.data.clone();
    let max_h = ui.ctx().content_rect().height() - 90.0;
    egui::Window::new("Create a Household").default_pos([20.0, 50.0]).default_width(430.0).max_height(max_h).resizable(false).show(ui.ctx(), |ui| {
        ui.horizontal(|ui| {
            ui.label("Family name");
            if ui.text_edit_singleline(&mut c.family_name).changed() {
                let name = c.family_name.clone();
                for m in &mut c.members {
                    m.last = name.clone();
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            for i in 0..c.members.len() {
                let m = &c.members[i];
                let label = format!("{} ({})", if m.first.is_empty() { "?" } else { &m.first }, m.stage.label());
                if ui.selectable_label(c.editing == i, label).clicked() {
                    c.editing = i;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.menu_button("+ Add person", |ui| {
                for s in STAGES {
                    if ui.button(s.label()).clicked() {
                        c.add_member(s);
                        ui.close();
                    }
                }
            });
            if ui.add_enabled(c.members.len() > 1, egui::Button::new("Remove")).clicked() {
                c.remove(c.editing);
            }
            if ui.button("🎲 Randomize").clicked() {
                c.randomize(c.editing);
            }
            ui.checkbox(&mut c.couple, "First two adults are married");
        });
        ui.separator();
        let i = c.editing;
        egui::ScrollArea::vertical().max_height((max_h - 230.0).max(200.0)).show(ui, |ui| {
            let mut new_stage = None;
            {
                let m = &mut c.members[i];
                ui.horizontal(|ui| {
                    ui.label("First name");
                    ui.text_edit_singleline(&mut m.first);
                });
                ui.horizontal(|ui| {
                    ui.label("Life stage");
                    egui::ComboBox::from_id_salt("stage").selected_text(m.stage.label()).show_ui(ui, |ui| {
                        for s in STAGES {
                            if ui.selectable_label(m.stage == s, s.label()).clicked() {
                                new_stage = Some(s);
                            }
                        }
                    });
                });
            }
            if let Some(s) = new_stage {
                c.set_stage(i, s);
            }
            let m = &mut c.members[i];
            egui::CollapsingHeader::new("Looks").default_open(true).show(ui, |ui| {
                let mut g = m.genes.clone().unwrap_or_else(|| genetics::genes_from_appearance(&m.appearance));
                let mut changed = false;
                ui.label("Skin");
                changed |= swatches(ui, &SKIN_TONES, &mut g.skin);
                ui.label("Hair");
                changed |= swatches(ui, &HAIR_COLORS, &mut g.hair);
                ui.label("Eyes");
                changed |= swatches(ui, &EYE_COLORS, &mut g.eyes);
                changed |= ui.add(egui::Slider::new(&mut g.height, 0.9..=1.1).text("height")).changed();
                changed |= ui.add(egui::Slider::new(&mut g.build, 0.0..=1.0).text("build")).changed();
                let mut style = m.appearance.hair_style as u32;
                ui.horizontal(|ui| {
                    ui.label("Hair style");
                    for (k, name) in ["Classic", "Bun", "Long", "Heroic fringe"].iter().enumerate() {
                        if ui.selectable_label(style == k as u32, *name).clicked() {
                            style = k as u32;
                            changed = true;
                        }
                    }
                });
                m.appearance.hair_style = style as u8;
                color_edit(ui, "Shirt", &mut m.appearance.shirt);
                color_edit(ui, "Pants", &mut m.appearance.pants);
                if changed {
                    m.genes = Some(g);
                    refresh_appearance(m);
                }
            });
            egui::CollapsingHeader::new("Personality").default_open(true).show(ui, |ui| {
                let a = &mut m.axes;
                let total = |a: &Axes| a.neat as u32 + a.outgoing as u32 + a.active as u32 + a.playful as u32 + a.nice as u32;
                let used = total(a);
                ui.label(RichText::new(format!("Points: {used} / {AXIS_BUDGET}")).color(if used > AXIS_BUDGET { Color32::RED } else { Color32::LIGHT_GRAY }));
                for (name, v) in [("Neat", &mut a.neat), ("Outgoing", &mut a.outgoing), ("Active", &mut a.active), ("Playful", &mut a.playful), ("Nice", &mut a.nice)] {
                    ui.add(egui::Slider::new(v, 0..=10).text(name));
                }
                let max = max_traits(m.stage);
                ui.label(format!("Traits ({} of {max})", m.traits.len()));
                ui.horizontal_wrapped(|ui| {
                    for t in &data.traits {
                        let has = m.traits.contains(&t.id);
                        let conflict = m.traits.iter().any(|o| t.conflicts.contains(o) || data.trait_def(o).is_some_and(|d| d.conflicts.contains(&t.id)));
                        let enabled = has || (!conflict && m.traits.len() < max);
                        let resp = ui.add_enabled(enabled, egui::Button::selectable(has, &t.name)).on_hover_text(&t.desc);
                        if resp.clicked() {
                            if has {
                                m.traits.retain(|x| x != &t.id);
                            } else {
                                m.traits.push(t.id.clone());
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Quirk");
                    let cur = data.quirk(&m.quirk).map(|q| q.name.clone()).unwrap_or_else(|| "?".into());
                    egui::ComboBox::from_id_salt("quirk").selected_text(cur).show_ui(ui, |ui| {
                        for q in &data.quirks {
                            if ui.selectable_label(m.quirk == q.id, &q.name).on_hover_text(&q.desc).clicked() {
                                m.quirk = q.id.clone();
                            }
                        }
                    });
                });
            });
            if m.stage.is_adult() && m.stage != LifeStage::Elder {
                ui.horizontal(|ui| {
                    ui.label("Career");
                    let cur = m.career.as_ref().and_then(|c| data.career(c)).map(|c| c.name.clone()).unwrap_or_else(|| "Unemployed".into());
                    egui::ComboBox::from_id_salt("career").selected_text(cur).show_ui(ui, |ui| {
                        if ui.selectable_label(m.career.is_none(), "Unemployed").clicked() {
                            m.career = None;
                        }
                        for cd in data.careers.iter().filter(|cd| cd.min_stage <= m.stage) {
                            if ui.selectable_label(m.career.as_deref() == Some(&cd.id), &cd.name).on_hover_text(&cd.desc).clicked() {
                                m.career = Some(cd.id.clone());
                            }
                        }
                    });
                });
            }
        });
        ui.separator();
        if let Err(e) = c.validate() {
            ui.label(RichText::new(e).color(Color32::from_rgb(255, 150, 120)));
        } else if let Some(e) = &c.error {
            ui.label(RichText::new(e).color(Color32::from_rgb(255, 150, 120)));
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(c.validate().is_ok(), egui::Button::new(RichText::new("Move In").strong())).clicked() {
                action = CreatorAction::MoveIn;
            }
            if ui.button("Cancel").clicked() {
                action = CreatorAction::Cancel;
            }
            ui.label(RichText::new(format!("Starting funds: ${START_FUNDS}")).weak());
        });
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creator_makes_a_valid_household() {
        let mut c = Creator::new(Arc::new(GameData::embedded()), 3);
        c.add_member(LifeStage::Adult);
        c.add_member(LifeStage::Child);
        c.add_member(LifeStage::Infant);
        c.validate().unwrap();
        let sim = c.move_in(3).unwrap();
        let peons = sim.living_peons();
        assert_eq!(peons.len(), 4);
        let kid = peons.iter().find(|e| sim.world.get::<sim::components::Age>(**e).unwrap().stage == LifeStage::Child).unwrap();
        assert_eq!(sim.world.get::<sim::components::Family>(*kid).unwrap().parents.len(), 2);
        assert!(sim.world.has::<sim::components::Student>(*kid));
        assert!(sim.world.res::<sim::economy::Household>().name.contains(&c.family_name));
    }

    #[test]
    fn validation_catches_mistakes() {
        let data = Arc::new(GameData::embedded());
        let mut c = Creator::new(data.clone(), 1);
        c.set_stage(0, LifeStage::Child);
        assert!(c.validate().unwrap_err().contains("bills"));
        c.set_stage(0, LifeStage::Adult);
        c.add_member(LifeStage::Adult);
        c.members[1].first = c.members[0].first.clone();
        assert!(c.validate().unwrap_err().contains("called"));
        c.members[1].first = "Zed".into();
        c.members[1].axes = Axes { neat: 10, outgoing: 10, active: 10, playful: 0, nice: 0 };
        assert!(c.validate().unwrap_err().contains("points"));
        c.members[1].axes = Axes { neat: 5, outgoing: 5, active: 5, playful: 5, nice: 5 };
        c.validate().unwrap();
        c.family_name = "  ".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn elders_go_grey_and_back() {
        let mut c = Creator::new(Arc::new(GameData::embedded()), 2);
        let natural = c.members[0].genes.clone().unwrap().hair;
        c.set_stage(0, LifeStage::Elder);
        assert_eq!(c.members[0].appearance.hair, genetics::ELDER_HAIR);
        c.set_stage(0, LifeStage::Adult);
        assert_eq!(c.members[0].appearance.hair, natural);
    }
}
