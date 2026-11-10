//! Builds a renderer [`Frame`] from the simulation state: lighting from the clock, lot geometry
//! with wall cutaway and level hiding, greybox furniture, rigged peon bodies ([`crate::peons`]; a
//! procedural greybox body when no art is loaded), and overlays.

use glam::{IVec2, Mat4, Quat, Vec3};
use hearth_render::{Builtins, Camera, Draw, Frame, MaterialId, MeshId, PointLight, flags};
use sim::Entity;
use sim::components::*;
use sim::data::{GameData, LifeStage, Pose, Shape};
use sim::lot::{Axis, Edge, LEVEL_HEIGHT, Lot, Opening, STAIR_LEN};
use sim::objects::ObjectInstance;
use sim::time::Clock;
use sim::Sim;

use crate::peons::{AnimState, Animator, Demeanor, Look, PeonArt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallMode {
    Up,
    Cutaway,
    Down,
}

/// A build/buy preview drawn translucently.
#[derive(Clone, Debug)]
pub enum Ghost {
    Object { def: String, level: i32, tile: IVec2, rot: u8, valid: bool },
    Walls { edges: Vec<Edge>, valid: bool },
    Tiles { level: i32, tiles: Vec<IVec2>, color: [f32; 3] },
}

#[derive(Clone, Debug)]
pub struct SceneOptions {
    pub active_level: i32,
    pub wall_mode: WallMode,
    /// Interpolation between the previous and current tick (0..1).
    pub alpha: f32,
    /// Real seconds, for idle animation.
    pub time: f32,
    pub selected: Option<Entity>,
    pub hovered: Option<Entity>,
    pub ghosts: Vec<Ghost>,
    pub show_rooms: bool,
    pub show_paths: bool,
    /// Camera focus point (used for cutaway decisions).
    pub focus: Vec3,
    /// Terrain heightmap mesh (see [`crate::lotmesh::LotMeshes`]); flat grass tiles if absent.
    pub terrain_mesh: Option<MeshId>,
    /// Roof meshes by the level they sit on.
    pub roof_meshes: Vec<(i32, MeshId)>,
    pub show_roofs: bool,
    /// Create-a-Household line-up: (stage, look, being edited).
    pub preview: Vec<(LifeStage, Appearance, bool)>,
    /// Where the preview line-up stands (centre, at the floor focus height).
    pub preview_at: Vec3,
}

impl Default for SceneOptions {
    fn default() -> Self {
        SceneOptions {
            active_level: 0,
            wall_mode: WallMode::Cutaway,
            alpha: 1.0,
            time: 0.0,
            selected: None,
            hovered: None,
            ghosts: Vec::new(),
            show_rooms: false,
            show_paths: false,
            focus: Vec3::new(16.0, 0.0, 16.0),
            terrain_mesh: None,
            roof_meshes: Vec::new(),
            show_roofs: true,
            preview: Vec::new(),
            preview_at: Vec3::ZERO,
        }
    }
}

/// Text to paint over the 3D view (speech bubbles, names).
#[derive(Clone, Debug)]
pub struct Label {
    pub world: Vec3,
    pub text: String,
    pub kind: LabelKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    Speech,
    Thought,
    Name,
    Alert,
}

pub struct SceneOut {
    pub frame: Frame,
    pub labels: Vec<Label>,
}

pub const WALL_THICK: f32 = 0.12;
pub const CUT_HEIGHT: f32 = 0.3;

pub fn wall_color(style: u16) -> [f32; 3] {
    match style {
        0 => [0.86, 0.80, 0.70],
        1 => [0.92, 0.90, 0.85],
        2 => [0.72, 0.84, 0.88],
        3 => [0.75, 0.52, 0.45],
        4 => [0.55, 0.68, 0.55],
        5 => [0.95, 0.96, 0.96],
        6 => [0.90, 0.74, 0.72],
        7 => [0.78, 0.86, 0.90],
        _ => [0.8, 0.8, 0.8],
    }
}

pub fn floor_color(style: u16, x: i32, z: i32) -> [f32; 3] {
    let checker = ((x + z) & 1) as f32;
    match style {
        0 => {
            let v = ((x * 7 + z * 13) % 5) as f32 * 0.012;
            [0.56 + v, 0.40 + v, 0.27]
        }
        1 => {
            let c = 0.78 + checker * 0.08;
            [c, c, c * 0.98]
        }
        2 => [0.47, 0.52, 0.62],
        3 => [0.35, 0.33, 0.32],
        4 => [0.88, 0.88, 0.87],
        5 => [0.64, 0.58, 0.48],
        6 => [0.55, 0.38, 0.21],
        _ => [0.6, 0.6, 0.6],
    }
}

/// Wall covering: the textured material and its tint when loaded, else the flat colour.
pub fn wall_surface(r: &Builtins, style: u16) -> ([f32; 3], Option<MaterialId>) {
    let (name, tint) = match style {
        3 => ("wall_brick", [1.0; 3]),
        5 => ("wall_tile", wall_color(5)),
        6 => ("wall_wallpaper", wall_color(6)),
        7 => ("wall_panel", wall_color(7)),
        s => ("wall_plaster", wall_color(s)),
    };
    match r.material(name) {
        Some(m) => (tint, Some(m)),
        None => (wall_color(style), None),
    }
}

/// Floor tile: the textured material and its tint when loaded, else the flat colour.
pub fn floor_surface(r: &Builtins, style: u16, x: i32, z: i32) -> ([f32; 3], Option<MaterialId>) {
    let name = match style {
        0 => "floor_wood",
        1 => "floor_tile",
        2 => "floor_carpet",
        4 => "floor_marble",
        5 => "floor_stone",
        6 => "floor_parquet",
        _ => "floor_concrete",
    };
    match r.material(name) {
        Some(m) => ([1.0; 3], Some(m)),
        None => (floor_color(style, x, z), None),
    }
}

fn mood_color(mood: f32) -> [f32; 3] {
    let t = ((mood + 100.0) / 200.0).clamp(0.0, 1.0);
    if t > 0.5 {
        let u = (t - 0.5) * 2.0;
        [1.0 - u * 0.8, 0.85, 0.2]
    } else {
        let u = t * 2.0;
        [0.95, 0.2 + u * 0.65, 0.15]
    }
}

/// Sun direction/colour/intensity and sky colours for the hour of day.
fn lighting(frame: &mut Frame, clock: &Clock) {
    let h = clock.hour_f();
    let day_t = ((h - 6.0) / 14.0).clamp(0.0, 1.0);
    let daylight = if (6.0..20.0).contains(&h) { (day_t * std::f32::consts::PI).sin() } else { 0.0 };
    let az = (-70.0 + day_t * 140.0f32).to_radians();
    let elev = (8.0 + daylight * 57.0f32).to_radians();
    let sun = Vec3::new(az.sin() * elev.cos(), elev.sin(), az.cos() * elev.cos() * 0.6 + 0.4).normalize();
    let night = 1.0 - (daylight * 4.0).clamp(0.0, 1.0);
    let golden = (1.0 - daylight * 2.5).clamp(0.0, 1.0) * (1.0 - night);
    let day_color = Vec3::new(1.0, 0.95, 0.88);
    let gold = Vec3::new(1.0, 0.65, 0.35);
    let moon = Vec3::new(0.45, 0.55, 0.85);
    frame.sun_dir = if night > 0.5 { Vec3::new(-0.3, 0.8, 0.5).normalize() } else { sun };
    frame.sun_color = day_color.lerp(gold, golden).lerp(moon, night);
    frame.sun_intensity = 3.2 * (1.0 - night) + 0.5 * night;
    let sky_day = Vec3::new(0.38, 0.45, 0.58);
    let sky_night = Vec3::new(0.04, 0.05, 0.09);
    frame.sky = sky_day.lerp(Vec3::new(0.5, 0.4, 0.4), golden).lerp(sky_night, night);
    frame.ground = Vec3::new(0.2, 0.18, 0.14).lerp(Vec3::new(0.03, 0.03, 0.04), night);
    frame.clear = Vec3::new(0.55, 0.72, 0.9).lerp(Vec3::new(0.95, 0.6, 0.45), golden).lerp(Vec3::new(0.02, 0.03, 0.07), night);
    frame.fog_color = frame.clear;
}

fn bx(r: &Builtins, center: Vec3, size: Vec3, color: [f32; 3]) -> Draw {
    Draw::new(r.cube, Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, center), color)
}

