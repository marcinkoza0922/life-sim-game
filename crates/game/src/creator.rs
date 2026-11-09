//! Create-a-Household: edit names, life stages, genes/looks, personality and careers, then move
//! the family into a house. The line-up preview draws the rigged bodies from [`crate::peons`]
//! (looks, height, build and hair style all apply live).

use std::sync::Arc;

use hearth_ui::theme::{self, CREAM, INK, INK_SOFT};
use hearth_ui::{Button, Color, FontId, Id, Order, SpringCfg, Text, Ui, Vec2, vec2};
use glam::Vec3;
use sim::components::{Appearance, Axes, Genes, Sex};
use sim::data::{GameData, LifeStage};
use sim::genetics::{self, BODY_SLIDERS, BOTTOMS, DRESS, EYE_COLORS, FACE_SLIDERS, FACIAL_HAIR, HAIR_STYLES, TOPS};
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

/// The member editor's pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Basics,
    Face,
    Body,
    Hair,
    Outfit,
    Personality,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Basics, Tab::Face, Tab::Body, Tab::Hair, Tab::Outfit, Tab::Personality];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Basics => "Basics",
            Tab::Face => "Face",
            Tab::Body => "Body",
            Tab::Hair => "Hair",
            Tab::Outfit => "Outfit",
            Tab::Personality => "Personality",
        }
    }
}

/// Where the camera should look while editing: (offset from the line-up anchor, distance, pitch,
/// lift of the focus point above the floor focus).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    pub offset: Vec3,
    pub dist: f32,
    pub pitch: f32,
    pub lift: f32,
}

/// Spacing of the preview line-up.
pub const LINEUP_SPACING: f32 = 1.1;

pub struct Creator {
    pub family_name: String,
    pub members: Vec<PeonSpec>,
    pub editing: usize,
    pub tab: Tab,
    /// Where the line-up stands (the camera frames members relative to it).
    pub anchor: Vec3,
    /// The last camera shot applied (so the player can orbit freely until the shot changes).
    pub shot: Option<Shot>,
    /// Marry the first two adults on move-in.
    pub couple: bool,
    pub error: Option<String>,
    /// Vacant lots to move into (lot id, label), and the one picked.
    pub lots: Vec<(usize, String)>,
    pub lot: Option<usize>,
    scratch: Sim,
    data: Arc<GameData>,
}

