//! `simctl` — headless tooling for the life sim.
//!
//! ```text
//! simctl run [--days N] [--seed S] [--echo] [--log FILE] [--manual-choices]
//!     Fast-forward the starter household and print a daily report: needs, mood, what each peon
//!     spent their time on, and anything alarming (starvation, stuck actions, broken economy).
//! simctl check [--seeds N] [--days D]
//!     Runs many seeds and fails (exit 1) on invariants: nobody starves, needs don't flatline,
//!     peons don't idle forever, the household doesn't go bankrupt immediately.
//! simctl render [--seed S] [--minutes M] [--hour H] [--level L] [--yaw D] [--pitch D] [--dist M]
//!               [--target X,Z] [--mode cutaway|up|down] [--size WxH] [--rooms] [--greybox] [--out FILE]
//!     Runs the starter household for M minutes and renders the lot offscreen to a PNG.
//!     `--lot N` fills out the street first and renders lot N instead (playing the household
//!     there, going out to it, or just looking at a vacant one).
//! simctl hood [--seed S] [--days D]
//!     Tours the neighborhood for D days: outings to the park, a day playing the neighbors, then
//!     home again. Prints the street (residents, funds, values) and fails on deaths by neglect.
//! simctl lineup [--pose stand|walk|sit|lie|use|dance|exercise] [--time T] [--yaw D] [--pitch D]
//!               [--anim CLIP] [--prop ID] [--demeanor normal|sad|proud|nervous] [--dist M] [--target X,Z]
//!               [--size WxH] [--out FILE]
//!     Renders every life stage's rigged body side by side (slim, average and heavy builds, all
//!     hair styles) in one pose (or interaction clip, holding a prop) at clip time T seconds.
//! simctl materials [--yaw D] [--pitch D] [--dist M] [--hour H] [--size WxH] [--out FILE]
//!     Renders every textured material (assets/textures) on a wall section and a floor patch.
//! ```

use std::collections::BTreeMap;
use std::time::Instant;

use sim::components::{ActionQueue, Dead, Needs, Peon};
use sim::data::Need;
use sim::log::LogKind;
use sim::starter::starter_sim;
use sim::{Entity, Sim};

struct Args {
    cmd: String,
    days: u64,
    seed: u64,
    seeds: u64,
    echo: bool,
    log: Option<String>,
    manual: bool,
    trace: Option<String>,
    render: RenderArgs,
}

struct RenderArgs {
    minutes: u64,
    hour: Option<f32>,
    level: i32,
    yaw: f32,
    pitch: f32,
    dist: f32,
    target: Option<(f32, f32)>,
    mode: String,
    size: (u32, u32),
    rooms: bool,
    /// Draw greybox peons and objects instead of the rigged/modelled art.
    greybox: bool,
    out: String,
    /// `lineup`: posture to show and clip time.
    pose: String,
    time: f32,
    /// `lineup`: interaction clip, held prop and demeanor (normal|sad|proud|nervous).
    anim: Option<String>,
    prop: Option<String>,
    demeanor: String,
    /// `render`: order everyone at home to do OBJECT:INTERACTION (ids) and render once they are.
    order: Option<String>,
    /// `render`: populate the street and render this lot.
    lot: Option<usize>,
}

