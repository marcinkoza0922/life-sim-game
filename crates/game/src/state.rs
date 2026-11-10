//! Game state outside the simulation: camera, modes, tools, selection, picking, time control.

use glam::{IVec2, Vec3};
use hearth_render::Camera;
use sim::components::*;
use sim::lot::{Axis, Dir4, Edge, LEVEL_HEIGHT, Lot, MAX_LEVEL, MIN_LEVEL, Opening, Roof, RoofKind, Stair, TerrainBrush};
use sim::objects::ObjectInstance;
use sim::{Entity, Sim};

use crate::camera::OrbitCamera;
use crate::scene::{Ghost, WallMode};

/// Ticks per real second for each speed setting.
pub const SPEEDS: [(f32, &str); 4] = [(1.0, "1x"), (3.0, "2x"), (10.0, "3x"), (60.0, "Ultra")];

pub const COST_WALL: i64 = 70;
pub const COST_FLOOR: i64 = 10;
pub const COST_DOOR: i64 = 150;
pub const COST_WINDOW: i64 = 100;
pub const COST_STAIRS: i64 = 800;
pub const COST_DIG: i64 = 120;
pub const COST_ROOF_TILE: i64 = 15;
/// Per terrain corner moved.
pub const COST_TERRAIN: i64 = 2;
pub const QUICKSAVE: &str = "saves/quicksave.ron";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Live,
    Buy,
    Build,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    None,
    Place { def: String, rot: u8 },
    Sell,
    Wall,
    Room,
    DeleteWall,
    Door,
    Window,
    Floor,
    DeleteFloor,
    Stairs { dir: Dir4 },
    Basement,
    Terrain { brush: TerrainBrush, radius: f32 },
    Roof { kind: RoofKind },
    DeleteRoof,
}

#[derive(Clone, Debug)]
pub enum Pick {
    Peon(Entity),
    Object(Entity),
    Floor { level: i32, pos: Vec3 },
}

#[derive(Clone, Debug)]
pub struct PieMenu {
    pub peon: Entity,
    pub target: Option<Entity>,
    pub title: String,
    pub screen: (f32, f32),
    pub options: Vec<(String, ActionKind, Result<(), String>)>,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub kind: sim::log::LogKind,
    pub born: f32,
}

#[derive(Default, Clone, Copy)]
pub struct DebugFlags {
    pub inspector: bool,
    pub autonomy: bool,
    pub overlays: bool,
    pub profiler: bool,
    pub settings: bool,
    pub log: bool,
}

pub struct Game {
    pub sim: Sim,
    pub cam: OrbitCamera,
    pub level: i32,
    pub wall_mode: WallMode,
    pub speed: usize,
    pub paused: bool,
    pub acc: f32,
    pub alpha: f32,
    pub time: f32,
    pub mode: Mode,
    pub tool: Tool,
    pub floor_style: u16,
    pub wall_style: u16,
    pub selected: Option<Entity>,
    pub hovered: Option<Entity>,
    pub hover_pick: Option<Pick>,
    pub drag_start: Option<IVec2>,
    pub menu: Option<PieMenu>,
    pub debug: DebugFlags,
    pub toasts: Vec<Toast>,
    pub log_seen: u64,
    pub inspect_entity: Option<Entity>,
    pub status: Option<(String, f32)>,
    pub screenshot: Option<std::path::PathBuf>,
    pub frame_ms: f32,
    pub sim_ms: f32,
    pub show_roofs: bool,
    pub roof_pitch: f32,
    /// Create-a-Household editor, when open.
    pub creator: Option<crate::creator::Creator>,
    /// Mouse held with the terrain tool (brush repeats while held).
    pub sculpting: Option<f32>,
    pub show_neighborhood: bool,
    /// Save/load window, with the name typed for the next save.
    pub show_saves: bool,
    pub save_name: String,
    /// Day of the last autosave (`None` until the first day change is seen).
    pub autosave_day: Option<u64>,
    pub mute: bool,
    /// Sound volumes (☰ Game menu); saved to `settings.ron` when changed.
    pub volumes: crate::audio::Volumes,
    pub graphics: crate::graphics::GraphicsSettings,
    /// One-shot sound effects requested this frame (ids in `sounds::IDS`), played by the app.
    pub sfx: Vec<&'static str>,
    /// The lot that was live last frame (see [`Game::check_lot_change`]).
    pub shown_lot: usize,
    /// Interface-only state (open menus, tabs).
    pub hud: crate::ui::HudState,
}