impl Creator {
    pub fn new(data: Arc<GameData>, seed: u64) -> Creator {
        let mut scratch = Sim::new(seed, (*data).clone());
        let first = scratch.random_spec(LifeStage::YoungAdult, None);
        let family_name = first.last.clone();
        let mut c = Creator { family_name, members: Vec::new(), editing: 0, tab: Tab::Basics, anchor: Vec3::ZERO, shot: None, couple: true, error: None, lots: Vec::new(), lot: None, scratch, data };
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
            let mut g = genetics::inherit(&mut rng, &adults[0], &adults[1]);
            genetics::sexed_shape(&mut rng, spec.sex, &mut g.shape);
            spec.appearance = genetics::express(&g, stage, &spec.appearance);
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

    /// Moves this household into the picked vacant lot of a running neighborhood (the household
    /// being played becomes a neighbor).
    pub fn move_in_to(&self, sim: &mut Sim) -> Result<(), String> {
        self.validate()?;
        let lot = self.lot.ok_or("Pick a lot to move into.")?;
        sim.found_household(lot, format!("The {} Family", self.family_name.trim()), START_FUNDS)?;
        spawn_household(sim, &self.family_name, &self.members, self.couple, |sim, i| sim::neighborhood::move_in_spot(&sim.world, i));
        Ok(())
    }

    /// Switches a member's sex: rerolls the sex-linked body shape (frame, chest) and the hair,
    /// facial hair and outfit to match; face, colours and personality stay.
    pub fn set_sex(&mut self, i: usize, sex: Sex) {
        let m = &mut self.members[i];
        if m.sex == sex {
            return;
        }
        m.sex = sex;
        let mut rng = sim::rng::SimRng::new(i as u64 * 31 + m.first.len() as u64 + sex as u64);
        let mut g = m.genes.clone().unwrap_or_else(|| genetics::genes_from_appearance(&m.appearance));
        genetics::sexed_shape(&mut rng, sex, &mut g.shape);
        genetics::random_style(&mut rng, sex, m.stage, &mut m.appearance);
        m.genes = Some(g);
        refresh_appearance(m);
    }

    /// Rerolls a member's face sliders (keeping body, colours and everything else).
    pub fn random_face(&mut self, i: usize, seed: u64) {
        let m = &mut self.members[i];
        let mut rng = sim::rng::SimRng::new(seed);
        let fresh = genetics::random_shape(&mut rng, m.sex);
        let mut g = m.genes.clone().unwrap_or_else(|| genetics::genes_from_appearance(&m.appearance));
        for (id, _) in FACE_SLIDERS {
            g.shape.insert(id.to_string(), fresh[id]);
        }
        m.genes = Some(g);
        refresh_appearance(m);
    }

    /// The camera shot for the current tab: close on the face for Face and Hair, the whole body
    /// for Body and Outfit, the family otherwise.
    pub fn camera_shot(&self) -> Shot {
        let n = self.members.len() as f32;
        let m = &self.members[self.editing];
        let x = (self.editing as f32 - (n - 1.0) * 0.5) * LINEUP_SPACING;
        let h = m.stage.height() * m.appearance.height;
        // the line-up stands on 0.25 m plinths; lift is measured from the floor focus
        match self.tab {
            Tab::Face | Tab::Hair => Shot { offset: Vec3::new(x, 0.0, 0.0), dist: 0.9 + h * 0.35, pitch: 0.1, lift: 0.25 + h * 0.93 },
            Tab::Body | Tab::Outfit => Shot { offset: Vec3::new(x, 0.0, 0.0), dist: 1.5 + h * 1.5, pitch: 0.2, lift: 0.25 + h * 0.55 },
            _ => Shot { offset: Vec3::ZERO, dist: 9.0, pitch: 0.75, lift: 0.0 },
        }
    }

    /// Bodies for the preview line-up (stage, appearance).
    pub fn preview(&self) -> Vec<(LifeStage, Appearance, bool)> {
        self.members.iter().enumerate().map(|(i, m)| (m.stage, m.appearance.clone(), i == self.editing)).collect()
    }
}

/// Recomputes the rendered look from genes (hair goes grey for elders).
pub fn refresh_appearance(m: &mut PeonSpec) {
    let g = m.genes.clone().unwrap_or_else(|| genetics::genes_from_appearance(&m.appearance));
    m.appearance = genetics::express(&g, m.stage, &m.appearance);
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
    sim::starter::build_house(&mut sim.world);
    let spots = [(11, 16), (13, 16), (12, 15), (14, 15), (10, 15), (15, 16), (11, 17), (13, 17)];
    spawn_household(&mut sim, family, members, couple, |_, i| {
        let (x, z) = spots[i % spots.len()];
        Lot::tile_center(0, glam::IVec2::new(x, z))
    });
    {
        let mut hh = sim.world.res_mut::<sim::economy::Household>();
        hh.name = format!("The {} Family", family.trim());
        hh.funds = START_FUNDS;
    }
    sim.populate_neighborhood();
    sim
}

/// Spawns the household's members into the played household, with family ties.
fn spawn_household(sim: &mut Sim, family: &str, members: &[PeonSpec], couple: bool, spot: impl Fn(&Sim, usize) -> Vec3) {
    let mut spawned: Vec<(Entity, LifeStage)> = Vec::new();
    for (i, m) in members.iter().enumerate() {
        let mut spec = m.clone();
        spec.first = spec.first.trim().to_string();
        spec.last = family.trim().to_string();
        let pos = spot(sim, i);
        let e = sim.spawn_peon(spec, pos);
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
}

// ------------------------------------------------------------------------------------------------
// UI

/// Clothing colours for shirts and trousers.
pub const CLOTHES: [[f32; 3]; 12] = [
    [0.86, 0.3, 0.3],
    [0.95, 0.55, 0.25],
    [0.96, 0.8, 0.3],
    [0.45, 0.75, 0.4],
    [0.2, 0.62, 0.58],
    [0.3, 0.55, 0.85],
    [0.5, 0.4, 0.8],
    [0.85, 0.45, 0.7],
    [0.92, 0.9, 0.85],
    [0.55, 0.55, 0.6],
    [0.25, 0.25, 0.32],
    [0.45, 0.32, 0.22],
];

pub const SHOES: [[f32; 3]; 6] = [[0.15, 0.12, 0.1], [0.92, 0.92, 0.9], [0.35, 0.22, 0.12], [0.2, 0.2, 0.25], [0.7, 0.2, 0.2], [0.3, 0.45, 0.7]];

/// Sliders for shape ids, -1..1 (stored on the genes so children inherit them).
fn shape_sliders(ui: &mut Ui, g: &mut Genes, sliders: &[(&str, &str)]) -> bool {
    let mut changed = false;
    for (id, label) in sliders {
        let mut v = g.shape.get(*id).copied().unwrap_or(0.0);
        if ui.slider(label, &mut v, -1.0, 1.0, 0.0, 220.0) {
            g.shape.insert(id.to_string(), v);
            changed = true;
        }
    }
    changed
}

/// One chip per option; returns true when the pick changed.
fn chips(ui: &mut Ui, options: &[(&str, &str)], current: &mut u8) -> bool {
    let mut changed = false;
    ui.wrap(|ui| {
        for (k, (_, label)) in options.iter().enumerate() {
            if ui.chip(*label, *current as usize == k).clicked && *current as usize != k {
                *current = k as u8;
                changed = true;
            }
        }
    });
    changed
}

/// Personality points, traits, quirk and career.
fn personality(ui: &mut Ui, data: &GameData, m: &mut PeonSpec) {
    let a = &mut m.axes;
    let total = |a: &Axes| a.neat as u32 + a.outgoing as u32 + a.active as u32 + a.playful as u32 + a.nice as u32;
    let used = total(a);
    ui.row(0.0, |ui| {
        ui.text(Text::new("Points").bold().size(14.0));
        ui.bar("points", used as f32 / AXIS_BUDGET as f32, if used > AXIS_BUDGET { theme::CORAL } else { theme::LILAC }, vec2(200.0, 16.0));
        ui.text(Text::new(format!("{used} / {AXIS_BUDGET}")).size(13.0).color(if used > AXIS_BUDGET { theme::CORAL } else { INK_SOFT }));
    });
    for (name, v) in [("Neat", &mut a.neat), ("Outgoing", &mut a.outgoing), ("Active", &mut a.active), ("Playful", &mut a.playful), ("Nice", &mut a.nice)] {
        ui.slider_u8(name, v, 0, 10, 220.0);
    }
    let max = max_traits(m.stage);
    ui.text(Text::new(format!("Traits ({} of {max})", m.traits.len())).bold().size(14.0));
    ui.wrap(|ui| {
        for t in &data.traits {
            let has = m.traits.contains(&t.id);
            let conflict = m.traits.iter().any(|o| t.conflicts.contains(o) || data.trait_def(o).is_some_and(|d| d.conflicts.contains(&t.id)));
            let enabled = has || (!conflict && m.traits.len() < max);
            let r = ui.btn(Button::new(t.name.clone()).small().color(theme::LILAC).selected(has).enabled(enabled));
            ui.tooltip(&r, t.desc.clone());
            if r.clicked {
                if has {
                    m.traits.retain(|x| x != &t.id);
                } else {
                    m.traits.push(t.id.clone());
                }
            }
        }
    });
    field(ui, "Quirk", |ui| {
        let cur = data.quirk(&m.quirk).map(|q| q.name.clone()).unwrap_or_else(|| "?".into());
        let names: Vec<String> = data.quirks.iter().map(|q| q.name.clone()).collect();
        if let Some(k) = ui.dropdown("quirk", &cur, &names, 250.0) {
            m.quirk = data.quirks[k].id.clone();
        }
    });
    if let Some(q) = data.quirk(&m.quirk) {
        ui.small(q.desc.clone());
    }
    if m.stage.is_adult() && m.stage != LifeStage::Elder {
        ui.section("Career", theme::SKY);
        let options: Vec<&sim::data::CareerDef> = data.careers.iter().filter(|cd| cd.min_stage <= m.stage).collect();
        let mut names = vec!["Unemployed".to_string()];
        names.extend(options.iter().map(|cd| cd.name.clone()));
        let cur = m.career.as_ref().and_then(|c| data.career(c)).map(|c| c.name.clone()).unwrap_or_else(|| "Unemployed".into());
        field(ui, "Job", |ui| {
            if let Some(k) = ui.dropdown("career", &cur, &names, 250.0) {
                m.career = if k == 0 { None } else { Some(options[k - 1].id.clone()) };
            }
        });
    }
}

fn swatches(ui: &mut Ui, palette: &[[f32; 3]], current: &mut [f32; 3]) -> bool {
    let size = if palette.len() > 10 { 21.0 } else { 24.0 };
    let mut changed = false;
    ui.wrap(|ui| {
        for c in palette {
            let selected = (current[0] - c[0]).abs() + (current[1] - c[1]).abs() + (current[2] - c[2]).abs() < 0.02;
            if ui.swatch(Color::rgb(*c), selected, size).clicked {
                *current = *c;
                changed = true;
            }
        }
    });
    changed
}

/// A labelled row: a fixed-width caption then the content.
fn field<R>(ui: &mut Ui, label: &str, f: impl FnOnce(&mut Ui) -> R) -> R {
    ui.row(36.0, |ui| {
        let r = ui.alloc(vec2(96.0, 20.0));
        ui.paint_text(vec2(r.min.x, r.center().y), vec2(0.0, 0.5), label, FontId::Bold, 14.0, INK);
        f(ui)
    })
}

/// What the player chose this frame.
pub enum CreatorAction {
    None,
    Cancel,
    MoveIn,
}

pub fn screen(ui: &mut Ui, c: &mut Creator) -> CreatorAction {
    let mut action = CreatorAction::None;
    let data = c.data.clone();
    let w = 480.0;
    let pos = vec2(16.0, 16.0);
    let pop = ui.spring(Id::new("creator"), 0, 1.0, 0.6, SpringCfg::BOUNCY);
    ui.area("creator", Order::Panel, pos, w, |ui| {
        ui.painter.push_transform(pos, Vec2::splat(pop), 0.0, Vec2::ZERO);
        ui.card(CREAM, 16.0, Some(w), |ui| {
            ui.heading("Create a Household");
            field(ui, "Family name", |ui| {
                if ui.text_field("family", &mut c.family_name, "Lastname", 250.0).changed {
                    let name = c.family_name.clone();
                    for m in &mut c.members {
                        m.last = name.clone();
                    }
                }
            });
            ui.wrap(|ui| {
                for i in 0..c.members.len() {
                    let m = &c.members[i];
                    let label = format!("{} · {}", if m.first.is_empty() { "?" } else { &m.first }, m.stage.label());
                    if ui.btn(Button::new(label).small().color(theme::SUN).selected(c.editing == i)).clicked {
                        c.editing = i;
                    }
                }
            });
            ui.row(0.0, |ui| {
                let stages: Vec<String> = STAGES.iter().map(|s| s.label().to_string()).collect();
                if let Some(i) = ui.dropdown("add", "➕ Add person", &stages, 150.0) {
                    c.add_member(STAGES[i]);
                }
                if ui.btn(Button::new("Remove").small().color(theme::CORAL).enabled(c.members.len() > 1)).clicked {
                    c.remove(c.editing);
                }
                let r = ui.btn(Button::new("🎲").small().round().color(theme::LILAC));
                ui.tooltip(&r, "Randomize this person");
                if r.clicked {
                    c.randomize(c.editing);
                }
                let r = ui.chip("💕 Couple", c.couple);
                ui.tooltip(&r, "The first two adults are married");
                if r.clicked {
                    c.couple = !c.couple;
                }
            });
            ui.dots();
            let i = c.editing;
            ui.wrap(|ui| {
                for t in Tab::ALL {
                    if ui.chip(t.label(), c.tab == t).clicked {
                        c.tab = t;
                    }
                }
            });
            let tab = c.tab;
            let h = (ui.screen.max.y - ui.cursor().y - 190.0).max(160.0);
            ui.scroll(("member", i, tab as u8), h, |ui| {
                let mut new_stage = None;
                {
                    let m = &mut c.members[i];
                    field(ui, "First name", |ui| ui.text_field("first", &mut m.first, "Firstname", 250.0));
                    field(ui, "Life stage", |ui| {
                        let labels: Vec<String> = STAGES.iter().map(|s| s.label().to_string()).collect();
                        if let Some(k) = ui.dropdown("stage", m.stage.label(), &labels, 170.0) {
                            new_stage = Some(STAGES[k]);
                        }
                    });
                }
                if let Some(s) = new_stage {
                    c.set_stage(i, s);
                }
                let m = &mut c.members[i];
                let mut g = m.genes.clone().unwrap_or_else(|| genetics::genes_from_appearance(&m.appearance));
                let mut changed = false;
                let mut new_sex = None;
                let mut reroll_face = false;
                match tab {
                    Tab::Basics => {
                        field(ui, "Sex", |ui| {
                            for (s, label) in [(Sex::Female, "Female"), (Sex::Male, "Male")] {
                                if ui.chip(label, m.sex == s).clicked {
                                    new_sex = Some(s);
                                }
                            }
                        });
                        field(ui, "Skin", |ui| changed |= swatches(ui, &SKIN_TONES, &mut g.skin));
                        field(ui, "Eyes", |ui| changed |= swatches(ui, &EYE_COLORS, &mut g.eyes));
                        changed |= ui.slider("Height", &mut g.height, 0.9, 1.1, 0.0, 220.0);
                        changed |= ui.slider("Build", &mut g.build, 0.0, 1.0, 0.0, 220.0);
                        ui.small("Shape the face, body, hair and clothes on the other tabs.");
                    }
                    Tab::Face => {
                        ui.row(0.0, |ui| {
                            if ui.btn(Button::new("🎲 Random face").small().color(theme::LILAC)).clicked {
                                reroll_face = true;
                            }
                            if ui.btn(Button::new("Average").small()).clicked {
                                for (id, _) in FACE_SLIDERS {
                                    g.shape.remove(id);
                                }
                                changed = true;
                            }
                        });
                        changed |= shape_sliders(ui, &mut g, &FACE_SLIDERS);
                    }
                    Tab::Body => {
                        changed |= ui.slider("Height", &mut g.height, 0.9, 1.1, 0.0, 220.0);
                        changed |= ui.slider("Build", &mut g.build, 0.0, 1.0, 0.0, 220.0);
                        changed |= shape_sliders(ui, &mut g, &BODY_SLIDERS);
                        if m.stage < LifeStage::Teen {
                            ui.small("Frame and chest grow in as children grow up.");
                        }
                    }
                    Tab::Hair => {
                        field(ui, "Colour", |ui| changed |= swatches(ui, &HAIR_COLORS, &mut g.hair));
                        if m.stage == LifeStage::Elder {
                            ui.small("Elders go grey; this is the colour they'd pass on.");
                        }
                        ui.text(Text::new("Style").bold().size(14.0));
                        changed |= chips(ui, &HAIR_STYLES, &mut m.appearance.hair_style);
                        if m.stage.is_adult() {
                            ui.text(Text::new("Facial hair").bold().size(14.0));
                            changed |= chips(ui, &FACIAL_HAIR, &mut m.appearance.facial_hair);
                        }
                    }
                    Tab::Outfit => {
                        ui.text(Text::new("Top").bold().size(14.0));
                        changed |= chips(ui, &TOPS, &mut m.appearance.top);
                        field(ui, "Colour", |ui| swatches(ui, &CLOTHES, &mut m.appearance.shirt));
                        if m.appearance.top != DRESS {
                            ui.text(Text::new("Bottom").bold().size(14.0));
                            changed |= chips(ui, &BOTTOMS, &mut m.appearance.bottom);
                            field(ui, "Colour", |ui| swatches(ui, &CLOTHES, &mut m.appearance.pants));
                        }
                        field(ui, "Shoes", |ui| swatches(ui, &SHOES, &mut m.appearance.shoes));
                    }
                    Tab::Personality => personality(ui, &data, m),
                }
                if changed {
                    m.genes = Some(g);
                    refresh_appearance(m);
                }
                if let Some(s) = new_sex {
                    c.set_sex(i, s);
                }
                if reroll_face {
                    let seed = c.members[i].appearance.shape.values().map(|v| v.to_bits() as u64).fold(i as u64, |a, b| a.rotate_left(7) ^ b);
                    c.random_face(i, seed);
                }
            });
            ui.dots();
            if let Err(e) = c.validate() {
                ui.text(Text::new(e).size(13.5).color(theme::CORAL.darken(0.15)).wrap());
            } else if let Some(e) = &c.error {
                ui.text(Text::new(e.clone()).size(13.5).color(theme::CORAL.darken(0.15)).wrap());
            }
            field(ui, "Move into", |ui| {
                let current = c.lots.iter().find(|(l, _)| Some(*l) == c.lot).map(|(_, s)| s.clone()).unwrap_or_else(|| "(no vacant lots)".into());
                let labels: Vec<String> = c.lots.iter().map(|(_, s)| s.clone()).collect();
                if let Some(k) = ui.dropdown("lot", &current, &labels, 300.0) {
                    c.lot = Some(c.lots[k].0);
                }
            });
            ui.row(50.0, |ui| {
                if ui.btn(Button::new("Move in!").big().color(theme::GRASS).enabled(c.validate().is_ok() && c.lot.is_some())).clicked {
                    action = CreatorAction::MoveIn;
                }
                if ui.btn(Button::new("Cancel").color(theme::CORAL)).clicked {
                    action = CreatorAction::Cancel;
                }
                ui.text(Text::new(format!("Starting funds {}", crate::ui::money(START_FUNDS))).size(13.0).soft());
            });
        });
        ui.painter.pop_transform();
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
        let peons: Vec<Entity> = sim.living_peons().into_iter().filter(|e| sim.world.has::<sim::components::Member>(*e)).collect();
        assert_eq!(peons.len(), 4);
        let kid = peons.iter().find(|e| sim.world.get::<sim::components::Age>(**e).unwrap().stage == LifeStage::Child).unwrap();
        assert_eq!(sim.world.get::<sim::components::Family>(*kid).unwrap().parents.len(), 2);
        assert!(sim.world.has::<sim::components::Student>(*kid));
        assert!(sim.world.res::<sim::economy::Household>().name.contains(&c.family_name));
    }

    #[test]
    fn moving_into_a_running_neighborhood() {
        let mut sim = sim::starter::starter_sim(8);
        sim.populate_neighborhood();
        let old = sim.world.res::<sim::economy::Household>().members.clone();
        let mut c = Creator::new(Arc::new(GameData::embedded()), 5);
        c.add_member(LifeStage::Adult);
        c.lot = sim::neighborhood::vacant_lots(&sim.world).first().copied();
        c.move_in_to(&mut sim).unwrap();
        assert_eq!(sim::neighborhood::active_lot(&sim.world), c.lot.unwrap());
        let hh = sim.world.res::<sim::economy::Household>().clone();
        assert_eq!(hh.members.len(), 2);
        assert_eq!(hh.funds, START_FUNDS);
        assert!(old.iter().all(|e| sim.world.has::<sim::components::Offlot>(*e)), "the old family are neighbors now");
        sim.run_minutes(120);
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
    fn sliders_and_styles_survive_stage_changes() {
        let mut c = Creator::new(Arc::new(GameData::embedded()), 4);
        let mut g = c.members[0].genes.clone().unwrap();
        g.shape.insert("nose_length".into(), 0.7);
        c.members[0].genes = Some(g);
        c.members[0].appearance.top = DRESS;
        c.members[0].appearance.hair_style = 6;
        refresh_appearance(&mut c.members[0]);
        c.set_stage(0, LifeStage::Elder);
        let a = &c.members[0].appearance;
        assert_eq!((a.shape("nose_length"), a.top, a.hair_style), (0.7, DRESS, 6));
    }

    #[test]
    fn switching_sex_reshapes_the_frame_but_keeps_the_face() {
        let mut c = Creator::new(Arc::new(GameData::embedded()), 6);
        c.set_sex(0, Sex::Female);
        let jaw = c.members[0].appearance.shape("jaw");
        let nose = c.members[0].appearance.shape("nose_width");
        c.set_sex(0, Sex::Male);
        let a = &c.members[0].appearance;
        assert!(a.shape("frame") > 0.0);
        assert_eq!(a.shape("nose_width"), nose);
        assert_eq!(a.shape("jaw"), jaw);
        c.random_face(0, 99);
        assert!(c.members[0].appearance.shape("frame") > 0.0, "a new face keeps the body");
    }

    #[test]
    fn kids_inherit_their_parents_faces() {
        let mut c = Creator::new(Arc::new(GameData::embedded()), 7);
        c.add_member(LifeStage::Adult);
        for (m, v) in c.members.iter_mut().zip([0.9f32, 0.7]) {
            let mut g = m.genes.clone().unwrap();
            g.shape.insert("nose_length".into(), v);
            m.genes = Some(g);
        }
        c.add_member(LifeStage::Child);
        let kid = c.members.last().unwrap().appearance.shape("nose_length");
        assert!(kid > 0.4, "long-nosed parents, long-nosed kid: {kid}");
    }

    #[test]
    fn the_camera_closes_in_on_the_face_tab() {
        let mut c = Creator::new(Arc::new(GameData::embedded()), 2);
        let wide = c.camera_shot();
        c.tab = Tab::Face;
        let close = c.camera_shot();
        assert!(close.dist < 2.0 && close.dist < wide.dist && close.lift > 0.8);
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