fn parse() -> Args {
    let mut a = Args { cmd: "run".into(), days: 3, seed: 1, seeds: 8, echo: false, log: None, manual: false, trace: None,
        render: RenderArgs { minutes: 60, hour: None, level: 0, yaw: 35.0, pitch: 45.0, dist: 30.0, target: None, mode: "cutaway".into(), size: (1600, 1000), rooms: false, greybox: false, out: "out/render.png".into(), pose: "stand".into(), time: 0.0, anim: None, prop: None, demeanor: "normal".into(), order: None, lot: None } };
    let mut it = std::env::args().skip(1);
    if let Some(c) = it.next() {
        a.cmd = c;
    }
    while let Some(x) = it.next() {
        match x.as_str() {
            "--days" => a.days = it.next().unwrap().parse().unwrap(),
            "--seed" => a.seed = it.next().unwrap().parse().unwrap(),
            "--seeds" => a.seeds = it.next().unwrap().parse().unwrap(),
            "--echo" => a.echo = true,
            "--log" => a.log = it.next(),
            "--manual-choices" => a.manual = true,
            "--trace" => a.trace = it.next(),
            "--minutes" => a.render.minutes = it.next().unwrap().parse().unwrap(),
            "--hour" => a.render.hour = Some(it.next().unwrap().parse().unwrap()),
            "--level" => a.render.level = it.next().unwrap().parse().unwrap(),
            "--yaw" => a.render.yaw = it.next().unwrap().parse().unwrap(),
            "--pitch" => a.render.pitch = it.next().unwrap().parse().unwrap(),
            "--dist" => a.render.dist = it.next().unwrap().parse().unwrap(),
            "--target" => {
                let v: Vec<f32> = it.next().unwrap().split(',').map(|x| x.parse().unwrap()).collect();
                a.render.target = Some((v[0], v[1]));
            }
            "--mode" => a.render.mode = it.next().unwrap(),
            "--size" => {
                let v: Vec<u32> = it.next().unwrap().split('x').map(|x| x.parse().unwrap()).collect();
                a.render.size = (v[0], v[1]);
            }
            "--rooms" => a.render.rooms = true,
            "--greybox" => a.render.greybox = true,
            "--pose" => a.render.pose = it.next().unwrap(),
            "--time" => a.render.time = it.next().unwrap().parse().unwrap(),
            "--anim" => a.render.anim = it.next(),
            "--prop" => a.render.prop = it.next(),
            "--demeanor" => a.render.demeanor = it.next().unwrap(),
            "--out" => a.render.out = it.next().unwrap(),
            "--order" => a.render.order = it.next(),
            "--lot" => a.render.lot = Some(it.next().unwrap().parse().unwrap()),
            other => {
                eprintln!("unknown arg {other}");
                std::process::exit(2);
            }
        }
    }
    a
}

/// Minutes each peon spent per action label.
#[derive(Default)]
struct TimeBudget {
    per_peon: BTreeMap<Entity, BTreeMap<String, u32>>,
    min_need: BTreeMap<Entity, [f32; Need::COUNT]>,
}

impl TimeBudget {
    fn sample(&mut self, sim: &Sim) {
        for e in sim.living_peons() {
            let w = &sim.world;
            let label = if w.has::<sim::components::AtWork>(e) {
                "(at work)".to_string()
            } else {
                w.get::<ActionQueue>(e)
                    .and_then(|q| q.current.as_ref().map(|c| c.action.label.clone()))
                    .unwrap_or_else(|| "(nothing)".into())
            };
            *self.per_peon.entry(e).or_default().entry(label).or_default() += 1;
            let n = w.get::<Needs>(e).unwrap();
            let m = self.min_need.entry(e).or_insert([100.0; Need::COUNT]);
            for (i, slot) in m.iter_mut().enumerate() {
                if n.active[i] {
                    *slot = slot.min(n.v[i]);
                }
            }
        }
    }
}

fn run_sim(seed: u64, days: u64, echo: bool, manual: bool, report: bool) -> (Sim, TimeBudget) {
    run_sim_traced(seed, days, echo, manual, report, false)
}

fn run_sim_traced(seed: u64, days: u64, echo: bool, manual: bool, report: bool, trace: bool) -> (Sim, TimeBudget) {
    let mut sim = starter_sim(seed);
    sim.world.res_mut::<sim::time::Settings>().trace_actions = trace;
    sim.world.res_mut::<sim::time::Settings>().auto_career_choices = !manual;
    sim.world.res_mut::<sim::log::SimLog>().echo = echo;
    let mut tb = TimeBudget::default();
    let t0 = Instant::now();
    for day in 0..days {
        for _ in 0..sim::time::TICKS_PER_DAY {
            sim.tick();
            tb.sample(&sim);
        }
        if report {
            println!("\n=== end of day {} — {} ===", day + 1, sim.clock().label());
            println!("{}", serde_json::to_string_pretty(&sim.summary()).unwrap());
        }
    }
    if report {
        let el = t0.elapsed();
        let ticks = days * sim::time::TICKS_PER_DAY;
        println!("\n{} ticks in {:.2?} ({:.0} ticks/s)", ticks, el, ticks as f64 / el.as_secs_f64());
    }
    (sim, tb)
}

