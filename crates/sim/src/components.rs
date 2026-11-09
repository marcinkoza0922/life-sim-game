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

/// Biological sex. Decides who can carry a pregnancy, voice pitch, and which couples can get
/// engaged and married (opposite-sex only, for now).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Sex {
    #[default]
    Female,
    Male,
}

impl Sex {
    pub fn opposite(self) -> Sex {
        match self {
            Sex::Female => Sex::Male,
            Sex::Male => Sex::Female,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Sex::Female => "Female",
            Sex::Male => "Male",
        }
    }
    pub fn symbol(self) -> &'static str {
        match self {
            Sex::Female => "♀",
            Sex::Male => "♂",
        }
    }
    /// A stable pick for peons from before sex was modelled (no randomness consumed).
    pub fn from_entity(e: Entity) -> Sex {
        if (e.index().wrapping_mul(2_654_435_761) >> 15) & 1 == 0 { Sex::Female } else { Sex::Male }
    }
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

/// What gets rendered: the expressed genes (see [`crate::genetics::express`]) plus the clothes and
/// grooming a peon picked. Fields added after the first saves default when missing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    pub skin: [f32; 3],
    pub hair: [f32; 3],
    /// Top colour.
    pub shirt: [f32; 3],
    /// Bottom colour.
    pub pants: [f32; 3],
    /// Multiplier on stage height (0.9–1.1).
    pub height: f32,
    /// 0 = slim, 1 = heavy.
    pub build: f32,
    /// Index into [`crate::genetics::HAIR_STYLES`].
    pub hair_style: u8,
    /// Iris colour (from [`Genes::eyes`]; saves from before eye colour default to brown).
    #[serde(default = "crate::genetics::default_eyes")]
    pub eyes: [f32; 3],
    /// Face and body shape sliders (ids in [`crate::genetics::FACE_SLIDERS`] and
    /// [`crate::genetics::BODY_SLIDERS`]), each -1..1; missing = 0 (average).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub shape: std::collections::BTreeMap<String, f32>,
    /// Index into [`crate::genetics::FACIAL_HAIR`] (0 = clean-shaven).
    #[serde(default)]
    pub facial_hair: u8,
    /// Index into [`crate::genetics::TOPS`].
    #[serde(default)]
    pub top: u8,
    /// Index into [`crate::genetics::BOTTOMS`].
    #[serde(default)]
    pub bottom: u8,
    #[serde(default = "crate::genetics::default_shoes")]
    pub shoes: [f32; 3],
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            skin: [0.87, 0.67, 0.5],
            hair: [0.35, 0.22, 0.1],
            shirt: [0.3, 0.55, 0.85],
            pants: [0.25, 0.25, 0.32],
            height: 1.0,
            build: 0.5,
            hair_style: 0,
            eyes: crate::genetics::default_eyes(),
            shape: Default::default(),
            facial_hair: 0,
            top: 0,
            bottom: 0,
            shoes: crate::genetics::default_shoes(),
        }
    }
}

impl Appearance {
    /// A shape slider's value (0 when unset).
    pub fn shape(&self, id: &str) -> f32 {
        self.shape.get(id).copied().unwrap_or(0.0)
    }
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
    /// Looping sound id for the current interaction (see `InteractionDef::sfx`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sfx: Option<String>,
}

impl Posture {
    pub fn performing(pose: Pose, anim: Option<String>, prop: Option<String>) -> Posture {
        Posture { pose, walking: false, anim, prop, sfx: None }
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
    /// Follow these points as given, without pathfinding (along the sidewalk, off the lot grid).
    Walk { path: Vec<Vec3> },
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
    /// Seat object and slot reserved to perform an object interaction from (see `SeatDef`); the
    /// object's own slot is then None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seat: Option<(Entity, usize)>,
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

/// Not on the active lot: lives on another lot of the neighborhood, or their household is out.
/// Simulated roughly (see `neighborhood::offlot_daily`). Drops by around `visit_at`
/// (`u64::MAX` = no plans to).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offlot {
    pub visit_at: u64,
}

/// On the active lot as a guest; lives elsewhere in the neighborhood.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Visitor {
    /// When the visit ends.
    pub leave_at: u64,
    /// Walking back to the street.
    pub leaving: bool,
}

/// Off the lot for any reason (work, school, or living elsewhere).
pub fn is_away(world: &hearth_ecs::World, e: hearth_ecs::Entity) -> bool {
    world.has::<AtWork>(e) || world.has::<Offlot>(e)
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
    /// Face and body shape (see [`Appearance::shape`]).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub shape: std::collections::BTreeMap<String, f32>,
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
