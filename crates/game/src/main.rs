//! `lifesim` — the playable game.
//!
//! Controls: RMB-drag rotate · MMB-drag / WASD pan · wheel zoom · Q/E rotate · PgUp/PgDn level ·
//! Space pause · 1–4 speed · F5/F6/F7 Live/Buy/Build · V wall mode · R rotate tool · Tab next peon ·
//! Home focus · L story log · F1 inspector · F2 autonomy · F3 overlays · F4 profiler · F8 settings ·
//! F12 screenshot · Esc cancel.
//!
//! Ctrl+S quick save · Ctrl+O quick load · ☰ Game → Saves… · `--continue` resumes the newest save · T roofs · M mute.
//!
//! Test hooks: `--seed N --minutes M --screenshot-after FRAMES PATH --mode live|buy|build --level L
//! --debug --size WxH --cam YAW,PITCH,DIST --menu-on OBJECT_ID --speed I --load SAVE --creator --walls up|cutaway|down --show saves|neighborhood`

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use game::scene::{self, SceneOptions};
use game::state::*;
use game::ui;
use glam::Vec3;
use hearth_render::{Gpu, Renderer, wgpu};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::raw_window_handle::HasDisplayHandle;
use winit::window::{Window, WindowId};

/// The OS clipboard (Wayland or X11) for hearth_ui text fields, via egui-winit's wrapper.
struct SystemClipboard(egui_winit::clipboard::Clipboard);

impl hearth_ui::Clipboard for SystemClipboard {
    fn get(&mut self) -> Option<String> {
        self.0.get()
    }
    fn set(&mut self, text: String) {
        self.0.set_text(text);
    }
}

struct Args {
    seed: u64,
    minutes: u64,
    screenshot_after: Option<(u32, PathBuf)>,
    mode: Mode,
    level: i32,
    debug: bool,
    size: (u32, u32),
    cam: Option<(f32, f32, f32)>,
    menu_on: Option<String>,
    speed: usize,
    load: Option<PathBuf>,
    /// Resume the newest save in `saves/`.
    resume: bool,
    creator: bool,
    walls: Option<scene::WallMode>,
    show: Option<String>,
}