impl Game {
    pub fn new(sim: Sim) -> Game {
        let selected = sim.living_peons().into_iter().find(|e| sim.world.has::<Member>(*e));
        let shown_lot = sim::neighborhood::active_lot(&sim.world);
        let mut cam = OrbitCamera::new(Vec3::new(15.0, 0.0, 14.0));
        cam.snap();
        Game {
            sim,
            cam,
            level: 0,
            wall_mode: WallMode::Cutaway,
            speed: 0,
            paused: false,
            acc: 0.0,
            alpha: 1.0,
            time: 0.0,
            mode: Mode::Live,
            tool: Tool::None,
            floor_style: 0,
            wall_style: 1,
            selected,
            hovered: None,
            hover_pick: None,
            drag_start: None,
            menu: None,
            debug: DebugFlags::default(),
            toasts: Vec::new(),
            log_seen: 0,
            inspect_entity: None,
            status: None,
            screenshot: None,
            frame_ms: 0.0,
            sim_ms: 0.0,
            show_roofs: true,
            roof_pitch: 0.5,
            creator: None,
            sculpting: None,
            show_neighborhood: false,
            show_saves: false,
            save_name: String::new(),
            autosave_day: None,
            mute: false,
            volumes: Default::default(),
            graphics: crate::graphics::GraphicsSettings::load(),
            sfx: Vec::new(),
            shown_lot,
            hud: Default::default(),
        }
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), self.time));
    }

    /// Advances real time; runs as many sim ticks as the speed allows.
    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        self.cam.update(dt);
        let build_paused = self.mode != Mode::Live || self.creator.is_some();
        if !self.paused && !build_paused {
            self.acc += dt * SPEEDS[self.speed].0;
            let t0 = std::time::Instant::now();
            let mut n = 0;
            while self.acc >= 1.0 && n < 240 {
                self.sim.tick();
                self.acc -= 1.0;
                n += 1;
                // don't let ultra speed starve the frame
                if t0.elapsed().as_millis() > 25 {
                    self.acc = self.acc.min(1.0);
                    break;
                }
            }
            if n > 0 {
                self.sim_ms = t0.elapsed().as_secs_f32() * 1000.0;
            }
            self.alpha = self.acc.clamp(0.0, 1.0);
        } else {
            self.alpha = 1.0;
        }
        self.check_lot_change();
        // selected peon may have died / left
        if let Some(s) = self.selected
            && (!self.sim.world.contains(s) || self.sim.world.has::<Dead>(s) || self.sim.world.has::<Offlot>(s)) {
                self.selected = self.first_member();
            }
        self.pull_notifications();
    }

    fn pull_notifications(&mut self) {
        let log = self.sim.world.res::<sim::log::SimLog>();
        let total = log.total;
        if total > self.log_seen {
            let new = (total - self.log_seen) as usize;
            let skip = log.entries.len().saturating_sub(new);
            for e in &log.entries[skip..] {
                if let Some(s) = crate::sounds::for_log(e.kind) {
                    self.sfx.push(s);
                }
                if e.kind != sim::log::LogKind::Debug {
                    self.toasts.push(Toast { text: e.text.clone(), kind: e.kind, born: self.time });
                }
            }
            self.log_seen = total;
        }
        drop(log);
        let now = self.time;
        self.toasts.retain(|t| now - t.born < 9.0);
        if self.toasts.len() > 6 {
            let n = self.toasts.len() - 6;
            self.toasts.drain(..n);
        }
    }

    pub fn camera(&self, aspect: f32) -> Camera {
        self.cam.camera(self.level, aspect)
    }

    /// Replaces the running simulation (load, new household), keeping camera and settings.
    pub fn replace_sim(&mut self, sim: Sim) {
        self.sim = sim;
        self.shown_lot = sim::neighborhood::active_lot(&self.sim.world);
        self.selected = self.first_member();
        self.menu = None;
        self.tool = Tool::None;
        self.mode = Mode::Live;
        self.inspect_entity = None;
        self.log_seen = self.sim.world.res::<sim::log::SimLog>().total;
        self.toasts.clear();
    }

    pub fn save_to(&mut self, path: &std::path::Path) {
        match self.sim.save_to_file(path) {
            Ok(()) => self.say(format!("Saved to {}", path.display())),
            Err(e) => self.say(format!("Save failed: {e}")),
        }
    }

    pub fn load_from(&mut self, path: &std::path::Path) {
        match Sim::load_from_file(path, (*self.sim.data()).clone()) {
            Ok(sim) => {
                self.replace_sim(sim);
                self.say(format!("Loaded {}", path.display()));
            }
            Err(e) => self.say(format!("Load failed: {e}")),
        }
    }

    /// Writes `saves/autosave.ron` once per sim day (interactive play only; the caller decides).
    pub fn autosave_if_new_day(&mut self) {
        let day = self.sim.clock().day();
        match self.autosave_day {
            None => self.autosave_day = Some(day),
            Some(d) if d != day => {
                self.autosave_day = Some(day);
                self.autosave();
            }
            _ => {}
        }
    }

    pub fn autosave(&mut self) {
        let path = crate::saves::path_for(crate::saves::AUTOSAVE);
        match self.sim.save_to_file(&path) {
            Ok(()) => self.say("Autosaved."),
            Err(e) => self.say(format!("Autosave failed: {e}")),
        }
    }

    /// A household member on the lot, else anyone in the household.
    pub fn first_member(&self) -> Option<Entity> {
        let w = &self.sim.world;
        let members: Vec<Entity> = self.sim.living_peons().into_iter().filter(|e| w.has::<Member>(*e)).collect();
        members.iter().copied().find(|e| !is_away(w, *e)).or(members.first().copied())
    }

    /// After the live lot changes (switching households, outings, moving): drop anything that
    /// pointed into the old lot and look at the new one from the street.
    pub fn check_lot_change(&mut self) {
        let active = sim::neighborhood::active_lot(&self.sim.world);
        if active == self.shown_lot {
            return;
        }
        self.shown_lot = active;
        self.selected = self.first_member();
        self.hovered = None;
        self.hover_pick = None;
        self.menu = None;
        self.tool = Tool::None;
        self.drag_start = None;
        self.inspect_entity = None;
        if !sim::neighborhood::at_home(&self.sim.world) && self.mode != Mode::Live {
            self.mode = Mode::Live;
        }
        self.level = 0;
        self.cam.target = Vec3::new(15.0, 0.0, 20.0);
        self.cam.dist = 22.0;
    }

    /// Build and Buy are only for your own lot.
    pub fn can_build(&self) -> bool {
        sim::neighborhood::at_home(&self.sim.world)
    }

    pub fn open_creator(&mut self) {
        let seed = self.sim.clock().tick ^ 0x5eed;
        let mut c = crate::creator::Creator::new(self.sim.data(), seed);
        let w = &self.sim.world;
        let hood = w.res::<sim::neighborhood::Neighborhood>().clone();
        for l in sim::neighborhood::vacant_lots(w) {
            let value = sim::neighborhood::build_value(w, l);
            let what = if value > 0 { "furnished house" } else { "empty plot" };
            c.lots.push((l, format!("{} — {what}", hood.lots[l].address)));
        }
        c.lot = c.lots.iter().find(|(l, _)| sim::neighborhood::build_value(w, *l) > 0).or(c.lots.first()).map(|(l, _)| *l);
        if c.lots.is_empty() {
            c.error = Some("There are no vacant lots. Move a household into a smaller place first.".into());
        }
        self.creator = Some(c);
        self.menu = None;
        self.mode = Mode::Live;
        self.tool = Tool::None;
        // stage the line-up on the front lawn
        self.level = 0;
        self.cam.target = Vec3::new(17.5, 0.0, 26.5);
        self.cam.dist = 9.0;
        self.cam.pitch = 0.32;
        self.cam.yaw = 0.25;
    }

    /// Applies the terrain brush at a world point, charging per corner moved.
    pub fn sculpt_at(&mut self, p: Vec3) {
        let Tool::Terrain { brush, radius } = self.tool.clone() else { return };
        if self.level != 0 {
            self.say("Terrain can only be shaped on the ground level.");
            return;
        }
        let corner = IVec2::new(p.x.round() as i32, p.z.round() as i32);
        let funds = self.sim.world.res::<sim::economy::Household>().funds;
        if funds < COST_TERRAIN * 4 {
            self.say("Can't afford to move any more dirt.");
            return;
        }
        let n = self.sim.lot_mut().sculpt(corner, radius, 0.12, brush) as i64;
        self.sim.world.res_mut::<sim::economy::Household>().funds -= n * COST_TERRAIN;
    }

    pub fn set_level(&mut self, l: i32) {
        self.level = l.clamp(MIN_LEVEL, MAX_LEVEL);
    }

    pub fn focus_selected(&mut self) {
        if let Some(t) = self.selected.and_then(|s| self.sim.world.get::<Transform>(s).map(|t| t.pos)) {
            self.cam.target = Vec3::new(t.x, 0.0, t.z);
            self.level = Lot::level_of(t);
        }
    }

    // ------------------------------------------------------------------ picking

    pub fn pick(&self, cam: &Camera, ndc: (f32, f32)) -> Option<Pick> {
        let (o, d) = cam.ray(ndc.0, ndc.1);
        let mut best: Option<(f32, Pick)> = None;
        let mut consider = |t: f32, p: Pick| {
            if t > 0.0 && best.as_ref().is_none_or(|b| t < b.0) {
                best = Some((t, p));
            }
        };
        let world = &self.sim.world;
        let top = self.level;
        let visible = |lvl: i32| lvl <= top && (top < 0 || lvl >= 0);
        for (e, tf, age, posture) in world.query_filtered::<(Entity, &Transform, &Age, &Posture), (hearth_ecs::Without<AtWork>, hearth_ecs::Without<Offlot>, hearth_ecs::Without<Dead>)>().iter() {
            if !visible(tf.level()) {
                continue;
            }
            if let Some(t) = ray_peon(o, d, tf, age.stage.height(), posture.pose) {
                consider(t - 0.3, Pick::Peon(e)); // peons win ties against what they stand, sit or lie on
            }
        }
        let data = self.sim.data();
        for (e, inst) in world.query::<(Entity, &ObjectInstance)>().iter() {
            if !visible(inst.level) {
                continue;
            }
            let def = data.object(inst.def);
            // in live mode a rug is floor: clicking it means "go here"
            if self.mode == Mode::Live && def.walkable && def.interactions.is_empty() {
                continue;
            }
            if let Some(t) = ray_object(o, d, def, inst) {
                consider(t, Pick::Object(e));
            }
        }
        let y = top as f32 * LEVEL_HEIGHT;
        if d.y.abs() > 1e-4 {
            let t = (y - o.y) / d.y;
            if t > 0.0 {
                consider(t + 0.01, Pick::Floor { level: top, pos: o + d * t });
            }
        }
        best.map(|b| b.1)
    }

    /// Ground-plane hit on the active level regardless of objects.
    pub fn floor_point(&self, cam: &Camera, ndc: (f32, f32)) -> Option<Vec3> {
        let (o, d) = cam.ray(ndc.0, ndc.1);
        let y = self.level as f32 * LEVEL_HEIGHT;
        if d.y.abs() < 1e-4 {
            return None;
        }
        let t = (y - o.y) / d.y;
        (t > 0.0).then(|| o + d * t)
    }

    // ------------------------------------------------------------------ tools

    /// Translucent previews for the current tool at the cursor.
    pub fn ghosts(&self, cursor: Option<Vec3>) -> Vec<Ghost> {
        let Some(p) = cursor else { return Vec::new() };
        let tile = Lot::tile_of(p);
        let corner = IVec2::new(p.x.round() as i32, p.z.round() as i32);
        let lvl = self.level;
        match &self.tool {
            Tool::Place { def, rot } => {
                let valid = self.sim.can_place(def, lvl, tile, *rot).is_ok();
                vec![Ghost::Object { def: def.clone(), level: lvl, tile, rot: *rot, valid }]
            }
            Tool::Wall | Tool::DeleteWall => {
                let start = self.drag_start.unwrap_or(corner);
                let edges = line_edges(lvl, start, corner);
                vec![Ghost::Walls { edges, valid: self.tool == Tool::Wall }]
            }
            Tool::Room => {
                let start = self.drag_start.unwrap_or(corner);
                let (min, max) = (start.min(corner), start.max(corner));
                let mut edges = Vec::new();
                if max.x > min.x && max.y > min.y {
                    edges.extend(line_edges(lvl, min, IVec2::new(max.x, min.y)));
                    edges.extend(line_edges(lvl, IVec2::new(min.x, max.y), max));
                    edges.extend(line_edges(lvl, min, IVec2::new(min.x, max.y)));
                    edges.extend(line_edges(lvl, IVec2::new(max.x, min.y), max));
                }
                vec![Ghost::Walls { edges, valid: true }]
            }
            Tool::Door | Tool::Window => {
                let e = nearest_edge(lvl, p);
                let valid = self.sim.world.res::<Lot>().walls.contains_key(&e);
                vec![Ghost::Walls { edges: vec![e], valid }]
            }
            Tool::Floor | Tool::DeleteFloor | Tool::Basement => {
                let start = self.drag_start.unwrap_or(tile);
                let (min, max) = (start.min(tile), start.max(tile));
                let mut tiles = Vec::new();
                for x in min.x..=max.x {
                    for z in min.y..=max.y {
                        tiles.push(IVec2::new(x, z));
                    }
                }
                let color = match self.tool {
                    Tool::Floor => [0.4, 0.8, 1.0],
                    Tool::Basement => [0.7, 0.5, 0.3],
                    _ => [1.0, 0.3, 0.3],
                };
                vec![Ghost::Tiles { level: lvl, tiles, color }]
            }
            Tool::Stairs { dir } => {
                let s = Stair { level: lvl, start: tile, dir: *dir };
                let tiles: Vec<IVec2> = s.tiles().collect();
                vec![Ghost::Tiles { level: lvl, tiles, color: [0.9, 0.8, 0.3] }, Ghost::Tiles { level: lvl, tiles: vec![s.bottom_access()], color: [0.3, 1.0, 0.3] }]
            }
            Tool::Terrain { radius, .. } => {
                let c = IVec2::new(p.x.round() as i32, p.z.round() as i32);
                let r = radius.ceil() as i32;
                let mut tiles = Vec::new();
                for dx in -r..r {
                    for dz in -r..r {
                        let t = c + IVec2::new(dx, dz);
                        let mid = t.as_vec2() + glam::Vec2::splat(0.5) - c.as_vec2();
                        if mid.length() <= *radius + 0.2 {
                            tiles.push(t);
                        }
                    }
                }
                vec![Ghost::Tiles { level: 0, tiles, color: [0.9, 0.7, 0.3] }]
            }
            Tool::Roof { .. } | Tool::DeleteRoof => {
                let start = self.drag_start.unwrap_or(tile);
                let (min, max) = (start.min(tile), start.max(tile));
                let mut tiles = Vec::new();
                for x in min.x..=max.x {
                    for z in min.y..=max.y {
                        tiles.push(IVec2::new(x, z));
                    }
                }
                let color = if self.tool == Tool::DeleteRoof { [1.0, 0.3, 0.3] } else { [0.85, 0.4, 0.3] };
                vec![Ghost::Tiles { level: lvl + 1, tiles, color }]
            }
            Tool::Sell | Tool::None => Vec::new(),
        }
    }

    pub fn rotate_tool(&mut self) {
        match &mut self.tool {
            Tool::Place { rot, .. } => *rot = (*rot + 1) % 4,
            Tool::Stairs { dir } => {
                *dir = match dir {
                    Dir4::PosZ => Dir4::PosX,
                    Dir4::PosX => Dir4::NegZ,
                    Dir4::NegZ => Dir4::NegX,
                    Dir4::NegX => Dir4::PosZ,
                }
            }
            _ => {}
        }
    }

    /// Mouse press on the world with the current tool.
    pub fn tool_press(&mut self, p: Vec3) {
        let before = self.funds();
        self.tool_press_inner(p);
        self.build_sfx(before);
    }

    pub fn tool_release(&mut self, p: Vec3) {
        let before = self.funds();
        self.tool_release_inner(p);
        self.build_sfx(before);
    }

    fn funds(&self) -> i64 {
        self.sim.world.res::<sim::economy::Household>().funds
    }

    /// Build/Buy feedback: anything that cost money thunks into place, refunds crumble.
    fn build_sfx(&mut self, before: i64) {
        let after = self.funds();
        if after < before {
            self.sfx.push(if matches!(self.tool, Tool::Place { .. }) { "place" } else { "build_wall" });
        } else if after > before {
            self.sfx.push("demolish");
        }
    }

    fn tool_press_inner(&mut self, p: Vec3) {
        let tile = Lot::tile_of(p);
        let corner = IVec2::new(p.x.round() as i32, p.z.round() as i32);
        match self.tool.clone() {
            Tool::Wall | Tool::DeleteWall | Tool::Room => self.drag_start = Some(corner),
            Tool::Floor | Tool::DeleteFloor | Tool::Basement | Tool::Roof { .. } => self.drag_start = Some(tile),
            Tool::Terrain { .. } => {
                self.sculpt_at(p);
                self.sculpting = Some(self.time);
            }
            Tool::DeleteRoof => {
                let n = self.sim.lot_mut().remove_roof_at(Some(self.level), tile);
                if n == 0 {
                    self.say("No roof on this level there (roofs sit on top of the level you're viewing).");
                }
            }
            Tool::Place { def, rot } => match self.sim.place_object(&def, self.level, tile, rot, true) {
                Ok(_) => {
                    let price = self.sim.data().object_id(&def).map(|i| self.sim.data().object(i).price).unwrap_or(0);
                    self.say(format!("Bought for ${price}"));
                }
                Err(e) => self.say(format!("Can't place: {e}")),
            },
            Tool::Door | Tool::Window => {
                let e = nearest_edge(self.level, p);
                let (kind, cost) = if self.tool == Tool::Door { (Opening::Door, COST_DOOR) } else { (Opening::Window, COST_WINDOW) };
                if !self.sim.world.res::<Lot>().walls.contains_key(&e) {
                    self.say("Doors and windows go on walls.");
                } else if !self.sim.charge(cost) {
                    self.say("Can't afford it.");
                } else {
                    self.sim.lot_mut().set_opening(e, kind);
                }
            }
            Tool::Stairs { dir } => {
                if !self.sim.charge(COST_STAIRS) {
                    self.say("Can't afford stairs.");
                    return;
                }
                let r = self.sim.lot_mut().add_stair(Stair { level: self.level, start: tile, dir });
                if let Err(e) = r {
                    self.sim.world.res_mut::<sim::economy::Household>().funds += COST_STAIRS;
                    self.say(format!("Can't build stairs: {e}"));
                }
            }
            Tool::Sell => {}
            Tool::None => {}
        }
    }

    /// Mouse release: finish drag tools.
    fn tool_release_inner(&mut self, p: Vec3) {
        self.sculpting = None;
        let Some(start) = self.drag_start.take() else { return };
        let tile = Lot::tile_of(p);
        let corner = IVec2::new(p.x.round() as i32, p.z.round() as i32);
        let lvl = self.level;
        match self.tool {
            Tool::Wall => {
                let edges = line_edges(lvl, start, corner);
                let new: Vec<Edge> = edges.into_iter().filter(|e| !self.sim.world.res::<Lot>().walls.contains_key(e)).collect();
                let cost = new.len() as i64 * COST_WALL;
                if !self.sim.charge(cost) {
                    self.say("Can't afford those walls.");
                    return;
                }
                let style = self.wall_style;
                let mut lot = self.sim.lot_mut();
                for e in new {
                    lot.add_wall(e, style);
                }
            }
            Tool::DeleteWall => {
                let edges = line_edges(lvl, start, corner);
                let mut lot = self.sim.lot_mut();
                let mut n = 0;
                for e in edges {
                    n += lot.remove_wall(e) as i64;
                }
                drop(lot);
                self.sim.world.res_mut::<sim::economy::Household>().funds += n * COST_WALL / 2;
            }
            Tool::Room => {
                let (min, max) = (start.min(corner), start.max(corner));
                if max.x <= min.x || max.y <= min.y {
                    return;
                }
                let tiles = ((max.x - min.x) * (max.y - min.y)) as i64;
                let perim = 2 * ((max.x - min.x) + (max.y - min.y)) as i64;
                if !self.sim.charge(tiles * COST_FLOOR + perim * COST_WALL) {
                    self.say("Can't afford that room.");
                    return;
                }
                let (ws, fs) = (self.wall_style, self.floor_style);
                let mut lot = self.sim.lot_mut();
                lot.wall_rect(lvl, min, max, ws);
                lot.floor_rect(lvl, min, max, fs);
            }
            Tool::Floor | Tool::DeleteFloor => {
                let (min, max) = (start.min(tile), start.max(tile) + IVec2::ONE);
                let n = ((max.x - min.x) * (max.y - min.y)) as i64;
                if self.tool == Tool::Floor {
                    if !self.sim.charge(n * COST_FLOOR) {
                        self.say("Can't afford that floor.");
                        return;
                    }
                    let fs = self.floor_style;
                    self.sim.lot_mut().floor_rect(lvl, min, max, fs);
                } else {
                    let mut lot = self.sim.lot_mut();
                    for x in min.x..max.x {
                        for z in min.y..max.y {
                            if lot.occupancy.contains_key(&(lvl, x, z)) {
                                continue;
                            }
                            lot.remove_floor(lvl, IVec2::new(x, z));
                        }
                    }
                }
            }
            Tool::Basement => {
                if lvl >= 0 {
                    self.say("Switch to a basement level (Page Down) to dig.");
                    return;
                }
                let (min, max) = (start.min(tile), start.max(tile) + IVec2::ONE);
                let n = ((max.x - min.x) * (max.y - min.y)) as i64;
                if !self.sim.charge(n * COST_DIG) {
                    self.say("Can't afford to dig that much.");
                    return;
                }
                let (ws, fs) = (self.wall_style, self.floor_style);
                self.sim.lot_mut().dig_basement(lvl, min, max, ws, fs);
            }
            Tool::Roof { kind } => {
                let (min, max) = (start.min(tile), start.max(tile) + IVec2::ONE);
                let n = ((max.x - min.x) * (max.y - min.y)) as i64;
                if !self.sim.charge(n * COST_ROOF_TILE) {
                    self.say("Can't afford that roof.");
                    return;
                }
                let pitch = self.roof_pitch;
                let r = self.sim.lot_mut().add_roof(Roof { level: lvl, min, max, kind, pitch, style: 0 });
                if let Err(e) = r {
                    self.sim.world.res_mut::<sim::economy::Household>().funds += n * COST_ROOF_TILE;
                    self.say(format!("Can't build roof: {e}"));
                }
            }
            _ => {}
        }
    }

    /// Opens a pie menu for the selected peon on whatever was clicked.
    pub fn open_menu(&mut self, pick: &Pick, screen: (f32, f32)) {
        let Some(peon) = self.selected else { return };
        match pick {
            Pick::Peon(e) if *e == peon => {}
            Pick::Peon(e) | Pick::Object(e) => {
                let options = self.sim.options(peon, *e);
                let title = match pick {
                    Pick::Peon(_) => self.sim.name(*e),
                    _ => self
                        .sim
                        .world
                        .get::<ObjectInstance>(*e)
                        .map(|o| self.sim.data().object(o.def).name.clone())
                        .unwrap_or_default(),
                };
                self.menu = Some(PieMenu { peon, target: Some(*e), title, screen, options });
            }
            Pick::Floor { level, pos } => {
                let tile = Lot::tile_of(*pos);
                self.menu = Some(PieMenu {
                    peon,
                    target: None,
                    title: "Here".into(),
                    screen,
                    options: vec![("Go Here".into(), ActionKind::GoTo { level: *level, tile }, Ok(()))],
                });
            }
        }
    }
}

