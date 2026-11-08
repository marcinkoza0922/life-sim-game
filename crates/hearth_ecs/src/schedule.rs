use std::time::{Duration, Instant};

use crate::world::World;

type SystemFn = Box<dyn FnMut(&mut World) + Send>;

struct System {
    name: String,
    run: SystemFn,
    last: Duration,
    avg: Duration,
}

struct Stage {
    name: String,
    systems: Vec<System>,
}

/// Timing info for profiling overlays.
#[derive(Clone, Debug)]
pub struct SystemTiming {
    pub stage: String,
    pub system: String,
    pub last: Duration,
    pub avg: Duration,
}

/// Ordered stages of systems. Systems are plain `FnMut(&mut World)`; deferred commands are
/// flushed after every system so later systems observe structural changes.
#[derive(Default)]
pub struct Schedule {
    stages: Vec<Stage>,
}

impl Schedule {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_stage(&mut self, name: &str) -> &mut Self {
        assert!(self.stages.iter().all(|s| s.name != name), "duplicate stage {name}");
        self.stages.push(Stage { name: name.to_string(), systems: Vec::new() });
        self
    }

    /// Adds a system to the end of `stage`. Its name is derived from the function's type name.
    pub fn add_system<F: FnMut(&mut World) + Send + 'static>(&mut self, stage: &str, f: F) -> &mut Self {
        let full = std::any::type_name::<F>();
        let name = full.rsplit("::").next().unwrap_or(full).to_string();
        self.add_named_system(stage, &name, f)
    }

    pub fn add_named_system<F: FnMut(&mut World) + Send + 'static>(
        &mut self,
        stage: &str,
        name: &str,
        f: F,
    ) -> &mut Self {
        let s = self
            .stages
            .iter_mut()
            .find(|s| s.name == stage)
            .unwrap_or_else(|| panic!("no stage named {stage}"));
        s.systems.push(System { name: name.to_string(), run: Box::new(f), last: Duration::ZERO, avg: Duration::ZERO });
        self
    }

    pub fn run(&mut self, world: &mut World) {
        for stage in &mut self.stages {
            for sys in &mut stage.systems {
                let t = Instant::now();
                (sys.run)(world);
                world.flush();
                sys.last = t.elapsed();
                sys.avg = sys.avg.mul_f64(0.95) + sys.last.mul_f64(0.05);
            }
        }
    }

    pub fn timings(&self) -> Vec<SystemTiming> {
        self.stages
            .iter()
            .flat_map(|st| {
                st.systems.iter().map(|s| SystemTiming {
                    stage: st.name.clone(),
                    system: s.name.clone(),
                    last: s.last,
                    avg: s.avg,
                })
            })
            .collect()
    }
}