fn parse_args() -> Args {
    let mut a = Args { seed: 7, minutes: 0, screenshot_after: None, mode: Mode::Live, level: 0, debug: false, size: (1600, 960), cam: None, menu_on: None, speed: 0, load: None, resume: false, creator: false, walls: None, show: None };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--seed" => a.seed = it.next().unwrap().parse().unwrap(),
            "--minutes" => a.minutes = it.next().unwrap().parse().unwrap(),
            "--screenshot-after" => {
                let n = it.next().unwrap().parse().unwrap();
                a.screenshot_after = Some((n, PathBuf::from(it.next().unwrap())));
            }
            "--mode" => {
                a.mode = match it.next().unwrap().as_str() {
                    "buy" => Mode::Buy,
                    "build" => Mode::Build,
                    _ => Mode::Live,
                }
            }
            "--level" => a.level = it.next().unwrap().parse().unwrap(),
            "--debug" => a.debug = true,
            "--size" => {
                let v: Vec<u32> = it.next().unwrap().split('x').map(|s| s.parse().unwrap()).collect();
                a.size = (v[0], v[1]);
            }
            "--cam" => {
                let v: Vec<f32> = it.next().unwrap().split(',').map(|s| s.parse().unwrap()).collect();
                a.cam = Some((v[0], v[1], v[2]));
            }
            "--menu-on" => a.menu_on = it.next(),
            "--speed" => a.speed = it.next().unwrap().parse().unwrap(),
            "--load" => a.load = it.next().map(PathBuf::from),
            "--continue" => a.resume = true,
            "--creator" => a.creator = true,
            "--show" => a.show = it.next(),
            "--walls" => {
                a.walls = Some(match it.next().unwrap().as_str() {
                    "up" => scene::WallMode::Up,
                    "down" => scene::WallMode::Down,
                    _ => scene::WallMode::Cutaway,
                })
            }
            "--help" | "-h" => {
                println!("{}", include_str!("main.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    a
}

// Field order matters: GPU resources must drop before the window they present to.
struct Gfx {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    /// The game's interface.
    ui: hearth_ui::Ui,
    ui_renderer: hearth_ui::Renderer,
    /// Developer windows only (F1–F4, F8, L).
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    window: Arc<Window>,
    meshes: game::lotmesh::LotMeshes,
    /// Rigged peon bodies; None falls back to greybox bodies.
    peons: Option<game::peons::PeonArt>,
    /// Per-peon animation playback (clip cross-fades).
    anims: game::peons::Animator,
}

struct App {
    args: Args,
    game: Game,
    gfx: Option<Gfx>,
    cursor: (f32, f32),
    keys: HashSet<KeyCode>,
    rmb: bool,
    mmb: bool,
    lmb_down_at: Option<(f32, f32)>,
    last: Instant,
    frames: u32,
    audio: game::audio::Audio,
    /// Seconds until changed volumes are written to `settings.ron` (debounces slider drags).
    volumes_save_in: Option<f32>,
    /// Seconds until each walking peon's next footstep.
    steps: HashMap<sim::Entity, f32>,
    /// When each one-shot last played (app seconds), so bursts don't stack.
    last_sfx: HashMap<&'static str, f32>,
    clock: f32,
    jukebox: game::sounds::Jukebox,
}

impl App {
    /// Interaction loops, footsteps and queued one-shots (Build/Buy, notifications).
    fn world_sounds(&mut self, dt: f32, eye: glam::Vec3) {
        use game::sounds;
        use sim::components::{AtWork, Offlot, Posture, Transform};
        self.clock += dt;
        let speed = SPEEDS[self.game.speed].0;
        let w = &self.game.sim.world;
        let mut loops: Vec<(u64, game::audio::Clip, f32)> = Vec::new();
        let mut steps = Vec::new();
        for (e, tf, posture) in w.query_filtered::<(sim::Entity, &Transform, &Posture), (hearth_ecs::Without<AtWork>, hearth_ecs::Without<Offlot>)>().iter() {
            if tf.level() > self.game.level {
                continue;
            }
            let dist = (tf.pos - eye).length();
            if posture.walking {
                let gain = sounds::falloff(dist);
                if gain > 0.0 {
                    steps.push((e, gain));
                }
            } else if let Some(id) = sounds::loop_for(posture.sfx.as_deref(), posture.anim.as_deref()) {
                let gain = sounds::loop_gain(id, dist);
                if gain <= 0.0 {
                    continue;
                }
                // one voice per sound, at the loudest source (three people watching one TV)
                let key = id.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64));
                match loops.iter_mut().find(|l| l.0 == key) {
                    Some(l) => l.2 = l.2.max(gain),
                    None => loops.extend(sounds::clip(id).map(|c| (key, c, gain))),
                }
            }
        }
        self.audio.update_loops(dt, &loops);

        let scene = if self.game.creator.is_some() {
            sounds::Scene::Creator
        } else if matches!(self.game.mode, Mode::Build | Mode::Buy) {
            sounds::Scene::Build
        } else {
            let h = w.res::<sim::time::Clock>().hour();
            if !(6..22).contains(&h) { sounds::Scene::Night } else { sounds::Scene::Day }
        };
        let track = self.jukebox.update(dt, scene).and_then(sounds::track);
        self.audio.update_music(dt, track.as_ref());

        // footsteps: a step every 0.42 s of game-speed time, only at 1x/2x so ultra isn't a stampede
        self.steps.retain(|e, _| steps.iter().any(|s| s.0 == *e));
        if speed <= 3.0 && !self.game.paused {
            let mut budget = 4;
            for (e, gain) in steps {
                let t = self.steps.entry(e).or_insert((e.to_bits() % 7) as f32 * 0.06);
                *t -= dt * speed;
                if *t <= 0.0 {
                    *t += 0.42;
                    if budget > 0 {
                        budget -= 1;
                        let id = if e.to_bits() % 2 == 0 { "step_a" } else { "step_b" };
                        if let Some(c) = sounds::clip(id) {
                            self.audio.play(&c, gain * 0.35);
                        }
                    }
                }
            }
        }

        for id in std::mem::take(&mut self.game.sfx) {
            let gap = if matches!(id, "notify" | "cash") { 0.6 } else { 0.08 };
            if self.last_sfx.get(id).is_some_and(|t| self.clock - t < gap) {
                continue;
            }
            self.last_sfx.insert(id, self.clock);
            if let Some(c) = sounds::clip(id) {
                self.audio.play(&c, 1.0);
            }
        }
    }

    fn new(args: Args) -> App {
        let load = args.load.clone().or_else(|| {
            if !args.resume {
                return None;
            }
            let newest = game::saves::list().into_iter().next();
            if newest.is_none() {
                eprintln!("--continue: no saves found, starting a new game");
            }
            newest.map(|s| s.path)
        });
        let mut sim = match &load {
            Some(p) => sim::Sim::load_from_file(p, sim::data::GameData::embedded()).unwrap_or_else(|e| {
                eprintln!("couldn't load {}: {e}", p.display());
                std::process::exit(1)
            }),
            None => {
                let mut s = sim::starter::starter_sim(args.seed);
                s.populate_neighborhood();
                s
            }
        };
        sim.run_minutes(args.minutes);
        let mut game = Game::new(sim);
        if args.creator {
            game.open_creator();
            game.cam.snap();
        }
        match args.show.as_deref() {
            Some("saves") => game.show_saves = true,
            Some("neighborhood") => game.show_neighborhood = true,
            _ => {}
        }
        if let Some(w) = args.walls {
            game.wall_mode = w;
        }
        game.mode = args.mode;
        game.level = args.level;
        game.speed = args.speed.min(SPEEDS.len() - 1);
        if args.debug {
            game.debug.inspector = true;
            game.debug.autonomy = true;
            game.debug.profiler = true;
            game.inspect_entity = game.selected;
        }
        if let Some((y, p, d)) = args.cam {
            game.cam.yaw = y.to_radians();
            game.cam.pitch = p.to_radians();
            game.cam.dist = d;
            game.cam.snap();
        }
        game.log_seen = game.sim.world.res::<sim::log::SimLog>().total;
        // automated captures stay silent
        let audio = if args.screenshot_after.is_some() {
            game::audio::Audio::disabled()
        } else {
            game.volumes = game::audio::Volumes::load();
            game::audio::Audio::new()
        };
        App { args, game, gfx: None, cursor: (0.0, 0.0), keys: HashSet::new(), rmb: false, mmb: false, lmb_down_at: None, last: Instant::now(), frames: 0, audio, volumes_save_in: None, steps: HashMap::new(), last_sfx: HashMap::new(), clock: 0.0, jukebox: Default::default() }
    }

    fn ndc(&self) -> Option<(f32, f32)> {
        let gfx = self.gfx.as_ref()?;
        let (w, h) = (gfx.config.width as f32, gfx.config.height as f32);
        Some((self.cursor.0 / w * 2.0 - 1.0, 1.0 - self.cursor.1 / h * 2.0))
    }

    fn aspect(&self) -> f32 {
        self.gfx.as_ref().map(|g| g.config.width as f32 / g.config.height.max(1) as f32).unwrap_or(1.6)
    }

    fn on_key(&mut self, key: KeyCode, pressed: bool, event_loop: &ActiveEventLoop) {
        if pressed {
            self.keys.insert(key);
        } else {
            self.keys.remove(&key);
            return;
        }
        let ctrl = self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight);
        let g = &mut self.game;
        if ctrl {
            match key {
                KeyCode::KeyS => g.save_to(std::path::Path::new(QUICKSAVE)),
                KeyCode::KeyO => g.load_from(std::path::Path::new(QUICKSAVE)),
                _ => {}
            }
            return;
        }
        match key {
            KeyCode::Space => g.paused = !g.paused,
            KeyCode::KeyT => g.show_roofs = !g.show_roofs,
            KeyCode::KeyM => g.mute = !g.mute,
            KeyCode::Digit1 => (g.speed, g.paused) = (0, false),
            KeyCode::Digit2 => (g.speed, g.paused) = (1, false),
            KeyCode::Digit3 => (g.speed, g.paused) = (2, false),
            KeyCode::Digit4 => (g.speed, g.paused) = (3, false),
            KeyCode::F1 => g.debug.inspector = !g.debug.inspector,
            KeyCode::F2 => g.debug.autonomy = !g.debug.autonomy,
            KeyCode::F3 => g.debug.overlays = !g.debug.overlays,
            KeyCode::F4 => g.debug.profiler = !g.debug.profiler,
            KeyCode::F8 => g.debug.settings = !g.debug.settings,
            KeyCode::KeyL => g.debug.log = !g.debug.log,
            KeyCode::F5 => (g.mode, g.tool) = (Mode::Live, Tool::None),
            KeyCode::F6 | KeyCode::F7 if !g.can_build() => g.say("You can only build and buy on your own lot."),
            KeyCode::F6 => (g.mode, g.tool) = (Mode::Buy, Tool::None),
            KeyCode::F7 => (g.mode, g.tool) = (Mode::Build, Tool::None),
            KeyCode::PageUp => g.set_level(g.level + 1),
            KeyCode::PageDown => g.set_level(g.level - 1),
            KeyCode::KeyV => {
                g.wall_mode = match g.wall_mode {
                    scene::WallMode::Up => scene::WallMode::Cutaway,
                    scene::WallMode::Cutaway => scene::WallMode::Down,
                    scene::WallMode::Down => scene::WallMode::Up,
                }
            }
            KeyCode::KeyR => g.rotate_tool(),
            KeyCode::Escape => {
                if g.creator.is_some() {
                    g.creator = None;
                } else if g.menu.is_some() {
                    g.menu = None;
                } else if g.tool != Tool::None {
                    g.tool = Tool::None;
                    g.drag_start = None;
                } else if g.mode != Mode::Live {
                    g.mode = Mode::Live;
                }
            }
            KeyCode::Tab => {
                let peons = g.sim.living_peons();
                if !peons.is_empty() {
                    let i = g.selected.and_then(|s| peons.iter().position(|p| *p == s)).map(|i| (i + 1) % peons.len()).unwrap_or(0);
                    g.selected = Some(peons[i]);
                    g.focus_selected();
                }
            }
            KeyCode::Home => g.focus_selected(),
            KeyCode::Delete => {
                if g.mode == Mode::Buy
                    && let Some(Pick::Object(e)) = g.hover_pick.clone()
                        && let Some(v) = g.sim.sell_object(e) {
                            g.say(format!("Sold for ${v}"));
                            g.sfx.push("sell");
                        }
            }
            KeyCode::F12 => {
                let _ = std::fs::create_dir_all("screenshots");
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                g.screenshot = Some(PathBuf::from(format!("screenshots/lifesim-{t}.png")));
            }
            KeyCode::F10 => {
                if g.creator.is_none() {
                    g.autosave();
                }
                event_loop.exit()
            }
            _ => {}
        }
    }

    fn on_click(&mut self, button: MouseButton, pressed: bool) {
        let Some(ndc) = self.ndc() else { return };
        let cam = self.game.camera(self.aspect());
        let g = &mut self.game;
        match (g.mode, button, pressed) {
            (Mode::Live, MouseButton::Left, false) => {
                // treat as click only if the mouse didn't drag
                let Some(start) = self.lmb_down_at.take() else { return };
                if (start.0 - self.cursor.0).abs() + (start.1 - self.cursor.1).abs() > 6.0 {
                    return;
                }
                g.menu = None;
                let Some(pick) = g.pick(&cam, ndc) else { return };
                if let Pick::Peon(e) = pick
                    && g.selected != Some(e) && g.sim.world.has::<sim::components::Member>(e) && g.selected.is_none_or(|_| true) {
                        // clicking a different household member: select (Shift+click would interact)
                        if !self.keys.contains(&KeyCode::ShiftLeft) {
                            g.selected = Some(e);
                            return;
                        }
                    }
                g.open_menu(&pick, self.cursor);
            }
            (Mode::Live, MouseButton::Left, true) => self.lmb_down_at = Some(self.cursor),
            (Mode::Buy, MouseButton::Left, true) => match g.tool.clone() {
                Tool::Sell => {
                    if let Some(Pick::Object(e)) = g.pick(&cam, ndc)
                        && let Some(v) = g.sim.sell_object(e) {
                            g.say(format!("Sold for ${v}"));
                            g.sfx.push("sell");
                        }
                }
                Tool::Place { .. } => {
                    if let Some(p) = g.floor_point(&cam, ndc) {
                        g.tool_press(p);
                    }
                }
                _ => {}
            },
            (Mode::Build, MouseButton::Left, true) => {
                if let Some(p) = g.floor_point(&cam, ndc) {
                    g.tool_press(p);
                }
            }
            (Mode::Build, MouseButton::Left, false) => {
                if let Some(p) = g.floor_point(&cam, ndc) {
                    g.tool_release(p);
                }
            }
            _ => {}
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.frames += 1;
        // continuous keys
        {
            let g = &mut self.game;
            let k = |c: KeyCode| self.keys.contains(&c);
            let pan = dt * 12.0;
            if k(KeyCode::KeyW) || k(KeyCode::ArrowUp) {
                g.cam.pan(0.0, pan);
            }
            if k(KeyCode::KeyS) || k(KeyCode::ArrowDown) {
                g.cam.pan(0.0, -pan);
            }
            if k(KeyCode::KeyA) || k(KeyCode::ArrowLeft) {
                g.cam.pan(-pan, 0.0);
            }
            if k(KeyCode::KeyD) || k(KeyCode::ArrowRight) {
                g.cam.pan(pan, 0.0);
            }
            if k(KeyCode::KeyQ) {
                g.cam.rotate(-dt * 1.5, 0.0);
            }
            if k(KeyCode::KeyE) {
                g.cam.rotate(dt * 1.5, 0.0);
            }
            if k(KeyCode::Equal) {
                g.cam.zoom(1.0 - dt);
            }
            if k(KeyCode::Minus) {
                g.cam.zoom(1.0 + dt);
            }
        }
        self.game.update(dt);
        if self.args.screenshot_after.is_none() {
            self.game.autosave_if_new_day();
        }
        // terrain brush repeats while the mouse is held
        if let (Some(last), Some(n)) = (self.game.sculpting, self.ndc())
            && self.game.time - last > 0.08 {
                let cam = self.game.camera(self.aspect());
                if let Some(p) = self.game.floor_point(&cam, n) {
                    self.game.sculpt_at(p);
                }
                self.game.sculpting = Some(self.game.time);
            }

        let aspect = self.aspect();
        let ndc = self.ndc();
        let cam = self.game.camera(aspect);
        // hover
        let over_ui = self.gfx.as_ref().is_some_and(|g| g.egui_ctx.is_pointer_over_egui() || g.ui.wants_pointer());
        let pick = if over_ui { None } else { ndc.and_then(|n| self.game.pick(&cam, n)) };
        self.game.hovered = match &pick {
            Some(Pick::Peon(e)) | Some(Pick::Object(e)) => Some(*e),
            _ => None,
        };
        self.game.hover_pick = pick;
        let cursor_floor = if over_ui { None } else { ndc.and_then(|n| self.game.floor_point(&cam, n)) };

        // voices for visible speakers (louder when the camera is close)
        {
            self.audio.muted = self.game.mute;
            self.audio.paused = self.game.paused;
            if self.audio.available() && self.audio.volumes != self.game.volumes {
                self.volumes_save_in = Some(1.0);
            }
            self.audio.volumes = self.game.volumes;
            if let Some(t) = self.volumes_save_in.as_mut() {
                *t -= dt;
                if *t <= 0.0 {
                    self.game.volumes.save();
                    self.volumes_save_in = None;
                }
            }
            let w = &self.game.sim.world;
            let eye = cam.eye;
            let mut speakers = Vec::new();
            for (e, tf, age, b) in w.query_filtered::<(sim::Entity, &sim::components::Transform, &sim::components::Age, &sim::components::Bubble), (hearth_ecs::Without<sim::components::AtWork>, hearth_ecs::Without<sim::components::Offlot>)>().iter() {
                let Some(line) = &b.speech else { continue };
                if tf.level() > self.game.level {
                    continue;
                }
                let dist = (tf.pos - eye).length();
                let loud = (1.0 - (dist - 8.0) / 40.0).clamp(0.0, 1.0);
                speakers.push((e, age.stage, line.clone(), loud));
            }
            speakers.sort_by_key(|s| s.0);
            let fast = SPEEDS[self.game.speed].0 > 10.0;
            if fast {
                speakers.clear();
            }
            self.audio.update(dt, &speakers);
            self.world_sounds(dt, eye);
        }

        if let Some(gfx) = self.gfx.as_mut() {
            gfx.meshes.sync(&mut gfx.renderer, &self.game.sim.world.res::<sim::lot::Lot>());
        }
        let (terrain_mesh, roof_meshes) = self.gfx.as_ref().map(|g| (g.meshes.terrain, g.meshes.roofs.clone())).unwrap_or_default();
        let preview = self.game.creator.as_ref().map(|c| c.preview()).unwrap_or_default();
        let opts = SceneOptions {
            terrain_mesh,
            roof_meshes,
            show_roofs: self.game.show_roofs,
            preview,
            active_level: self.game.level,
            wall_mode: self.game.wall_mode,
            alpha: self.game.alpha,
            time: self.game.time,
            selected: self.game.selected,
            hovered: self.game.hovered,
            ghosts: if self.game.mode == Mode::Live { Vec::new() } else { self.game.ghosts(cursor_floor) },
            show_rooms: self.game.debug.overlays,
            show_paths: self.game.debug.overlays,
            focus: self.game.cam.focus(self.game.level),
        };
        let Some(gfx) = self.gfx.as_mut() else { return };
        let mut out = scene::build(&self.game.sim, &gfx.renderer.builtins, gfx.peons.as_ref(), &mut gfx.anims, cam, &opts);
        out.frame.lighting = self.game.graphics.lighting();

        // UI
        let size_px = [gfx.config.width, gfx.config.height];
        let cx = ui::UiCtx { camera: &cam, labels: &out.labels, screen: (size_px[0] as f32, size_px[1] as f32) };
        gfx.ui.begin_frame(dt, size_px);
        ui::draw(&mut gfx.ui, &mut self.game, &cx);
        gfx.ui.end_frame();
        if gfx.ui.clicks > 0 {
            self.game.sfx.push("ui_click");
        }
        gfx.ui_renderer.prepare(&gfx.renderer.gpu.device, &gfx.renderer.gpu.queue, &mut gfx.ui, size_px);
        // developer windows
        let raw = gfx.egui_state.take_egui_input(&gfx.window);
        let dcx = game::debug_ui::DebugCtx { render_stats: gfx.renderer.stats };
        let game = &mut self.game;
        let mut full = gfx.egui_ctx.run_ui(raw, |ui| {
            if game::debug_ui::any_open(game) {
                game::debug_ui::draw(ui, game, &dcx);
            }
        });
        gfx.egui_state.handle_platform_output(&gfx.window, full.platform_output);
        let prims = gfx.egui_ctx.tessellate(full.shapes, full.pixels_per_point);
        let (dev, q) = (&gfx.renderer.gpu.device.clone(), &gfx.renderer.gpu.queue.clone());
        for (id, deltas) in &full.textures_delta.set {
            for delta in deltas.iter() {
                gfx.egui_renderer.update_texture(dev, q, *id, delta);
            }
        }
        let freed: Vec<egui::TextureId> = full.textures_delta.free.iter().copied().collect();
        full.textures_delta.clear();
        let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [gfx.config.width, gfx.config.height], pixels_per_point: full.pixels_per_point };

        let draw_to = |gfx: &mut Gfx, tex: &wgpu::Texture| {
            let srgb = gfx.renderer.format;
            let view3d = tex.create_view(&wgpu::TextureViewDescriptor { format: Some(srgb), ..Default::default() });
            let view = &tex.create_view(&wgpu::TextureViewDescriptor { format: Some(srgb.remove_srgb_suffix()), ..Default::default() });
            gfx.renderer.render(&out.frame, &view3d, sd.size_in_pixels[0], sd.size_in_pixels[1]);
            let mut enc = dev.create_command_encoder(&Default::default());
            let extra = gfx.egui_renderer.update_buffers(dev, q, &mut enc, &prims, &sd);
            {
                let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                let mut pass = pass.forget_lifetime();
                gfx.ui_renderer.paint(&mut pass);
                gfx.egui_renderer.render(&mut pass, &prims, &sd);
            }
            q.submit(extra.into_iter().chain([enc.finish()]));
        };

        // screenshot (full UI) if requested, before presenting
        let want_shot = self.game.screenshot.take().or_else(|| match &self.args.screenshot_after {
            Some((n, p)) if self.frames == *n => Some(p.clone()),
            _ => None,
        });
        if let Some(path) = want_shot {
            let tex = gfx.renderer.offscreen_texture(gfx.config.width, gfx.config.height);
            draw_to(gfx, &tex);
            let px = gfx.renderer.readback(&tex, gfx.config.width, gfx.config.height);
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match image::RgbaImage::from_raw(gfx.config.width, gfx.config.height, px).map(|img| img.save(&path)) {
                Some(Ok(())) => {
                    println!("screenshot saved: {}", path.display());
                    self.game.say(format!("Screenshot saved to {}", path.display()));
                }
                other => eprintln!("screenshot failed: {other:?}"),
            }
            if self.args.screenshot_after.as_ref().is_some_and(|(n, _)| self.frames == *n) {
                event_loop.exit();
                return;
            }
        }

        // automated runs don't present: the compositor may never show (and so never pace) the window
        let headless_test = self.args.screenshot_after.is_some();
        match if headless_test { wgpu::CurrentSurfaceTexture::Timeout } else { gfx.surface.get_current_texture() } {
            wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                draw_to(gfx, &frame.texture);
                gfx.window.pre_present_notify();
                q.present(frame);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {}
            _ => {
                gfx.surface.configure(dev, &gfx.config);
            }
        }
        for id in &freed {
            gfx.egui_renderer.free_texture(id);
        }
        self.game.frame_ms = now.elapsed().as_secs_f32() * 1000.0;

        // test hook: open a pie menu on an object after the first frame
        if self.frames == 2
            && let Some(id) = self.args.menu_on.clone() {
                let data = self.game.sim.data();
                let target = self.game.sim.objects().into_iter().find(|e| {
                    self.game.sim.world.get::<sim::objects::ObjectInstance>(*e).is_some_and(|o| data.object(o.def).id == id)
                });
                if let Some(t) = target {
                    let center = {
                        let o = self.game.sim.world.get::<sim::objects::ObjectInstance>(t).unwrap();
                        o.center(data.object(o.def)) + Vec3::Y
                    };
                    let screen = cam.to_screen(center, gfx.config.width as f32, gfx.config.height as f32).unwrap_or((400.0, 300.0));
                    self.game.open_menu(&Pick::Object(t), screen);
                }
            }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Life Sim — Hearth")
            .with_inner_size(winit::dpi::PhysicalSize::new(self.args.size.0, self.args.size.1));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance.create_surface(window.clone()).expect("create surface");
        let gpu = Gpu::with_instance(instance, Some(&surface)).expect("no suitable GPU");
        let size = window.inner_size();
        let caps = surface.get_capabilities(&gpu.adapter);
        // Linear surface format for the UI (which blends in gamma space), with an sRGB view for 3D.
        log::info!("surface formats: {:?}", caps.formats);
        let base = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm]
            .into_iter()
            .find(|f| caps.formats.contains(f))
            .unwrap_or(caps.formats[0].remove_srgb_suffix());
        let srgb = base.add_srgb_suffix();
        let mut config = surface.get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1)).expect("surface config");
        config.format = base;
        config.view_formats = vec![srgb];
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&gpu.device, &config);
        let mut renderer = Renderer::new(gpu.clone(), srgb);
        game::materials::install(&mut renderer, &self.game.sim.data());
        game::object_models::install(&mut renderer, &self.game.sim.data());
        let peons = game::peons::PeonArt::load(&mut renderer).inspect_err(|e| log::error!("peon models: {e:#}; using greybox bodies")).ok();
        let format = base;
        let egui_ctx = egui::Context::default();
        egui_ctx.set_visuals(egui::Visuals::dark());
        let egui_state = egui_winit::State::new(egui_ctx.clone(), egui::ViewportId::ROOT, &window, Some(window.scale_factor() as f32), None, None);
        let egui_renderer = egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::default());
        let mut ui = hearth_ui::Ui::new();
        ui.input.ppp = window.scale_factor() as f32;
        ui.clipboard = Box::new(SystemClipboard(egui_winit::clipboard::Clipboard::new(window.display_handle().ok().map(|h| h.as_raw()))));
        let ui_renderer = hearth_ui::Renderer::new(&gpu.device, format);
        if self.args.screenshot_after.is_some() {
            // automated captures shouldn't catch anything mid-bounce
            egui_ctx.all_styles_mut(|s| s.animation_time = 0.0);
            ui.instant = true;
        }
        self.gfx = Some(Gfx { surface, config, renderer, ui, ui_renderer, egui_ctx, egui_state, egui_renderer, window, meshes: Default::default(), peons, anims: Default::default() });
        self.last = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        let consumed = gfx.egui_state.on_window_event(&gfx.window, &event).consumed;
        let over_egui = gfx.egui_ctx.is_pointer_over_egui();
        let egui_kb = gfx.egui_ctx.egui_wants_keyboard_input();
        // developer windows sit on top: what they take, the game UI doesn't see
        let to_ui = match &event {
            WindowEvent::MouseInput { state: ElementState::Pressed, .. } | WindowEvent::MouseWheel { .. } => !over_egui,
            WindowEvent::KeyboardInput { .. } => !egui_kb,
            _ => true,
        };
        if to_ui {
            gfx.ui.input.on_event(&event);
        }
        let wants_kb = egui_kb || gfx.ui.wants_keyboard();
        let over_ui = over_egui || gfx.ui.wants_pointer();
        match event {
            WindowEvent::CloseRequested => {
                if self.args.screenshot_after.is_none() && self.game.creator.is_none() {
                    self.game.autosave();
                }
                event_loop.exit()
            }
            WindowEvent::Resized(s) => {
                gfx.config.width = s.width.max(1);
                gfx.config.height = s.height.max(1);
                gfx.surface.configure(&gfx.renderer.gpu.device, &gfx.config);
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32, position.y as f32);
                let (dx, dy) = (p.0 - self.cursor.0, p.1 - self.cursor.1);
                self.cursor = p;
                if self.rmb {
                    self.game.cam.rotate(dx * 0.006, dy * 0.004);
                }
                if self.mmb {
                    self.game.cam.pan(-dx * 0.05, dy * 0.05);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                match button {
                    MouseButton::Right => self.rmb = pressed && !over_ui || (self.rmb && pressed),
                    MouseButton::Middle => self.mmb = pressed && !over_ui,
                    _ => {}
                }
                if button == MouseButton::Right && !pressed {
                    self.rmb = false;
                }
                if (!(consumed || over_ui) || !pressed)
                    && !(over_ui && pressed) {
                        self.on_click(button, pressed);
                    }
            }
            WindowEvent::MouseWheel { delta, .. } if !over_ui => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.game.cam.zoom(1.0 - y * 0.1);
            }
            WindowEvent::KeyboardInput { event, .. } if !wants_kb => {
                if let PhysicalKey::Code(code) = event.physical_key
                    && (!event.repeat || !event.state.is_pressed()) {
                        self.on_key(code, event.state.is_pressed(), event_loop);
                    }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.args.screenshot_after.is_some() && self.gfx.is_some() {
            // drive frames ourselves in automated runs
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
            self.redraw(event_loop);
            return;
        }
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,game=info,hearth_render=info")).init();
    let args = parse_args();
    let event_loop = EventLoop::new()?;
    let mut app = App::new(args);
    event_loop.run_app(&mut app)?;
    Ok(())
}
