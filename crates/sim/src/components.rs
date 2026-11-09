//! Peon components.

use std::collections::VecDeque;

use glam::{IVec2, Vec3};
use hearth_ecs::Entity;
use serde::{Deserialize, Serialize};

use crate::data::{LifeStage, Need, Pose, Skill};
use crate::time::FreeWill;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peon {
    pub first: String,
    pub last: String,
}

impl Peon {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first, self.last)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Age {
    pub stage: LifeStage,
    pub days_in_stage: f32,
    /// Length of the current stage in days (already scaled & randomized).
    pub stage_len: f32,
    pub total_days: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Needs {
    pub v: [f32; Need::COUNT],
    pub active: [bool; Need::COUNT],
    /// Minutes spent at zero hunger (starvation clock).
    pub starving_minutes: u32,
}

impl Needs {
    pub fn new() -> Self {
        let mut active = [false; Need::COUNT];
        for n in Need::CORE {
            active[n.idx()] = true;
        }
        Needs { v: [80.0; Need::COUNT], active, starving_minutes: 0 }
    }
    pub fn get(&self, n: Need) -> f32 {
        self.v[n.idx()]
    }
    pub fn add(&mut self, n: Need, d: f32) {
        let x = &mut self.v[n.idx()];
        *x = (*x + d).clamp(0.0, 100.0);
    }
    pub fn set(&mut self, n: Need, v: f32) {
        self.v[n.idx()] = v.clamp(0.0, 100.0);
    }
    pub fn is_active(&self, n: Need) -> bool {
        self.active[n.idx()]
    }
    pub fn active_needs(&self) -> impl Iterator<Item = Need> + '_ {
        Need::ALL.into_iter().filter(|n| self.is_active(*n))
    }
    /// Needs with real consequences at zero (accidents, collapse, starvation).
    pub const PHYSICAL: [Need; 3] = [Need::Hunger, Need::Energy, Need::Bladder];

    /// Physical needs currently below `threshold`.
    pub fn critical_physical(&self, threshold: f32) -> Vec<Need> {
        Self::PHYSICAL.into_iter().filter(|n| self.get(*n) < threshold).collect()
    }