pub fn line_edges(level: i32, a: IVec2, b: IVec2) -> Vec<Edge> {
    let d = b - a;
    let mut out = Vec::new();
    if d.x.abs() >= d.y.abs() {
        for x in a.x.min(b.x)..a.x.max(b.x) {
            out.push(Edge::new(level, x, a.y, Axis::X));
        }
    } else {
        for z in a.y.min(b.y)..a.y.max(b.y) {
            out.push(Edge::new(level, a.x, z, Axis::Z));
        }
    }
    out
}

pub fn nearest_edge(level: i32, p: Vec3) -> Edge {
    let ex = Edge::new(level, p.x.floor() as i32, p.z.round() as i32, Axis::X);
    let ez = Edge::new(level, p.x.round() as i32, p.z.floor() as i32, Axis::Z);
    let dx = (p.z - p.z.round()).abs();
    let dz = (p.x - p.x.round()).abs();
    if dx < dz { ex } else { ez }
}

/// Parts thinner than this (rugs, shelves, pictures) are picked as if this thick.
const MIN_PICK: f32 = 0.08;

/// Ray distance to the nearest of an object's parts: each part's box, oriented like the drawn part
/// (round parts use their bounding box, which is forgiving to click). Empty space within the
/// footprint, under a table or beside a headboard, doesn't count.
pub fn ray_object(o: Vec3, d: Vec3, def: &sim::data::ObjectDef, inst: &ObjectInstance) -> Option<f32> {
    let rot = glam::Quat::from_rotation_y(inst.yaw());
    def.parts
        .iter()
        .filter_map(|p| {
            // same transform as `scene::object_draws`; the ray parameter is unchanged by it
            let m = glam::Mat4::from_scale_rotation_translation(p.size.max(Vec3::splat(MIN_PICK)), rot, inst.local_to_world(def, p.at));
            let inv = m.inverse();
            ray_aabb(inv.transform_point3(o), inv.transform_vector3(d), Vec3::splat(-0.5), Vec3::splat(0.5))
        })
        .min_by(f32::total_cmp)
}