fn print_budget(sim: &Sim, tb: &TimeBudget) {
    println!("\n=== time budget (minutes) ===");
    for (e, m) in &tb.per_peon {
        let name = sim.world.get::<Peon>(*e).map(|p| p.full_name()).unwrap_or_else(|| format!("{e}"));
        let mut v: Vec<_> = m.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        let parts: Vec<String> = v.iter().take(14).map(|(k, n)| format!("{k}: {n}")).collect();
        println!("{name}:\n    {}", parts.join("\n    "));
        if let Some(mins) = tb.min_need.get(e) {
            let low: Vec<String> = Need::ALL
                .iter()
                .filter(|n| mins[n.idx()] < 100.0)
                .map(|n| format!("{} {:.0}", n.label(), mins[n.idx()]))
                .collect();
            println!("    min needs: {}", low.join(", "));
        }
    }
}

fn print_log(sim: &Sim, path: Option<&str>) {
    let log = sim.world.res::<sim::log::SimLog>();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in &log.entries {
        *counts.entry(format!("{:?}", e.kind)).or_default() += 1;
    }
    println!("\n=== log ({} entries) {:?} ===", log.entries.len(), counts);
    for e in log.entries.iter().filter(|e| e.kind != LogKind::Debug).rev().take(40).collect::<Vec<_>>().into_iter().rev() {
        let c = sim::time::Clock { tick: e.tick };
        println!("  [{}] {:?}: {}", c.label(), e.kind, e.text);
    }
    if let Some(p) = path {
        let json = serde_json::to_string_pretty(&log.entries).unwrap();
        std::fs::write(p, json).expect("write log");
        println!("full log written to {p}");
    }
}

/// Invariants every healthy run must satisfy. Returns failures.
fn check_invariants(sim: &Sim, tb: &TimeBudget) -> Vec<String> {
    let mut fails = Vec::new();
    for e in sim.peons() {
        let name = sim.name(e);
        if let Some(d) = sim.world.get::<Dead>(e)
            && d.cause != "old age" {
                fails.push(format!("{name} died of {}", d.cause));
            }
        if let Some(m) = tb.per_peon.get(&e) {
            let total: u32 = m.values().sum();
            let idle = m.get("Idle").copied().unwrap_or(0) + m.get("(nothing)").copied().unwrap_or(0);
            if total > 0 && idle as f32 / total as f32 > 0.45 {
                fails.push(format!("{name} idled {:.0}% of the time", 100.0 * idle as f32 / total as f32));
            }
        }
        if let Some(mins) = tb.min_need.get(&e) {
            // hunger and energy at zero mean starvation / collapse: real failures
            for n in [Need::Hunger, Need::Energy] {
                if mins[n.idx()] <= 0.0 {
                    fails.push(format!("{name}'s {} hit zero", n.label()));
                }
            }
            // zero hygiene (usually right after an "accident") is allowed but worth knowing about
            if mins[Need::Hygiene.idx()] <= 0.0 {
                println!("  (warning) {name}'s Hygiene hit zero");
            }
        }
    }
    let funds = sim.world.res::<sim::economy::Household>().funds;
    if funds < 0 {
        fails.push(format!("negative funds {funds}"));
    }
    fails
}

fn main() {
    let a = parse();
    match a.cmd.as_str() {
        "run" => {
            let (sim, tb) = run_sim_traced(a.seed, a.days, a.echo, a.manual, a.trace.is_none(), a.trace.is_some());
            if let Some(who) = &a.trace {
                let log = sim.world.res::<sim::log::SimLog>();
                for e in log.entries.iter().filter(|e| e.kind == LogKind::Debug && e.text.starts_with(who.as_str())) {
                    println!("[{}] {}", sim::time::Clock { tick: e.tick }.label(), e.text);
                }
                for e in log.entries.iter().filter(|e| e.kind != LogKind::Debug && e.text.contains(who.as_str())) {
                    println!("[{}] ** {:?}: {}", sim::time::Clock { tick: e.tick }.label(), e.kind, e.text);
                }
                return;
            }
            print_budget(&sim, &tb);
            print_log(&sim, a.log.as_deref());
            let fails = check_invariants(&sim, &tb);
            if !fails.is_empty() {
                println!("\n!!! invariant failures:\n  {}", fails.join("\n  "));
            }
        }
        "check" => {
            let mut bad = 0;
            for seed in 1..=a.seeds {
                let (sim, tb) = run_sim(seed, a.days, false, false, false);
                let fails = check_invariants(&sim, &tb);
                if fails.is_empty() {
                    println!("seed {seed}: ok (funds ${})", sim.world.res::<sim::economy::Household>().funds);
                } else {
                    bad += 1;
                    println!("seed {seed}: FAIL\n  {}", fails.join("\n  "));
                }
            }
            if bad > 0 {
                std::process::exit(1);
            }
        }
        "render" => render(&a),
        "lineup" => lineup(&a.render),
        "materials" => materials(&a.render),
        "hood" => hood_tour(a.seed, a.days, a.log.as_deref()),
        other => {
            eprintln!("unknown command {other}; try `run`, `check`, `render`, `lineup`, `materials` or `hood`");
            std::process::exit(2);
        }
    }
}