    /// The need to worry about first: the lowest physical need if one is low, else the lowest core need.
    pub fn most_urgent(&self) -> (Need, f32) {
        let phys = Self::PHYSICAL
            .into_iter()
            .map(|n| (n, self.get(n)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        if phys.1 < 20.0 { phys } else { self.lowest_core() }
    }

    /// Lowest active core need.
    pub fn lowest_core(&self) -> (Need, f32) {
        Need::CORE
            .into_iter()
            .filter(|n| self.is_active(*n))
            .map(|n| (n, self.get(n)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap()
    }
}

impl Default for Needs {
    fn default() -> Self {
        Self::new()
    }
}

/// Personality axes, each 0–10.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Axes {
    pub neat: u8,
    pub outgoing: u8,
    pub active: u8,
    pub playful: u8,
    pub nice: u8,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Personality {
    pub axes: Axes,
    pub traits: Vec<String>,
    pub quirk: String,
}

impl Personality {
    pub fn has(&self, t: &str) -> bool {
        self.traits.iter().any(|x| x == t)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Skills {
    pub v: [f32; Skill::COUNT],
}

impl Skills {
    pub fn get(&self, s: Skill) -> f32 {
        self.v[s.idx()]
    }
    /// Returns true if a whole level was gained.
    pub fn add(&mut self, s: Skill, d: f32) -> bool {
        let before = self.v[s.idx()].floor();
        self.v[s.idx()] = (self.v[s.idx()] + d).clamp(0.0, 10.0);
        self.v[s.idx()].floor() > before
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActiveMoodlet {
    pub id: String,
    pub name: String,
    pub value: f32,
    pub until: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mood {
    pub value: f32,
    pub moodlets: Vec<ActiveMoodlet>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Transform {
    pub pos: Vec3,
    pub yaw: f32,
    /// Position/yaw at the previous tick, for render interpolation.
    pub prev_pos: Vec3,
    pub prev_yaw: f32,
}

impl Transform {
    pub fn at(pos: Vec3) -> Self {
        Transform { pos, yaw: 0.0, prev_pos: pos, prev_yaw: 0.0 }
    }
    pub fn level(&self) -> i32 {
        crate::lot::Lot::level_of(self.pos)
    }
    pub fn tile(&self) -> IVec2 {
        crate::lot::Lot::tile_of(self.pos)
    }
}

/// Genes that drive appearance (placeholder bodies now, morph targets later).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Appearance {
    pub skin: [f32; 3],
    pub hair: [f32; 3],
    pub shirt: [f32; 3],
    pub pants: [f32; 3],
    /// Multiplier on stage height (0.9–1.1).
    pub height: f32,
    /// 0 = slim, 1 = heavy.
    pub build: f32,
    /// Index of hair style.
    pub hair_style: u8,
}

/// What the body is currently doing (for rendering/animation).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Posture {
    pub pose: Pose,
    pub walking: bool,
    /// Specific clip for the current interaction (see `InteractionDef::anim`); None = by `pose`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<String>,
    /// Prop id held (see `PropDef`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prop: Option<String>,
}

impl Posture {
    pub fn performing(pose: Pose, anim: Option<String>, prop: Option<String>) -> Posture {
        Posture { pose, walking: false, anim, prop }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Bubble {
    pub icon: Option<String>,
    pub speech: Option<String>,
    pub until: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Player,
    Autonomy,
    /// Scheduled by the sim itself (work, emergencies).
    System,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ActionKind {
    Object { obj: Entity, interaction: usize },
    Social { target: Entity, social: usize },
    /// Being the target of someone else's social interaction.
    Respond { initiator: Entity, social: usize, minutes: u32 },
    GoTo { level: i32, tile: IVec2 },
    GoToWork,
    GoToSchool,
    /// Collapse on the floor from exhaustion.
    PassOut,
    /// Stand around / fidget, optionally muttering.
    Idle { minutes: u32 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueuedAction {
    pub id: u64,
    pub kind: ActionKind,
    pub source: Source,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Phase {
    Start,
    Walking {
        path: Vec<Vec3>,
        idx: usize,
        slot: Option<usize>,
        retries: u8,
        /// Minutes spent waiting for someone to get out of the way.
        #[serde(default)]
        blocked: u8,
    },
    Performing { slot: Option<usize>, elapsed: u32 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurrentAction {
    pub action: QueuedAction,
    pub phase: Phase,
    pub started: u64,
    /// Set when the action should wrap up at the next opportunity.
    pub cancel: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ActionQueue {
    pub current: Option<CurrentAction>,
    pub queue: VecDeque<QueuedAction>,
}

/// A scored autonomy option, kept for the "why did they do that?" debug overlay.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoredOption {
    pub label: String,
    pub score: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Brain {
    /// Overrides the global free-will setting for this peon.
    pub free_will: Option<FreeWill>,
    pub next_think: u64,
    pub last_scores: Vec<ScoredOption>,
    pub last_think_tick: u64,
    pub idle_streak: u32,
    /// Recently completed activities (label, tick) — repeated activities get boring.
    pub recent: Vec<(String, u64)>,
    /// A critical need nothing on the lot can currently fix (suppresses critical interrupts).
    pub unfixable: Option<Need>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub career: String,
    pub level: usize,
    /// -100..100; promotion at 100.
    pub performance: f32,
    pub days_worked: u32,
    /// Absolute tick of the last shift start (avoid double scheduling).
    pub last_shift_day: Option<u64>,
}

/// Peon is off-lot at work until `until`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AtWork {
    pub until: u64,
    pub event_at: u64,
    pub event_done: bool,
    /// At school rather than at a job.
    #[serde(default)]
    pub school: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Family {
    pub parents: Vec<Entity>,
    pub children: Vec<Entity>,
    pub spouse: Option<Entity>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dead {
    pub cause: String,
    pub tick: u64,
}

/// Marks a peon as part of the player's household.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Member;

/// Inheritable genotype. [`Appearance`] is the phenotype (elders go grey; clothes aren't genetic).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Genes {
    pub skin: [f32; 3],
    pub hair: [f32; 3],
    pub eyes: [f32; 3],
    /// Multiplier on stage height (0.9–1.1).
    pub height: f32,
    /// 0 = slim, 1 = heavy.
    pub build: f32,
}

/// Enrolled in school (Child and Teen).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Student {
    /// -100..100. Report cards are derived from this.
    pub grade: f32,
    /// Day index of the last school day attended or skipped (avoid double scheduling).
    pub last_school_day: Option<u64>,
    /// Homework done since the last school day.
    pub homework_done: bool,
}

impl Student {
    pub fn new() -> Self {
        Student { grade: 0.0, last_school_day: None, homework_done: false }
    }
    pub fn letter(&self) -> &'static str {
        match self.grade {
            g if g >= 60.0 => "A",
            g if g >= 25.0 => "B",
            g if g >= -10.0 => "C",
            g if g >= -50.0 => "D",
            _ => "F",
        }
    }
}

impl Default for Student {
    fn default() -> Self {
        Self::new()
    }
}

/// Expecting a baby with `other` (who may since have died or left).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pregnant {
    pub other: Entity,
    pub since: u64,
    pub due: u64,
}

/// A dead peon who lingers as a ghost, bound to their grave marker object.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ghost {
    pub grave: Entity,
    /// Visible on the lot until this tick (0 = not manifested).
    pub visible_until: u64,
    /// Where the ghost is drifting to while manifested.
    pub drift_to: Option<Vec3>,
    pub next_prank: u64,
}
