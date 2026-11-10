//! Opt-in capacity checks: no machine-dependent timing assertions.
use glam::Vec3;
use sim::{
    components::{Needs, Transform},
    data::LifeStage,
    sim::{random_spec, spawn_peon},
    starter::starter_sim,
};

#[test]
#[ignore = "capacity/soak test; run explicitly with nextest --run-ignored ignored-only"]
fn crowded_household_stays_finite() {
    for seed in [1, 7, 42] {
        let mut sim = starter_sim(seed);
        while sim.living_peons().len() < 64 {
            let mut spec = random_spec(&sim.world, LifeStage::Adult, None);
            spec.career = None;
            spawn_peon(&mut sim.world, spec, Vec3::new(16.5, 0.0, 22.5));
        }
        let start = sim.clock().tick;
        for _ in 0..1440 {
            sim.tick();
            for e in sim.living_peons() {
                assert!(sim.world.get::<Transform>(e).unwrap().pos.is_finite());
                for v in sim.world.get::<Needs>(e).unwrap().v {
                    assert!(v.is_finite() && (0.0..=100.0).contains(&v));
                }
            }
        }
        assert_eq!(sim.clock().tick, start + 1440);
        let save = sim
            .save_to_string()
            .expect("crowded world remains serializable");
        assert!(!save.is_empty());
    }
}

#[test]
#[ignore = "large navigation capacity test"]
fn large_navigation_reaches_far_corner() {
    use glam::IVec2;
    use sim::{
        lot::Lot,
        nav::{NavField, NavNode},
    };
    for size in [32, 64, 128] {
        let lot = Lot::new(size, size);
        let start = NavNode::new(0, IVec2::ZERO);
        let end = NavNode::new(0, IVec2::splat(size - 1));
        let field = NavField::flood(&lot, start, 1000.0);
        assert!(field.distance(end).unwrap().is_finite());
        let path = field.path_to(end).unwrap();
        assert_eq!(path.first(), Some(&start));
        assert_eq!(path.last(), Some(&end));
    }
}

#[test]
#[ignore = "crowded deterministic replay and save/restore check"]
fn crowded_replay_and_restore() {
    fn crowded() -> sim::Sim {
        let mut sim = starter_sim(42);
        while sim.living_peons().len() < 16 {
            let mut spec = random_spec(&sim.world, LifeStage::Adult, None);
            spec.career = None;
            spawn_peon(&mut sim.world, spec, Vec3::new(16.5, 0.0, 22.5));
        }
        sim.run_minutes(240);
        sim
    }
    let mut original = crowded();
    let saved = original.save_to_string().unwrap();
    assert_eq!(saved, crowded().save_to_string().unwrap());
    let data = (*original.world.res::<sim::sim::Data>().0).clone();
    let mut restored = sim::Sim::load_from_str(&saved, data).unwrap();
    assert_eq!(original.living_peons().len(), restored.living_peons().len());
    original.run_minutes(120);
    restored.run_minutes(120);
    assert_eq!(original.clock().tick, restored.clock().tick);
    assert_eq!(original.summary(), restored.summary());
}