fn lineup(ra: &RenderArgs) {
    use glam::{Mat4, Vec3};
    use sim::components::{Appearance, Posture};
    use sim::data::{LifeStage, Pose};
    let gpu = hearth_render::Gpu::headless().expect("no GPU adapter");
    let mut r = hearth_render::Renderer::new(gpu, hearth_render::wgpu::TextureFormat::Rgba8UnormSrgb);
    let art = game::peons::PeonArt::load(&mut r).expect("peon models");
    game::materials::install(&mut r, &sim::data::GameData::embedded());
    let (pose, walking) = match ra.pose.as_str() {
        "walk" => (Pose::Stand, true),
        "sit" => (Pose::Sit, false),
        "lie" => (Pose::Lie, false),
        "use" => (Pose::Use, false),
        "dance" => (Pose::Dance, false),
        "exercise" => (Pose::Exercise, false),
        _ => (Pose::Stand, false),
    };
    let posture = Posture { pose, walking, anim: ra.anim.clone(), prop: ra.prop.clone(), sfx: None };
    let demeanor = match ra.demeanor.as_str() {
        "sad" => game::peons::Demeanor::Sad,
        "proud" => game::peons::Demeanor::Proud,
        "nervous" => game::peons::Demeanor::Nervous,
        _ => game::peons::Demeanor::Normal,
    };
    let data = sim::data::GameData::embedded();
    let prop = ra.prop.as_deref().map(|id| data.prop(id).unwrap_or_else(|| panic!("unknown prop {id}")));
    let stages = [LifeStage::Infant, LifeStage::Toddler, LifeStage::Child, LifeStage::Teen, LifeStage::Adult, LifeStage::Adult, LifeStage::Elder];
    // skin, hair, shirt, pants
    let looks: [[[f32; 3]; 4]; 4] = [
        [[0.96, 0.8, 0.69], [0.35, 0.22, 0.12], [0.85, 0.3, 0.3], [0.25, 0.3, 0.5]],
        [[0.55, 0.36, 0.24], [0.08, 0.06, 0.05], [0.3, 0.6, 0.4], [0.35, 0.3, 0.25]],
        [[0.8, 0.6, 0.45], [0.85, 0.7, 0.35], [0.95, 0.85, 0.4], [0.2, 0.2, 0.22]],
        [[0.36, 0.24, 0.17], [0.6, 0.2, 0.1], [0.4, 0.5, 0.9], [0.6, 0.55, 0.45]],
    ];
    let spacing = 1.2;
    let n = stages.len() as f32;
    let (w, h) = ra.size;
    // --target X,Z looks at that spot (e.g. -3.6,0 for the infant at the left end of the row)
    let target = ra.target.map_or(Vec3::new(0.0, 0.7, 0.0), |(x, z)| Vec3::new(x, 0.3, z));
    // `render`'s camera defaults (yaw 35, pitch 45, dist 30) mean "not given": frame the whole row
    let yaw = if ra.yaw == 35.0 { 0.0 } else { ra.yaw };
    let pitch = if ra.pitch == 45.0 { 12.0 } else { ra.pitch };
    let dist = if ra.dist == 30.0 { 9.0 } else { ra.dist };
    let camera = hearth_render::Camera::orbit(target, yaw.to_radians(), pitch.to_radians(), dist, w as f32 / h as f32);
    let mut frame = hearth_render::Frame::new(camera);
    frame.shadow_radius = 6.0;
    frame.draws.push(hearth_render::Draw::new(r.builtins.cube, Mat4::from_scale_rotation_translation(Vec3::new(12.0, 0.1, 6.0), glam::Quat::IDENTITY, Vec3::new(0.0, -0.05, 0.0)), [0.75, 0.72, 0.68]).rough(0.9));
    for (i, stage) in stages.iter().enumerate() {
        let x = (i as f32 - (n - 1.0) * 0.5) * spacing;
        let [skin, hair, shirt, pants] = looks[i % looks.len()];
        let app = Appearance { skin, hair, shirt, pants, height: 1.0, build: [0.5, 0.2, 0.5, 0.0, 0.5, 1.0, 0.6][i], hair_style: (i % 4) as u8, eyes: sim::genetics::EYE_COLORS[i % sim::genetics::EYE_COLORS.len()] };
        let y = if pose == Pose::Sit && *stage != LifeStage::Infant { 0.45 } else { 0.0 };
        if pose == Pose::Sit && *stage != LifeStage::Infant {
            frame.draws.push(hearth_render::Draw::new(r.builtins.cube, Mat4::from_scale_rotation_translation(Vec3::new(0.5, 0.45, 0.5), glam::Quat::IDENTITY, Vec3::new(x, 0.225, -0.05)), [0.55, 0.4, 0.3]));
        }
        let yaw = 0.0;
        let drawn = art.draw((&mut frame.skinned, &mut frame.joints), Vec3::new(x, y, 0.0), yaw, *stage, &app, &posture, demeanor, &mut game::peons::AnimState::default(), ra.time, game::peons::Look::Normal);
        if let Some(p) = prop
            && let Some(grip) = drawn.grip(p.hand)
        {
            game::scene::prop_draws(&mut frame.draws, &r.builtins, p, grip);
        }
    }
    if let Some(dir) = std::path::Path::new(&ra.out).parent() {
        std::fs::create_dir_all(dir).ok();
    }
    r.save_png(&frame, w, h, std::path::Path::new(&ra.out)).expect("save png");
    println!("{} skinned draws -> {}", r.stats.skinned, ra.out);
}