pub fn build(sim: &Sim, r: &Builtins, peons: Option<&PeonArt>, anims: &mut Animator, camera: Camera, opts: &SceneOptions) -> SceneOut {
    anims.begin_frame();
    let data = sim.data();
    let world = &sim.world;
    let clock = *world.res::<Clock>();
    let lot = world.res::<Lot>();
    let mut frame = Frame::new(camera);
    frame.indirect_center = opts.focus;
    let mut labels = Vec::new();
    lighting(&mut frame, &clock);
    frame.shadow_center = Vec3::new(lot.width as f32 * 0.5, opts.active_level as f32 * LEVEL_HEIGHT, lot.depth as f32 * 0.5);
    frame.shadow_radius = (lot.width.max(lot.depth) as f32) * 0.8;
    let d = &mut frame.draws;
    let top = opts.active_level;
    let night = clock.is_night() || clock.hour() < 7 || clock.hour() >= 19;

    // ---------------------------------------------------------------- ground & street
    if top >= 0 {
        let (w, dp) = (lot.width as f32, lot.depth as f32);
        // surrounding land: four slabs around the lot so holes in the lot stay holes
        let land = [0.30, 0.42, 0.2];
        let big = 80.0;
        for (c, sz) in [
            (Vec3::new(-big * 0.5, -0.06, dp * 0.5), Vec3::new(big, 0.1, dp + 2.0 * big)),
            (Vec3::new(w + big * 0.5, -0.06, dp * 0.5), Vec3::new(big, 0.1, dp + 2.0 * big)),
            (Vec3::new(w * 0.5, -0.06, -big * 0.5), Vec3::new(w, 0.1, big)),
            (Vec3::new(w * 0.5, -0.06, dp + big * 0.5), Vec3::new(w, 0.1, big)),
        ] {
            d.push(bx(r, c, sz, land).rough(0.95).flags(flags::NO_SHADOW_CAST));
        }
        // the lot itself (slightly lusher), with holes where basements have been dug under open sky
        if let Some(tm) = opts.terrain_mesh {
            d.push(Draw::new(tm, Mat4::IDENTITY, [1.0, 1.0, 1.0]).rough(0.95).flags(flags::NO_SHADOW_CAST));
        }
        for x in 0..lot.width {
            if opts.terrain_mesh.is_some() {
                break;
            }
            for z in 0..lot.depth {
                let t = IVec2::new(x, z);
                if lot.has_floor(0, t) || !lot.has_ground(0, t) {
                    continue;
                }
                let v = (((x * 31 + z * 17) % 7) as f32) * 0.006;
                d.push(bx(r, Vec3::new(x as f32 + 0.5, -0.04, z as f32 + 0.5), Vec3::new(1.0, 0.1, 1.0), [0.33 + v, 0.5 + v, 0.22]).rough(0.95).flags(flags::NO_SHADOW_CAST));
            }
        }
        // sidewalk & street on the +z side
        d.push(bx(r, Vec3::new(w * 0.5, -0.02, dp + 1.0), Vec3::new(160.0, 0.1, 2.0), [0.62, 0.62, 0.6]).rough(0.9));
        d.push(bx(r, Vec3::new(w * 0.5, -0.05, dp + 6.0), Vec3::new(160.0, 0.1, 8.0), [0.22, 0.22, 0.24]).rough(0.8));
        for i in -20..30 {
            d.push(bx(r, Vec3::new(i as f32 * 4.0, 0.0, dp + 6.0), Vec3::new(2.0, 0.02, 0.15), [0.9, 0.85, 0.4]).flags(flags::NO_SHADOW_CAST));
        }
        // a walkway from the street to the front door, if there is one on the street side
        let exit = sim::actions::exit_tile(&lot);
        d.push(bx(r, Vec3::new(exit.x as f32 + 0.5, -0.01, dp - 0.5), Vec3::new(1.0, 0.06, 1.0), [0.66, 0.64, 0.6]));
    } else {
        frame.clear = Vec3::new(0.08, 0.07, 0.07);
        frame.fog_color = frame.clear;
    }

    // ---------------------------------------------------------------- floors
    for (&(level, x, z), f) in lot.floors.iter() {
        if level > top || (top >= 0 && level < 0) {
            continue;
        }
        let y = level as f32 * LEVEL_HEIGHT;
        let thick = if level == 0 { 0.06 } else { 0.25 };
        let (color, mat) = floor_surface(r, f.style, x, z);
        d.push(bx(r, Vec3::new(x as f32 + 0.5, y + 0.02 - thick * 0.5, z as f32 + 0.5), Vec3::new(1.0, thick, 1.0), color).rough(0.55).material(mat).flags(flags::WORLD_TEXTURE));
    }
    // basement earth walls (dark slab under dug area so the basement doesn't float)
    if top < 0 {
        for (&(level, x, z), _) in lot.floors.iter().filter(|((l, _, _), _)| *l < 0 && *l >= top) {
            d.push(bx(r, Vec3::new(x as f32 + 0.5, level as f32 * LEVEL_HEIGHT - 0.4, z as f32 + 0.5), Vec3::new(1.0, 0.6, 1.0), [0.25, 0.2, 0.16]));
        }
    }

    // ---------------------------------------------------------------- walls
    let f = {
        let v = opts.focus - camera.eye;
        Vec3::new(v.x, 0.0, v.z).normalize_or_zero()
    };
    for (e, wall) in lot.walls.iter() {
        if e.level > top || (top >= 0 && e.level < 0) {
            continue;
        }
        let cut = e.level == top
            && match opts.wall_mode {
                WallMode::Up => false,
                WallMode::Down => true,
                WallMode::Cutaway => {
                    let n = match e.axis {
                        Axis::X => Vec3::Z,
                        Axis::Z => Vec3::X,
                    };
                    let facing = n.dot(f);
                    if facing.abs() < 0.2 {
                        false
                    } else {
                        // tile "behind" the wall from the camera's point of view
                        let (a, b) = e.tiles();
                        let behind = if facing > 0.0 { b } else { a };
                        lot.room_at(e.level, behind).is_some_and(|rm| rm.indoor)
                    }
                }
            };
        let opening = lot.openings.get(e).copied();
        d.extend(wall_piece_list(r, e, wall_surface(r, wall.style), cut, opening));
    }

    // ---------------------------------------------------------------- stairs
    for s in &lot.stairs {
        // stairs leading down from the active level are visible through their stairwell
        let leads_down_from_top = s.level + 1 == top;
        if s.level > top || (top >= 0 && s.level < 0 && !leads_down_from_top) {
            continue;
        }
        if leads_down_from_top && top >= 0 && s.level < 0 {
            // dark shaft floor at the bottom of the stairwell
            for t in s.tiles() {
                d.push(bx(r, Vec3::new(t.x as f32 + 0.5, s.level as f32 * LEVEL_HEIGHT - 0.05, t.y as f32 + 0.5), Vec3::new(1.0, 0.1, 1.0), [0.12, 0.1, 0.09]));
            }
        }
        let steps = 12;
        let dir = s.dir.vec().as_vec2();
        let start = s.start.as_vec2() + glam::Vec2::splat(0.5) - dir * 0.5;
        let base = s.level as f32 * LEVEL_HEIGHT;
        let run = STAIR_LEN as f32;
        let side = glam::Vec2::new(-dir.y, dir.x);
        for i in 0..steps {
            let t0 = i as f32 / steps as f32;
            let p = start + dir * (t0 + 0.5 / steps as f32) * run;
            let h = (i + 1) as f32 / steps as f32 * LEVEL_HEIGHT;
            let len = run / steps as f32;
            let size = if dir.x.abs() > 0.5 { Vec3::new(len, 0.12, 0.9) } else { Vec3::new(0.9, 0.12, len) };
            d.push(bx(r, Vec3::new(p.x, base + h - 0.06, p.y), size, [0.55, 0.4, 0.28]).rough(0.5));
        }
        // stringers
        for sgn in [-0.47f32, 0.47] {
            let mid = start + dir * run * 0.5 + side * sgn;
            let ang = (LEVEL_HEIGHT / run).atan();
            let len = (run * run + LEVEL_HEIGHT * LEVEL_HEIGHT).sqrt();
            let yaw = dir.x.atan2(dir.y);
            let m = Mat4::from_translation(Vec3::new(mid.x, base + LEVEL_HEIGHT * 0.5, mid.y))
                * Mat4::from_rotation_y(yaw)
                * Mat4::from_rotation_x(-ang)
                * Mat4::from_scale(Vec3::new(0.06, 0.25, len));
            d.push(Draw::new(r.cube, m, [0.4, 0.28, 0.2]));
        }
    }

    // ---------------------------------------------------------------- roofs
    // A roof hides the level it sits on unless walls are fully up; roofs over lower levels always show.
    if opts.show_roofs && top >= 0 {
        for (l, mesh) in &opts.roof_meshes {
            if *l < top || (*l == top && opts.wall_mode == WallMode::Up) {
                d.push(Draw::new(*mesh, Mat4::IDENTITY, [1.0, 1.0, 1.0]).rough(0.8));
            }
        }
    }

    // ---------------------------------------------------------------- objects
    for (oe, inst) in world.query::<(Entity, &ObjectInstance)>().iter() {
        if inst.level > top || (top >= 0 && inst.level < 0) {
            continue;
        }
        let def = data.object(inst.def);
        let selected = opts.hovered == Some(oe);
        object_draws(d, r, &data, inst.def, inst, 1.0, if selected { Some([1.15, 1.15, 1.0]) } else { None });
        if let Some((intensity, color)) = def.light
            && (night || inst.level < 0) {
                let p = inst.center(def) + Vec3::Y * 1.4;
                frame.lights.push(PointLight { pos: p, radius: 7.0, color: Vec3::from(color), intensity });
            }
        if inst.broken {
            labels.push(Label { world: inst.center(def) + Vec3::Y * 1.2, text: "broken!".into(), kind: LabelKind::Alert });
        }
    }
    // ceiling-ish fill light per indoor room at night so interiors aren't pitch black
    if night || top < 0 {
        for rm in lot.rooms.iter().filter(|rm| rm.indoor && rm.level <= top && (top < 0 || rm.level >= 0)) {
            // light at the centroid of the room's tiles
            let mut sum = Vec3::ZERO;
            let mut n = 0.0;
            for (&(l, x, z), &id) in lot.room_of.iter() {
                if id == rm.id && l == rm.level {
                    sum += Vec3::new(x as f32 + 0.5, 0.0, z as f32 + 0.5);
                    n += 1.0;
                }
            }
            if n > 0.0 && frame.lights.len() < hearth_render::MAX_POINT_LIGHTS {
                let c = sum / n;
                frame.lights.push(PointLight {
                    pos: Vec3::new(c.x, rm.level as f32 * LEVEL_HEIGHT + 2.6, c.z),
                    radius: (n.sqrt() * 1.6).max(5.0),
                    color: Vec3::new(1.0, 0.85, 0.65),
                    intensity: 2.2,
                });
            }
        }
    }

    // ---------------------------------------------------------------- peons
    for (e, peon, tf, age, app, posture, bubble) in world
        .query_filtered::<(Entity, &Peon, &Transform, &Age, &Appearance, &Posture, &Bubble), (hearth_ecs::Without<AtWork>, hearth_ecs::Without<Offlot>, hearth_ecs::Without<Dead>)>()
        .iter()
    {
        let pos = tf.prev_pos.lerp(tf.pos, opts.alpha);
        let lvl = Lot::level_of(pos);
        if lvl > top || (top >= 0 && lvl < 0) {
            continue;
        }
        let yaw = lerp_angle(tf.prev_yaw, tf.yaw, opts.alpha);
        let mood = world.get::<Mood>(e).map(|m| m.value).unwrap_or(0.0);
        let demeanor = Demeanor::of(mood, world.get::<Personality>(e).as_deref());
        let h = match peons {
            Some(art) => {
                let drawn = art.draw((&mut frame.skinned, &mut frame.joints), pos, yaw, age.stage, app, posture, demeanor, anims.state(e), opts.time + e.index() as f32 * 0.37, Look::Normal);
                if let Some(prop) = posture.prop.as_deref().and_then(|id| data.prop(id))
                    && let Some(grip) = drawn.grip(prop.hand)
                {
                    prop_draws(d, r, prop, grip);
                }
                drawn.crown
            }
            None => body_draws(d, r, pos, yaw, age.stage, app, posture, opts.time * 7.0 + e.index() as f32 * 1.7),
        };
        if opts.selected == Some(e) {
            let floor_y = lvl as f32 * LEVEL_HEIGHT + 0.03;
            d.push(
                Draw::new(r.disc, Mat4::from_scale_rotation_translation(Vec3::new(0.9, 1.0, 0.9), Quat::IDENTITY, Vec3::new(pos.x, floor_y, pos.z)), mood_color(mood))
                    .flags(flags::UNLIT | flags::NO_SHADOW_CAST)
                    .alpha(0.85),
            );
        }
        let head = pos + Vec3::Y * (h + 0.35);
        if let Some(s) = &bubble.speech {
            let text = match &bubble.icon {
                Some(i) => format!("[{}] {}", icon_text(i), s),
                None => s.clone(),
            };
            labels.push(Label { world: head, text, kind: LabelKind::Speech });
        } else if let Some(i) = &bubble.icon {
            labels.push(Label { world: head, text: icon_text(i), kind: LabelKind::Thought });
        }
        if opts.hovered == Some(e) {
            labels.push(Label { world: head + Vec3::Y * 0.3, text: peon.full_name(), kind: LabelKind::Name });
        }
        // debug: current path
        if opts.show_paths
            && let Some(q) = world.get::<ActionQueue>(e)
                && let Some(CurrentAction { phase: Phase::Walking { path, idx, .. }, .. }) = &q.current {
                    for p in path.iter().skip(*idx) {
                        d.push(Draw::new(r.sphere, Mat4::from_scale_rotation_translation(Vec3::splat(0.15), Quat::IDENTITY, *p + Vec3::Y * 0.1), [0.2, 0.6, 1.0]).flags(flags::UNLIT));
                    }
                }
    }

    // ---------------------------------------------------------------- creator line-up
    if !opts.preview.is_empty() {
        let n = opts.preview.len() as f32;
        let base = opts.preview_at;
        for (i, (stage, app, editing)) in opts.preview.iter().enumerate() {
            let p = base + Vec3::new((i as f32 - (n - 1.0) * 0.5) * crate::creator::LINEUP_SPACING, 0.25, 0.0);
            let to_cam = camera.eye - p;
            let yaw = to_cam.x.atan2(to_cam.z);
            d.push(Draw::new(r.cylinder, Mat4::from_scale_rotation_translation(Vec3::new(0.9, 0.25, 0.9), Quat::IDENTITY, p - Vec3::Y * 0.125), if *editing { [0.95, 0.85, 0.4] } else { [0.7, 0.7, 0.72] }).rough(0.4));
            let posture = Posture::default();
            match peons {
                Some(art) => _ = art.draw((&mut frame.skinned, &mut frame.joints), p, yaw, *stage, app, &posture, Demeanor::Normal, &mut AnimState::default(), opts.time + i as f32 * 0.5, Look::Normal),
                None => _ = body_draws(d, r, p, yaw, *stage, app, &posture, opts.time * 2.0),
            }
        }
    }

    // ---------------------------------------------------------------- ghosts
    for e in sim::ghosts::visible_ghosts(world) {
        let (Some(tf), Some(age), Some(app)) = (world.get::<Transform>(e), world.get::<Age>(e), world.get::<Appearance>(e)) else { continue };
        let pos = tf.prev_pos.lerp(tf.pos, opts.alpha);
        let lvl = Lot::level_of(pos);
        if lvl > top || (top >= 0 && lvl < 0) {
            continue;
        }
        let yaw = lerp_angle(tf.prev_yaw, tf.yaw, opts.alpha);
        let bob = (opts.time * 2.0 + e.index() as f32).sin() * 0.08;
        let h = match peons {
            Some(art) => art.draw((&mut frame.skinned, &mut frame.joints), pos + Vec3::Y * bob, yaw, age.stage, &app, &Posture::default(), Demeanor::Sad, anims.state(e), opts.time, Look::Ghost).crown,
            None => {
                let first = d.len();
                let h = body_draws(d, r, pos + Vec3::Y * bob, yaw, age.stage, &app, &Posture::default(), opts.time * 2.0);
                for dr in &mut d[first..] {
                    let c = dr.color;
                    dr.color = [c[0] * 0.3 + 0.55, c[1] * 0.3 + 0.7, c[2] * 0.3 + 0.75, 0.38];
                    dr.emissive = 0.6;
                    dr.flags |= flags::NO_SHADOW_CAST;
                }
                h
            }
        };
        if let Some(b) = world.get::<Bubble>(e)
            && let Some(s) = &b.speech {
                labels.push(Label { world: pos + Vec3::Y * (h + 0.5), text: s.clone(), kind: LabelKind::Speech });
            }
        if opts.hovered == Some(e) {
            labels.push(Label { world: pos + Vec3::Y * (h + 0.8), text: format!("Ghost of {}", world.get::<Peon>(e).map(|p| p.full_name()).unwrap_or_default()), kind: LabelKind::Name });
        }
    }

    // ---------------------------------------------------------------- overlays
    if opts.show_rooms {
        for (&(l, x, z), &id) in lot.room_of.iter() {
            if l != top {
                continue;
            }
            let hue = (id as f32 * 0.61803) % 1.0;
            let c = hsv(hue, 0.6, 1.0);
            let col = if id == 0 { [0.5, 0.5, 0.5] } else { c };
            d.push(bx(r, Vec3::new(x as f32 + 0.5, l as f32 * LEVEL_HEIGHT + 0.06, z as f32 + 0.5), Vec3::new(0.9, 0.02, 0.9), col).flags(flags::UNLIT | flags::NO_SHADOW_CAST).alpha(0.35));
        }
        for &(l, x, z) in lot.occupancy.keys() {
            if l == top {
                d.push(bx(r, Vec3::new(x as f32 + 0.5, l as f32 * LEVEL_HEIGHT + 0.08, z as f32 + 0.5), Vec3::new(0.5, 0.02, 0.5), [1.0, 0.1, 0.1]).flags(flags::UNLIT | flags::NO_SHADOW_CAST).alpha(0.5));
            }
        }
        for rm in lot.rooms.iter().filter(|rm| rm.indoor && rm.level == top) {
            if let Some((&(l, x, z), _)) = lot.room_of.iter().find(|(k, id)| **id == rm.id && k.0 == rm.level) {
                labels.push(Label {
                    world: Vec3::new(x as f32 + 0.5, l as f32 * LEVEL_HEIGHT + 0.3, z as f32 + 0.5),
                    text: format!("room {} env {:.0}", rm.id, rm.score),
                    kind: LabelKind::Name,
                });
            }
        }
    }
    for g in &opts.ghosts {
        match g {
            Ghost::Object { def, level, tile, rot, valid } => {
                if let Some(id) = data.object_id(def) {
                    let inst = ObjectInstance::new(id, data.object(id), *level, *tile, *rot);
                    let tint = if *valid { [0.6, 1.3, 0.6] } else { [1.4, 0.4, 0.4] };
                    object_draws(d, r, &data, id, &inst, 0.55, Some(tint));
                }
            }
            Ghost::Walls { edges, valid } => {
                let c = if *valid { [0.5, 0.9, 0.5] } else { [0.95, 0.35, 0.3] };
                for e in edges {
                    for mut piece in wall_piece_list(r, e, (c, None), false, None) {
                        piece.color[3] = 0.5;
                        d.push(piece.flags(flags::NO_SHADOW_CAST));
                    }
                }
            }
            Ghost::Tiles { level, tiles, color } => {
                for t in tiles {
                    d.push(bx(r, Vec3::new(t.x as f32 + 0.5, *level as f32 * LEVEL_HEIGHT + 0.05, t.y as f32 + 0.5), Vec3::new(0.96, 0.04, 0.96), *color).flags(flags::UNLIT | flags::NO_SHADOW_CAST).alpha(0.45));
                }
            }
        }
    }

    SceneOut { frame, labels }
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Maps bubble icon keys to short display text (until there are real icon sprites).
pub fn icon_text(icon: &str) -> String {
    if let Some(need) = icon.strip_prefix("cry:") {
        return format!("WAAH ({need})");
    }
    match icon {
        "Zzz" | "zz" => "Zzz",
        "snack" => "nom",
        "pan" => "sizzle",
        "tv" => "tv",
        "shower" => "splish",
        "$" => "$",
        "<3" => "<3",
        "<3<3" => "<3 <3",
        "work" => "off to work",
        other => other,
    }
    .to_string()
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = (b - a) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    a + d * t
}

/// Wall pieces for one edge as draws; `surface` (from [`wall_surface`]) covers the wall itself,
/// not the door/window trim.
pub fn wall_piece_list(r: &Builtins, e: &Edge, surface: ([f32; 3], Option<MaterialId>), cut: bool, opening: Option<Opening>) -> Vec<Draw> {
    let (color, mat) = surface;
    let (a, b) = e.endpoints();
    let mid = (a + b) * 0.5;
    let along_x = e.axis == Axis::X;
    let len = 1.0 + WALL_THICK;
    let size = |l: f32, h: f32| if along_x { Vec3::new(l, h, WALL_THICK) } else { Vec3::new(WALL_THICK, h, l) };
    let full = if cut { CUT_HEIGHT } else { LEVEL_HEIGHT };
    let mut out = Vec::new();
    let mut seg = |y0: f32, y1: f32, c: [f32; 3]| {
        let y1 = y1.min(full);
        if y1 > y0 {
            out.push(bx(r, mid + Vec3::Y * ((y0 + y1) * 0.5), size(len, y1 - y0), c).rough(0.85).material(mat).flags(flags::WORLD_TEXTURE));
        }
    };
    match opening {
        None => seg(0.0, LEVEL_HEIGHT, color),
        Some(Opening::Door) => {
            seg(2.15, LEVEL_HEIGHT, color);
            // jambs
            let off = if along_x { Vec3::X * 0.46 } else { Vec3::Z * 0.46 };
            let jamb = |p: Vec3| bx(r, p + Vec3::Y * (full.min(2.15) * 0.5), size(0.08, full.min(2.15)).max(Vec3::splat(0.0)) + if along_x { Vec3::Z * 0.04 } else { Vec3::X * 0.04 }, [0.95, 0.94, 0.9]);
            out.push(jamb(mid - off));
            out.push(jamb(mid + off));
        }
        Some(Opening::Window) => {
            seg(0.0, 0.9, color);
            seg(2.1, LEVEL_HEIGHT, color);
            if !cut {
                let sill = bx(r, mid + Vec3::Y * 0.92, size(len - WALL_THICK, 0.05) + if along_x { Vec3::Z * 0.06 } else { Vec3::X * 0.06 }, [0.95, 0.94, 0.9]);
                out.push(sill);
                out.push(bx(r, mid + Vec3::Y * 1.5, size(0.94, 1.15) * if along_x { Vec3::new(1.0, 1.0, 0.25) } else { Vec3::new(0.25, 1.0, 1.0) }, [0.6, 0.78, 0.9]).rough(0.05).alpha(0.3).flags(flags::NO_SHADOW_CAST));
            }
        }
    }
    out
}

fn shape_mesh(r: &Builtins, s: Shape) -> MeshId {
    match s {
        Shape::Box => r.cube,
        Shape::Cylinder => r.cylinder,
        Shape::Sphere => r.sphere,
    }
}

fn part_material(r: &Builtins, p: &sim::data::Part) -> Option<MaterialId> {
    p.material.as_deref().and_then(|m| r.material(m))
}

/// Draws a held prop's parts in a hand's world-space grip frame.
pub fn prop_draws(d: &mut Vec<Draw>, r: &Builtins, prop: &sim::data::PropDef, grip: Mat4) {
    let rot = Quat::from_euler(glam::EulerRot::ZYX, prop.rot.z.to_radians(), prop.rot.y.to_radians(), prop.rot.x.to_radians());
    for p in &prop.parts {
        let m = grip * Mat4::from_quat(rot) * Mat4::from_scale_rotation_translation(p.size, Quat::IDENTITY, p.at);
        d.push(Draw::new(shape_mesh(r, p.shape), m, p.color).rough(p.rough).metal(p.metal).emissive(p.emissive).material(part_material(r, p)));
    }
}

pub fn object_draws(d: &mut Vec<Draw>, r: &Builtins, data: &GameData, def_id: usize, inst: &ObjectInstance, alpha: f32, tint: Option<[f32; 3]>) {
    let def = data.object(def_id);
    let rot = Quat::from_rotation_y(inst.yaw());
    let grime = 1.0 - inst.dirt * 0.35;
    let shade = |c: [f32; 3]| {
        let mut c = [c[0] * grime, c[1] * grime, c[2] * grime];
        if inst.broken {
            c = [c[0] * 0.6, c[1] * 0.55, c[2] * 0.5];
        }
        if let Some(t) = tint {
            c = [(c[0] * t[0]).min(1.0), (c[1] * t[1]).min(1.0), (c[2] * t[2]).min(1.0)];
        }
        c
    };
    if let Some(model) = r.models.get(&def.id) {
        let m = Mat4::from_rotation_translation(rot, inst.local_to_world(def, Vec3::ZERO));
        for p in model {
            let mut dr = Draw::new(p.mesh, m, shade([p.color[0], p.color[1], p.color[2]]))
                .rough(p.rough)
                .metal(p.metal)
                .emissive(if inst.broken { 0.0 } else { p.emissive })
                .alpha(alpha * p.color[3])
                .material(r.material(&p.material));
            if alpha * p.color[3] < 1.0 {
                dr = dr.flags(flags::NO_SHADOW_CAST);
            }
            d.push(dr);
        }
        return;
    }
    for p in &def.parts {
        let center = inst.local_to_world(def, p.at);
        let m = Mat4::from_scale_rotation_translation(p.size, rot, center);
        let mut dr = Draw::new(shape_mesh(r, p.shape), m, shade(p.color))
            .rough(p.rough)
            .metal(p.metal)
            .emissive(if inst.broken { 0.0 } else { p.emissive })
            .alpha(alpha)
            .material(part_material(r, p));
        if alpha < 1.0 {
            dr = dr.flags(flags::NO_SHADOW_CAST);
        }
        d.push(dr);
    }
}

/// Draws a greybox peon from primitives (fallback when no rigged art is loaded). Returns the body
/// height (for label placement).
#[allow(clippy::too_many_arguments)]
pub fn body_draws(d: &mut Vec<Draw>, r: &Builtins, pos: Vec3, yaw: f32, stage: LifeStage, app: &Appearance, posture: &Posture, phase: f32) -> f32 {
    let h = stage.height() * app.height;
    let head_ratio = match stage {
        LifeStage::Infant => 0.36,
        LifeStage::Toddler => 0.3,
        LifeStage::Child => 0.24,
        LifeStage::Teen => 0.2,
        _ => 0.18,
    };
    let head = h * head_ratio;
    let legs = h * (if stage <= LifeStage::Toddler { 0.3 } else { 0.47 });
    let torso = h - head - legs;
    let width = h * 0.2 * (0.85 + app.build * 0.45);
    let depth = h * 0.11 * (0.85 + app.build * 0.5);
    let limb = h * 0.065 * (0.85 + app.build * 0.4);

    let walking = posture.walking;
    let swing = if walking { phase.sin() * 0.55 } else { 0.0 };
    let mut root = Mat4::from_translation(pos) * Mat4::from_rotation_y(yaw);
    let mut leg_pitch = [swing, -swing];
    let mut arm_pitch = [-swing * 0.8, swing * 0.8];
    let mut hips_y = legs;
    match posture.pose {
        Pose::Sit => {
            leg_pitch = [-1.45, -1.45];
            arm_pitch = [-0.5, -0.5];
            hips_y = 0.02;
        }
        Pose::Lie => {
            root = Mat4::from_translation(pos + Vec3::Y * (depth * 0.5 + 0.02)) * Mat4::from_rotation_y(yaw) * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2) * Mat4::from_translation(Vec3::new(0.0, -h * 0.5, 0.0));
            leg_pitch = [0.0, 0.0];
            arm_pitch = [0.0, 0.0];
        }
        Pose::Use => {
            arm_pitch = [-1.2 + (phase * 0.7).sin() * 0.15, -1.2 - (phase * 0.7).sin() * 0.15];
        }
        Pose::Exercise => {
            root *= Mat4::from_translation(Vec3::Y * (phase * 1.5).sin().abs() * 0.08);
            leg_pitch = [(phase * 1.5).sin() * 0.7, -(phase * 1.5).sin() * 0.7];
            arm_pitch = [-2.6, -2.6];
        }
        Pose::Dance => {
            root *= Mat4::from_rotation_y((phase * 0.8).sin() * 0.6);
            arm_pitch = [-2.0 + phase.sin(), -2.0 - phase.sin()];
        }
        Pose::Stand => {}
    }
    if stage == LifeStage::Infant && posture.pose != Pose::Lie {
        // infants sit (and scoot) everywhere
        leg_pitch = [-1.5, -1.5];
        hips_y = 0.02;
    }
    let skin = app.skin;
    let shirt = app.shirt;
    let pants = app.pants;
    let part = |d: &mut Vec<Draw>, local: Mat4, mesh: MeshId, color: [f32; 3]| d.push(Draw::new(mesh, root * local, color).rough(0.75));
    // legs (pivot at hip)
    for (i, sx) in [-1.0f32, 1.0].iter().enumerate() {
        let hip = Vec3::new(sx * width * 0.25, hips_y, 0.0);
        let m = Mat4::from_translation(hip) * Mat4::from_rotation_x(leg_pitch[i]) * Mat4::from_translation(Vec3::new(0.0, -legs * 0.5, 0.0)) * Mat4::from_scale(Vec3::new(limb * 1.15, legs, limb * 1.2));
        part(d, m, r.cube, pants);
        // shoe
        let shoe = Mat4::from_translation(hip) * Mat4::from_rotation_x(leg_pitch[i]) * Mat4::from_translation(Vec3::new(0.0, -legs + 0.03, limb * 0.35)) * Mat4::from_scale(Vec3::new(limb * 1.2, 0.06, limb * 2.0));
        part(d, shoe, r.cube, [0.15, 0.12, 0.1]);
    }
    // torso
    let tm = Mat4::from_translation(Vec3::new(0.0, hips_y + torso * 0.5, 0.0)) * Mat4::from_scale(Vec3::new(width, torso, depth));
    part(d, tm, r.cube, shirt);
    // arms (pivot at shoulder)
    let arm_len = torso * 0.95;
    for (i, sx) in [-1.0f32, 1.0].iter().enumerate() {
        let sh = Vec3::new(sx * (width * 0.5 + limb * 0.55), hips_y + torso * 0.95, 0.0);
        let m = Mat4::from_translation(sh) * Mat4::from_rotation_x(arm_pitch[i]) * Mat4::from_translation(Vec3::new(0.0, -arm_len * 0.5, 0.0)) * Mat4::from_scale(Vec3::new(limb, arm_len, limb));
        part(d, m, r.cube, shirt);
        let hand = Mat4::from_translation(sh) * Mat4::from_rotation_x(arm_pitch[i]) * Mat4::from_translation(Vec3::new(0.0, -arm_len - limb * 0.4, 0.0)) * Mat4::from_scale(Vec3::splat(limb * 1.1));
        part(d, hand, r.sphere, skin);
    }
    // head
    let neck = hips_y + torso;
    let hc = Vec3::new(0.0, neck + head * 0.55, 0.0);
    part(d, Mat4::from_translation(hc) * Mat4::from_scale(Vec3::new(head * 0.85, head, head * 0.9)), r.sphere, skin);
    // eyes & nose so facing is readable
    for sx in [-1.0f32, 1.0] {
        part(d, Mat4::from_translation(hc + Vec3::new(sx * head * 0.17, head * 0.08, head * 0.41)) * Mat4::from_scale(Vec3::splat(head * 0.1)), r.sphere, [0.05, 0.05, 0.07]);
    }
    part(d, Mat4::from_translation(hc + Vec3::new(0.0, -head * 0.05, head * 0.45)) * Mat4::from_scale(Vec3::splat(head * 0.12)), r.sphere, [skin[0] * 0.92, skin[1] * 0.85, skin[2] * 0.82]);
    // hair
    let hair = app.hair;
    match app.hair_style % 4 {
        0 => part(d, Mat4::from_translation(hc + Vec3::new(0.0, head * 0.15, -head * 0.05)) * Mat4::from_scale(Vec3::new(head * 0.92, head * 0.8, head * 0.95)), r.sphere, hair),
        1 => {
            part(d, Mat4::from_translation(hc + Vec3::new(0.0, head * 0.12, -head * 0.05)) * Mat4::from_scale(Vec3::new(head * 0.9, head * 0.78, head * 0.93)), r.sphere, hair);
            part(d, Mat4::from_translation(hc + Vec3::new(0.0, head * 0.6, -head * 0.1)) * Mat4::from_scale(Vec3::splat(head * 0.4)), r.sphere, hair);
        }
        2 => {
            part(d, Mat4::from_translation(hc + Vec3::new(0.0, head * 0.15, -head * 0.05)) * Mat4::from_scale(Vec3::new(head * 0.94, head * 0.8, head * 0.95)), r.sphere, hair);
            part(d, Mat4::from_translation(hc + Vec3::new(0.0, -head * 0.35, -head * 0.32)) * Mat4::from_scale(Vec3::new(head * 0.85, head * 0.9, head * 0.3)), r.cube, hair);
        }
        _ => {
            // mostly bald with a heroic fringe
            part(d, Mat4::from_translation(hc + Vec3::new(0.0, -head * 0.05, -head * 0.25)) * Mat4::from_scale(Vec3::new(head * 0.9, head * 0.45, head * 0.5)), r.sphere, hair);
        }
    }
    match posture.pose {
        Pose::Lie => 0.5,
        Pose::Sit => hips_y + torso + head + pos.y.fract().min(0.6),
        _ => h,
    }
}
