//! Deterministic CPU stress runner; see tools/profiling/README.md.
use glam::{IVec2, Vec3};
use sim::{
    components::Transform,
    data::LifeStage,
    lot::Lot,
    nav::{NavField, NavNode},
    sim::{random_spec, spawn_peon},
    starter::starter_sim,
};
use std::{collections::BTreeMap, hint::black_box, time::Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("tick");
    let count: usize = args
        .get(2)
        .map(|s| s.parse().expect("positive population/size"))
        .unwrap_or(4);
    let iterations: usize = args
        .get(3)
        .map(|s| s.parse().expect("positive iterations"))
        .unwrap_or(1440);
    let seed: u64 = args.get(4).map(|s| s.parse().expect("seed")).unwrap_or(1);
    assert!(count > 0 && iterations > 0);
    let mut sim = starter_sim(seed);
    while sim.living_peons().len() < count {
        let mut spec = random_spec(&sim.world, LifeStage::Adult, None);
        spec.career = None;
        spawn_peon(&mut sim.world, spec, Vec3::new(16.5, 0.0, 22.5));
    }
    let initial_population = sim.living_peons().len();
    // Warm caches and let startup decisions settle outside the timed window.
    sim.run_minutes(60);
    let data = sim.world.res::<sim::sim::Data>().0.clone();
    let peons = sim.living_peons();
    let lot = Lot::new(count as i32, count as i32);
    let mut samples = Vec::with_capacity(iterations);
    let mut systems = BTreeMap::<String, f64>::new();
    let mut checksum = 0usize;
    #[cfg(feature = "profiling")]
    sim::profiling::reset();
    let wall = Instant::now();
    for i in 0..iterations {
        let start = Instant::now();
        match mode {
            "tick" => sim.tick(),
            "autonomy" => {
                checksum += black_box(sim::autonomy::candidates(
                    &sim.world,
                    peons[i % peons.len()],
                    &data,
                ))
                .len()
            }
            "nav" => {
                black_box(NavField::flood(&lot, NavNode::new(0, IVec2::ZERO), 200.0));
            }
            "nav-house" => {
                let tf = *sim.world.get::<Transform>(peons[i % peons.len()]).unwrap();
                black_box(NavField::flood(
                    &sim.world.res::<Lot>(),
                    NavNode::new(tf.level(), tf.tile()),
                    200.0,
                ));
            }
            "save" => checksum += black_box(sim.save_to_string().unwrap()).len(),
            _ => panic!("mode must be tick, autonomy, nav, nav-house, or save"),
        }
        samples.push(start.elapsed().as_secs_f64() * 1e6);
        if mode == "tick" {
            for t in sim.timings() {
                *systems
                    .entry(format!("{}/{}", t.stage, t.system))
                    .or_default() += t.last.as_secs_f64() * 1e6;
            }
        }
    }
    let wall_seconds = wall.elapsed().as_secs_f64();
    let total: f64 = samples.iter().sum();
    samples.sort_by(f64::total_cmp);
    let percentile = |p: f64| samples[((iterations - 1) as f64 * p).ceil() as usize];
    let system_mean_us: BTreeMap<_, _> = systems
        .into_iter()
        .map(|(k, v)| (k, v / iterations as f64))
        .collect();
    #[cfg(feature = "profiling")]
    let spans: BTreeMap<_, _> = sim::profiling::NAMES
        .into_iter()
        .zip(sim::profiling::snapshot())
        .map(|(name, m)| {
            (
                name,
                serde_json::json!({
                    "calls": m.calls, "total_us": m.elapsed.as_secs_f64() * 1e6
                }),
            )
        })
        .collect();
    #[cfg(not(feature = "profiling"))]
    let spans = BTreeMap::<String, serde_json::Value>::new();
    println!(
        "{}",
        serde_json::json!({
            "mode": mode, "count": count, "seed": seed, "iterations": iterations,
            "initial_population": initial_population, "final_population": sim.living_peons().len(),
            "mean_us": total / iterations as f64, "p50_us": percentile(0.5),
            "p95_us": percentile(0.95), "p99_us": percentile(0.99), "max_us": samples[iterations-1],
            "wall_seconds": wall_seconds, "inclusive_spans": spans, "system_mean_us": system_mean_us, "checksum": checksum,
        })
    );
}