/// Ray distance to a peon's body: a sphere around a standing body's middle, lower for a seated one
/// (the origin is on the seat), and a row of spheres along a lying body (centred on the origin,
/// head towards the model's -z).
pub fn ray_peon(o: Vec3, d: Vec3, tf: &Transform, height: f32, pose: sim::data::Pose) -> Option<f32> {
    use sim::data::Pose;
    match pose {
        Pose::Lie => {
            let head = glam::Quat::from_rotation_y(tf.yaw) * Vec3::NEG_Z;
            let c = tf.pos + Vec3::Y * 0.12;
            let r = 0.12 * height + 0.12;
            [-0.33, 0.0, 0.33].into_iter().filter_map(|k| ray_sphere(o, d, c + head * (k * height), r)).min_by(f32::total_cmp)
        }
        Pose::Sit => ray_sphere(o, d, tf.pos + Vec3::Y * (height * 0.3), 0.4),
        _ => ray_sphere(o, d, tf.pos + Vec3::Y * (height * 0.5), 0.45),
    }
}

fn ray_sphere(o: Vec3, d: Vec3, c: Vec3, r: f32) -> Option<f32> {
    let oc = o - c;
    let b = oc.dot(d);
    let cc = oc.dot(oc) - r * r;
    let disc = b * b - cc;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    (t > 0.0).then_some(t)
}