/// One swatch per textured material: a wall section (with a floor patch in front), projected in
/// world space like walls and floors are.
fn materials(ra: &RenderArgs) {
    use glam::{Mat4, Quat, Vec3};
    use hearth_render::{Draw, flags};
    let gpu = hearth_render::Gpu::headless().expect("no GPU adapter");
    let mut r = hearth_render::Renderer::new(gpu, hearth_render::wgpu::TextureFormat::Rgba8UnormSrgb);
    game::materials::install(&mut r, &sim::data::GameData::embedded());
    let mut names: Vec<(String, hearth_render::MaterialId)> = r.builtins.materials.iter().map(|(n, m)| (n.clone(), *m)).collect();
    names.sort();
    let (w, h) = ra.size;
    let spacing = 3.0;
    let n = names.len().max(1) as f32;
    // `render`'s camera defaults mean "not given": frame the whole row from the front
    let yaw = if ra.yaw == 35.0 { 15.0 } else { ra.yaw };
    let pitch = if ra.pitch == 45.0 { 20.0 } else { ra.pitch };
    let dist = if ra.dist == 30.0 { 3.2 * n + 2.0 } else { ra.dist };
    let target = ra.target.map_or(Vec3::new(0.0, 1.0, 0.5), |(x, z)| Vec3::new(x, 1.0, z));
    let mut frame = hearth_render::Frame::new(hearth_render::Camera::orbit(target, yaw.to_radians(), pitch.to_radians(), dist, w as f32 / h as f32));
    frame.shadow_radius = 2.0 * n + 4.0;
    if let Some(hour) = ra.hour {
        let t = ((hour - 6.0) / 12.0).clamp(0.0, 1.0) * std::f32::consts::PI;
        frame.sun_dir = Vec3::new(-t.cos(), t.sin().max(0.15), 0.6).normalize();
    }
    frame.draws.push(Draw::new(r.builtins.cube, Mat4::from_scale_rotation_translation(Vec3::new(spacing * n + 4.0, 0.1, 8.0), Quat::IDENTITY, Vec3::new(0.0, -0.06, 1.0)), [0.45, 0.5, 0.4]).rough(0.95));
    for (i, (_, m)) in names.iter().enumerate() {
        let x = (i as f32 - (n - 1.0) * 0.5) * spacing;
        let wall = Mat4::from_scale_rotation_translation(Vec3::new(2.4, 2.8, 0.12), Quat::IDENTITY, Vec3::new(x, 1.4, 0.0));
        let floor = Mat4::from_scale_rotation_translation(Vec3::new(2.4, 0.06, 2.4), Quat::IDENTITY, Vec3::new(x, 0.0, 1.3));
        for model in [wall, floor] {
            frame.draws.push(Draw::new(r.builtins.cube, model, [1.0; 3]).material(Some(*m)).flags(flags::WORLD_TEXTURE));
        }
    }
    if let Some(dir) = std::path::Path::new(&ra.out).parent() {
        std::fs::create_dir_all(dir).ok();
    }
    r.save_png(&frame, w, h, std::path::Path::new(&ra.out)).expect("save png");
    println!("{} materials ({}) -> {}", names.len(), names.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "), ra.out);
}

/// Orders every peon at home to do `OBJECT:INTERACTION` and runs (free will off) until they all are,
/// or two hours pass.
fn stage_order(sim: &mut sim::Sim, order: &str) {
    use sim::components::{ActionKind, ActionQueue, Phase};
    let (obj_id, inter_id) = order.split_once(':').expect("--order OBJECT:INTERACTION");
    let data = sim.data();
    let obj = sim
        .objects()
        .into_iter()
        .find(|e| sim.world.get::<sim::objects::ObjectInstance>(*e).is_some_and(|o| data.object(o.def).id == obj_id))
        .unwrap_or_else(|| panic!("no {obj_id} on the lot"));
    let def = data.object(sim.world.get::<sim::objects::ObjectInstance>(obj).unwrap().def);
    let interaction = def.interactions.iter().position(|i| i.id == inter_id).unwrap_or_else(|| panic!("{obj_id} has no {inter_id}"));
    sim.world.res_mut::<sim::time::Settings>().free_will = sim::time::FreeWill::Off;
    let home: Vec<_> = sim.living_peons().into_iter().filter(|&p| !sim.world.has::<sim::components::AtWork>(p)).collect();
    let mut taking = Vec::new();
    for &p in &home {
        match sim.order(p, ActionKind::Object { obj, interaction }) {
            Ok(()) => taking.push(p),
            Err(why) => println!("  {} won't: {why}", sim.name(p)),
        }
    }
    let since = sim.world.res::<sim::log::SimLog>().total;
    let doing = |sim: &sim::Sim, p| {
        sim.world.get::<ActionQueue>(p).is_some_and(|q| {
            q.current.as_ref().is_some_and(|c| matches!(c.action.kind, ActionKind::Object { obj: o, .. } if o == obj) && matches!(c.phase, Phase::Performing { .. }))
        })
    };
    for i in 0..120 {
        // (orders reach the action queue on the next tick)
        if i >= 2 && taking.iter().all(|&p| doing(sim, p) || sim.world.get::<ActionQueue>(p).is_none_or(|q| q.current.is_none() && q.queue.is_empty())) {
            break;
        }
        sim.tick();
    }
    {
        let log = sim.world.res::<sim::log::SimLog>();
        let new = (log.total - since) as usize;
        for e in log.entries.iter().rev().take(new).rev() {
            println!("  log: {}", e.text);
        }
    }
    for &p in &taking {
        let now = sim.world.get::<ActionQueue>(p).and_then(|q| q.current.as_ref().map(|c| format!("{} ({:?})", c.action.label, std::mem::discriminant(&c.phase))));
        println!("  {}: {}", sim.name(p), if doing(sim, p) { "doing it".to_string() } else { format!("not doing it — {}", now.unwrap_or("idle".into())) });
    }
}

/// Makes `lot` the live one, the way the player would get there.
fn visit_lot(sim: &mut Sim, lot: usize) {
    use sim::neighborhood::{self as nh, LotKind};
    if lot == nh::active_lot(&sim.world) {
        return;
    }
    let kind = sim.world.res::<nh::Neighborhood>().lots.get(lot).map(|l| l.kind).unwrap_or_else(|| panic!("no lot {lot}"));
    let r = match (kind, nh::residents(&sim.world, lot)) {
        (LotKind::Community, _) => sim.go_out(lot).map(|()| {
            sim.finish_trip();
        }),
        (LotKind::Residential, Some(_)) => sim.play_household(lot),
        (LotKind::Residential, None) => {
            let r = nh::switch_lot(&mut sim.world, lot, &[]);
            sim.world.flush();
            r
        }
    };
    r.unwrap_or_else(|e| panic!("can't get to lot {lot}: {e}"));
}

fn print_street(sim: &Sim) {
    use sim::neighborhood as nh;
    let w = &sim.world;
    let hood = w.res::<nh::Neighborhood>().clone();
    println!("{} — {}", hood.name, sim.clock().label());
    for (i, lot) in hood.lots.iter().enumerate() {
        let who = match nh::residents(w, i) {
            Some(r) => {
                let people: Vec<String> = r.members.iter().map(|e| {
                    let n = sim.name(*e);
                    let stage = w.get::<sim::components::Age>(*e).map(|a| a.stage.label()).unwrap_or("?");
                    if w.has::<Dead>(*e) { format!("{n} (✝)") } else { format!("{n} ({stage})") }
                }).collect();
                format!("{}{} ${}: {}", r.name, if r.played { " [played]" } else { "" }, r.funds, people.join(", "))
            }
            None => format!("{:?}, vacant", lot.kind),
        };
        let here = if i == hood.active { "*" } else { " " };
        let graves = match &lot.parked {
            Some(p) => p.objects.iter().filter(|o| o.def == sim::ghosts::GRAVE_OBJECT).count(),
            None => sim.objects().iter().filter(|&&o| w.get::<sim::objects::ObjectInstance>(o).is_some_and(|x| sim.data().object(x.def).id == sim::ghosts::GRAVE_OBJECT)).count(),
        };
        let graves = if graves > 0 { format!("  ✝×{graves}") } else { String::new() };
        println!("{here}{i:>2} {:<34} value ${:>6}  {who}{graves}", lot.title(), nh::build_value(w, i));
    }
}

fn hood_tour(seed: u64, days: u64, log_path: Option<&str>) {
    use sim::neighborhood::{self as nh, LotKind};
    let mut sim = starter_sim(seed);
    sim.world.res_mut::<sim::time::Settings>().auto_career_choices = true;
    sim.populate_neighborhood();
    let park = {
        let hood = sim.world.res::<nh::Neighborhood>();
        hood.lots.iter().position(|l| l.kind == LotKind::Community).unwrap()
    };
    let t0 = Instant::now();
    for day in 0..days {
        let until = |sim: &mut Sim, h: u32| {
            while sim.clock().hour() != h {
                sim.tick();
            }
        };
        until(&mut sim, 10);
        match day % 3 {
            0 => {
                if let Err(e) = sim.go_out(park) {
                    println!("day {day}: no outing ({e})");
                }
                until(&mut sim, 15);
                if !nh::at_home(&sim.world) && sim.trip().is_none() && sim.go_home().is_ok() {
                    sim.finish_trip();
                }
            }
            1 => {
                // whoever lives next door now (households merge, move and die out)
                let next_door = sim.world.res::<nh::Neighborhood>().households.first().and_then(|h| h.home);
                if let Some(lot) = next_door.filter(|_| nh::at_home(&sim.world)) {
                    // follow a member back: a parked household can merge into another meanwhile
                    let ours = sim.world.res::<sim::economy::Household>().members.clone();
                    sim.play_household(lot).unwrap();
                    until(&mut sim, 9);
                    let back = sim.world.res::<nh::Neighborhood>().households.iter()
                        .find(|h| h.members.iter().any(|m| ours.contains(m)))
                        .and_then(|h| h.home);
                    match back {
                        Some(lot) => sim.play_household(lot).unwrap(),
                        None => println!("day {day}: our household is gone; staying with the neighbors"),
                    }
                }
            }
            _ => {
                // drop in on whoever lives there now
                let lot = sim.world.res::<nh::Neighborhood>().households.first().and_then(|h| h.home);
                if let Some(lot) = lot.filter(|_| nh::at_home(&sim.world)) {
                    match sim.visit(lot) {
                        Ok(()) => {
                            until(&mut sim, 15);
                            // everyone may have gone off to work or school from there
                            if sim.go_home().is_ok() {
                                sim.finish_trip();
                            }
                        }
                        Err(e) => println!("day {day}: no visit ({e})"),
                    }
                }
            }
        }
        until(&mut sim, 0);
    }
    print_street(&sim);
    if let Some(path) = log_path {
        let text: Vec<String> = sim.world.res::<sim::log::SimLog>().entries.iter().map(|e| format!("[{}] {}", sim::time::Clock { tick: e.tick }.label(), e.text)).collect();
        std::fs::write(path, text.join("\n")).expect("write --log");
    }
    let log = sim.world.res::<sim::log::SimLog>();
    let count = |s: &str| log.entries.iter().filter(|e| e.text.contains(s)).count();
    println!("\nvisits: {} at home, {} out and about, {} to the neighbors; newcomers: {}; neighbor promotions: {}; births: {}",
        count("dropped by") + count("rang the doorbell") + count("cup of sugar") + count("casserole") + count("wandered over"),
        count("too."), count("dropped in on"), count("moving van"), count("Word on the street"), count("had a baby"));
    println!("romance: {} engaged, {} weddings, {} called off at the altar, {} breakups/divorces, {} moved in together, {} moved out, {} jealous scenes",
        count("said yes") + count("answer was not"), count("are married!") + count("at the courthouse"), count("cold feet"),
        count("broke up with") + count("getting divorced"), count("moved in with"), count("moved out to"), count("is Not Happy") + count("made a scene"));
    println!("simulated {days} days in {:.1}s", t0.elapsed().as_secs_f32());
    for e in log.entries.iter().filter(|e| e.kind == LogKind::Death || e.text.contains("up for sale")) {
        println!("  [{}] {}", sim::time::Clock { tick: e.tick }.label(), e.text);
    }
    let neglect: Vec<String> = log.entries.iter().filter(|e| e.kind == LogKind::Death && e.text.contains("starvation")).map(|e| e.text.clone()).collect();
    drop(log);
    if !neglect.is_empty() {
        println!("\n!!! deaths by neglect:\n  {}", neglect.join("\n  "));
        std::process::exit(1);
    }
}

fn render(a: &Args) {
    use game::scene::{SceneOptions, WallMode, build};
    let ra = &a.render;
    let mut sim = starter_sim(a.seed);
    sim.world.res_mut::<sim::time::Settings>().auto_career_choices = true;
    if let Some(lot) = ra.lot {
        sim.populate_neighborhood();
        visit_lot(&mut sim, lot);
    }
    let mut minutes = ra.minutes.max(1);
    if let Some(h) = ra.hour {
        let now = sim.clock().minute_of_day() as f32;
        let want = h * 60.0;
        let delta = if want >= now { want - now } else { 1440.0 - now + want };
        minutes = minutes.max(delta as u64);
    }
    sim.run_minutes(minutes);
    if let Some(order) = &ra.order {
        stage_order(&mut sim, order);
    }
    let gpu = hearth_render::Gpu::headless().expect("no GPU adapter");
    let mut r = hearth_render::Renderer::new(gpu, hearth_render::wgpu::TextureFormat::Rgba8UnormSrgb);
    game::materials::install(&mut r, &sim.data());
    if !ra.greybox {
        game::object_models::install(&mut r, &sim.data());
    }
    let (w, h) = ra.size;
    let target = ra.target.map(|(x, z)| glam::Vec3::new(x, 0.0, z)).unwrap_or(glam::Vec3::new(15.0, 0.0, 14.0));
    let mut cam = game::camera::OrbitCamera::new(target);
    cam.yaw = ra.yaw.to_radians();
    cam.pitch = ra.pitch.to_radians();
    cam.dist = ra.dist;
    cam.snap();
    let camera = cam.camera(ra.level, w as f32 / h as f32);
    let mut meshes = game::lotmesh::LotMeshes::default();
    meshes.sync(&mut r, &sim.world.res::<sim::lot::Lot>());
    let opts = SceneOptions {
        terrain_mesh: meshes.terrain,
        roof_meshes: meshes.roofs.clone(),
        active_level: ra.level,
        wall_mode: match ra.mode.as_str() {
            "up" => WallMode::Up,
            "down" => WallMode::Down,
            _ => WallMode::Cutaway,
        },
        selected: sim.living_peons().first().copied(),
        show_rooms: ra.rooms,
        show_paths: ra.rooms,
        focus: cam.focus(ra.level),
        ..Default::default()
    };
    let peons = if ra.greybox { None } else { Some(game::peons::PeonArt::load(&mut r).expect("peon models")) };
    let out = build(&sim, &r.builtins, peons.as_ref(), &mut game::peons::Animator::default(), camera, &opts);
    if let Some(dir) = std::path::Path::new(&ra.out).parent() {
        std::fs::create_dir_all(dir).ok();
    }
    r.save_png(&out.frame, w, h, std::path::Path::new(&ra.out)).expect("save png");
    println!("{} at {} — {} draws, {} labels -> {}", sim.world.res::<sim::economy::Household>().name, sim.clock().label(), r.stats.draws, out.labels.len(), ra.out);
    for l in out.labels.iter().take(10) {
        println!("  label {:?} {:?}: {}", l.kind, l.world, l.text);
    }
}
