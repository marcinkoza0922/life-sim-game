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
//!               [--target X,Z] [--mode cutaway|up|down] [--size WxH] [--rooms] [--out FILE]
//!     Runs the starter household for M minutes and renders the lot offscreen to a PNG.
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
    out: String,
}

fn parse() -> Args {
    let mut a = Args { cmd: "run".into(), days: 3, seed: 1, seeds: 8, echo: false, log: None, manual: false, trace: None,
        render: RenderArgs { minutes: 60, hour: None, level: 0, yaw: 35.0, pitch: 45.0, dist: 30.0, target: None, mode: "cutaway".into(), size: (1600, 1000), rooms: false, out: "out/render.png".into() } };
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
            "--out" => a.render.out = it.next().unwrap(),
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
            for i in 0..Need::COUNT {
                if n.active[i] {
                    m[i] = m[i].min(n.v[i]);
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
        if let Some(d) = sim.world.get::<Dead>(e) {
            if d.cause != "old age" {
                fails.push(format!("{name} died of {}", d.cause));
            }
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
        other => {
            eprintln!("unknown command {other}; try `run`, `check` or `render`");
            std::process::exit(2);
        }
    }
}

fn render(a: &Args) {
    use game::scene::{SceneOptions, WallMode, build};
    let ra = &a.render;
    let mut sim = starter_sim(a.seed);
    sim.world.res_mut::<sim::time::Settings>().auto_career_choices = true;
    let mut minutes = ra.minutes.max(1);
    if let Some(h) = ra.hour {
        let now = sim.clock().minute_of_day() as f32;
        let want = h * 60.0;
        let delta = if want >= now { want - now } else { 1440.0 - now + want };
        minutes = minutes.max(delta as u64);
    }
    sim.run_minutes(minutes);
    let gpu = hearth_render::Gpu::headless().expect("no GPU adapter");
    let mut r = hearth_render::Renderer::new(gpu, hearth_render::wgpu::TextureFormat::Rgba8UnormSrgb);
    let (w, h) = ra.size;
    let target = ra.target.map(|(x, z)| glam::Vec3::new(x, 0.0, z)).unwrap_or(glam::Vec3::new(15.0, 0.0, 14.0));
    let mut cam = game::camera::OrbitCamera::new(target);
    cam.yaw = ra.yaw.to_radians();
    cam.pitch = ra.pitch.to_radians();
    cam.dist = ra.dist;
    cam.snap();
    let camera = cam.camera(ra.level, w as f32 / h as f32);
    let opts = SceneOptions {
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
    let out = build(&sim, &r.builtins, camera, &opts);
    if let Some(dir) = std::path::Path::new(&ra.out).parent() {
        std::fs::create_dir_all(dir).ok();
    }
    r.save_png(&out.frame, w, h, std::path::Path::new(&ra.out)).expect("save png");
    println!("{} at {} — {} draws, {} labels -> {}", sim.world.res::<sim::economy::Household>().name, sim.clock().label(), r.stats.draws, out.labels.len(), ra.out);
    for l in out.labels.iter().take(10) {
        println!("  label {:?} {:?}: {}", l.kind, l.world, l.text);
    }
}