fn ray_aabb(o: Vec3, d: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = d.recip();
    let t1 = (min - o) * inv;
    let t2 = (max - o) * inv;
    let tmin = t1.min(t2).max_element();
    let tmax = t1.max(t2).min_element();
    (tmax >= tmin.max(0.0)).then_some(tmin.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::data::{GameData, Pose};

    fn placed(data: &GameData, id: &str, tile: (i32, i32), rot: u8) -> (sim::data::ObjectDef, ObjectInstance) {
        let def_id = data.object_id(id).unwrap();
        let def = data.object(def_id).clone();
        let inst = ObjectInstance::new(def_id, &def, 0, IVec2::new(tile.0, tile.1), rot);
        (def, inst)
    }

    #[test]
    fn objects_are_picked_by_their_parts_not_their_footprint_box() {
        let data = GameData::embedded();
        let (table, t) = placed(&data, "dining_table", (0, 0), 0);
        // straight down onto the top (5 cm thick at 0.75, padded to MIN_PICK)
        let hit = ray_object(Vec3::new(1.0, 5.0, 0.5), Vec3::NEG_Y, &table, &t).unwrap();
        assert!((hit - (5.0 - 0.75 - MIN_PICK * 0.5)).abs() < 0.01, "table top at t={hit}");
        // level with the legs, between them: nothing there
        assert_eq!(ray_object(Vec3::new(-3.0, 0.4, 0.5), Vec3::X, &table, &t), None);

        // rotated a quarter turn the table spans 1x2 tiles
        let (_, t1) = placed(&data, "dining_table", (0, 0), 1);
        assert!(ray_object(Vec3::new(0.5, 5.0, 1.0), Vec3::NEG_Y, &table, &t1).is_some());
        assert_eq!(ray_object(Vec3::new(1.5, 5.0, 0.5), Vec3::NEG_Y, &table, &t1), None);

        // a double bed is only headboard-tall at the head end
        let (bed, b) = placed(&data, "bed_double", (0, 0), 0);
        assert!(ray_object(Vec3::new(-3.0, 0.9, 0.03), Vec3::X, &bed, &b).is_some());
        assert_eq!(ray_object(Vec3::new(-3.0, 0.9, 1.8), Vec3::X, &bed, &b), None);

        // a rug is a hair thick but still clickable (in buy mode)
        let (rug, r) = placed(&data, "rug", (4, 4), 0);
        assert!(ray_object(Vec3::new(5.0, 3.0, 5.0), Vec3::NEG_Y, &rug, &r).is_some());
    }

    #[test]
    fn peons_lying_down_are_picked_where_the_body_is() {
        let tf = Transform { pos: Vec3::new(1.0, 0.5, 1.0), yaw: 0.0, prev_pos: Vec3::ZERO, prev_yaw: 0.0 };
        // looking straight down at the shins of someone asleep in bed (head towards -z)
        let shins = Vec3::new(1.0, 4.0, 1.0 + 0.55);
        assert!(ray_peon(shins, Vec3::NEG_Y, &tf, 1.75, Pose::Lie).is_some());
        // the standing sphere would float over the pillow instead
        assert!(ray_peon(shins, Vec3::NEG_Y, &tf, 1.75, Pose::Stand).is_none());
        // and a lying body doesn't reach up there
        assert!(ray_peon(Vec3::new(-3.0, 1.4, 1.0), Vec3::X, &tf, 1.75, Pose::Lie).is_none());
    }
}
