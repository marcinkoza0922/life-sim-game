//! Content definitions loaded from RON (`assets/data/*.ron`). The files are embedded at
//! compile time so tests and tools work from any directory; the game can reload from disk.

use std::collections::BTreeMap;
use std::path::Path;

use glam::Vec3;
use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------------------------------------
// Enums shared across the sim

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Need {
    Hunger,
    Energy,
    Bladder,
    Hygiene,
    Social,
    Fun,
    Comfort,
    Environment,
    // personality-granted ("silly") needs
    Drama,
    Validation,
    Chaos,
    Narration,
    Vigilance,
}

impl Need {
    pub const COUNT: usize = 13;
    pub const ALL: [Need; Need::COUNT] = [
        Need::Hunger,
        Need::Energy,
        Need::Bladder,
        Need::Hygiene,
        Need::Social,
        Need::Fun,
        Need::Comfort,
        Need::Environment,
        Need::Drama,
        Need::Validation,
        Need::Chaos,
        Need::Narration,
        Need::Vigilance,
    ];
    pub const CORE: [Need; 8] = [
        Need::Hunger,
        Need::Energy,
        Need::Bladder,
        Need::Hygiene,
        Need::Social,
        Need::Fun,
        Need::Comfort,
        Need::Environment,
    ];
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn is_core(self) -> bool {
        (self as usize) < 8
    }
    pub fn label(self) -> &'static str {
        match self {
            Need::Hunger => "Hunger",
            Need::Energy => "Energy",
            Need::Bladder => "Bladder",
            Need::Hygiene => "Hygiene",
            Need::Social => "Social",
            Need::Fun => "Fun",
            Need::Comfort => "Comfort",
            Need::Environment => "Environment",
            Need::Drama => "Drama",
            Need::Validation => "Validation",
            Need::Chaos => "Chaos",
            Need::Narration => "Narration",
            Need::Vigilance => "Vigilance",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Skill {
    Cooking,
    Charisma,
    Logic,
    Fitness,
    Creativity,
    Mischief,
    Handiness,
}

impl Skill {
    pub const COUNT: usize = 7;
    pub const ALL: [Skill; Skill::COUNT] = [
        Skill::Cooking,
        Skill::Charisma,
        Skill::Logic,
        Skill::Fitness,
        Skill::Creativity,
        Skill::Mischief,
        Skill::Handiness,
    ];
    pub fn idx(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LifeStage {
    Infant,
    Toddler,
    Child,
    Teen,
    YoungAdult,
    Adult,
    Elder,
}

impl LifeStage {
    pub const ALL: [LifeStage; 7] = [
        LifeStage::Infant,
        LifeStage::Toddler,
        LifeStage::Child,
        LifeStage::Teen,
        LifeStage::YoungAdult,
        LifeStage::Adult,
        LifeStage::Elder,
    ];
    /// Default duration in days at lifespan scale 1.0.
    pub fn base_days(self) -> f32 {
        match self {
            LifeStage::Infant => 3.0,
            LifeStage::Toddler => 4.0,
            LifeStage::Child => 8.0,
            LifeStage::Teen => 8.0,
            LifeStage::YoungAdult => 15.0,
            LifeStage::Adult => 20.0,
            LifeStage::Elder => 12.0,
        }
    }
    pub fn next(self) -> Option<LifeStage> {
        LifeStage::ALL.get(self as usize + 1).copied()
    }
    pub fn can_walk(self) -> bool {
        self != LifeStage::Infant
    }
    pub fn is_adult(self) -> bool {
        matches!(self, LifeStage::YoungAdult | LifeStage::Adult | LifeStage::Elder)
    }
    /// Body height in meters for placeholder rendering and nav.
    pub fn height(self) -> f32 {
        match self {
            LifeStage::Infant => 0.55,
            LifeStage::Toddler => 0.85,
            LifeStage::Child => 1.25,
            LifeStage::Teen => 1.6,
            LifeStage::YoungAdult | LifeStage::Adult => 1.75,
            LifeStage::Elder => 1.68,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            LifeStage::Infant => "Infant",
            LifeStage::Toddler => "Toddler",
            LifeStage::Child => "Child",
            LifeStage::Teen => "Teen",
            LifeStage::YoungAdult => "Young Adult",
            LifeStage::Adult => "Adult",
            LifeStage::Elder => "Elder",
        }
    }
}

/// Body pose while performing an interaction (drives placeholder animation).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pose {
    #[default]
    Stand,
    Sit,
    Lie,
    Use,
    Dance,
    Exercise,
}

// ------------------------------------------------------------------------------------------------
// Objects

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Category {
    Seating,
    Sleeping,
    Surfaces,
    Kitchen,
    Bathroom,
    Electronics,
    Decor,
    Lighting,
    Fun,
    Kids,
    Misc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    Box,
    Cylinder,
    Sphere,
}

fn default_rough() -> f32 {
    0.6
}

/// A primitive used to greybox an object until real art exists. Positions are in object-local
/// meters: x across the footprint width, z across its depth (front is +z), y up.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Part {
    pub shape: Shape,
    pub size: Vec3,
    pub at: Vec3,
    pub color: [f32; 3],
    #[serde(default = "default_rough")]
    pub rough: f32,
    #[serde(default)]
    pub metal: f32,
    #[serde(default)]
    pub emissive: f32,
    /// A [`MaterialDef`] id: textures the part, with `color` as a tint (white = as authored).
    /// Without textures loaded the part falls back to flat `color`/`rough`/`metal`.
    #[serde(default)]
    pub material: Option<String>,
}

/// A textured surface. Maps live in `assets/textures/<id>/` (`albedo.png`, optional
/// `normal.png` and `orm.png`) and are loaded by the game; the sim only checks names.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaterialDef {
    pub id: String,
    /// World meters covered by one repeat of the texture.
    #[serde(default = "default_material_scale")]
    pub scale: f32,
}

fn default_material_scale() -> f32 {
    1.0
}

/// Where a peon stands to start using an object, and where/how it is positioned while using it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlotDef {
    /// Local (x, z) of the approach point; the containing tile is the nav target.
    pub stand: (f32, f32),
    /// Local position of the peon's feet/hips during use.
    pub use_at: Vec3,
    /// Yaw in degrees during use (0 = facing +z / the object's front direction).
    #[serde(default)]
    pub facing: f32,
}

/// Which hand holds a prop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hand {
    #[default]
    Right,
    Left,
}

/// A small object a peon holds while performing (book, phone, spoon), greyboxed from parts.
/// Parts are in *grip space*: origin in the palm, +y along the hand towards the fingertips, +z
/// out of the back of the hand's forward side (the body's forward when the arm hangs), meters.
/// `rot` (degrees, applied x then y then z) turns the whole prop within the grip.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropDef {
    pub id: String,
    #[serde(default)]
    pub hand: Hand,
    #[serde(default)]
    pub rot: Vec3,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DurationDef {
    Minutes(u32),
    /// Until the need reaches ~full, capped at the given minutes.
    UntilFull(Need, u32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Outcome {
    Money { chance: f32, min: i64, max: i64 },
    Moodlet { chance: f32, id: String },
    Log { chance: f32, text: String },
    Dirty { amount: f32 },
    Break { chance: f32 },
    /// A mishap: with `chance` (scaled down by `skill`, doubled for the Clumsy) `text` is logged and
    /// `moodlet` applied; it is fatal (`cause` of death, Reaper permitting) with probability `fatal`.
    Accident {
        chance: f32,
        cause: String,
        text: String,
        #[serde(default)]
        moodlet: Option<String>,
        #[serde(default)]
        fatal: f32,
        #[serde(default)]
        skill: Option<Skill>,
    },
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InteractionDef {
    pub id: String,
    pub name: String,
    /// Need gain per game hour while performing the interaction.
    #[serde(default)]
    pub rates: BTreeMap<Need, f32>,
    /// What the interaction *claims* to offer (used for autonomy). Derived from `rates` if empty.
    #[serde(default)]
    pub ads: BTreeMap<Need, f32>,
    pub duration: DurationDef,
    #[serde(default)]
    pub pose: Pose,
    /// Allowed life stages; empty = Child and older.
    #[serde(default)]
    pub stages: Vec<LifeStage>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub skill: Option<(Skill, f32)>,
    #[serde(default)]
    pub requires_trait: Option<String>,
    #[serde(default)]
    pub outcomes: Vec<Outcome>,
    /// Icon/word shown in the speech/thought bubble while performing.
    #[serde(default)]
    pub bubble: Option<String>,
    #[serde(default = "yes")]
    pub autonomous: bool,
    /// Money charged when started.
    #[serde(default)]
    pub cost: i64,
    /// Restrict to these slot indices (default: any free slot).
    #[serde(default)]
    pub slots: Vec<usize>,
    /// Allowed even when the object is broken (e.g. Repair).
    #[serde(default)]
    pub when_broken: bool,
    /// Only offered when the object is broken.
    #[serde(default)]
    pub only_broken: bool,
    /// Only offered when the object's dirt is above this.
    #[serde(default)]
    pub min_dirt: f32,
    /// Removes dirt / repairs on completion.
    #[serde(default)]
    pub cleans: bool,
    #[serde(default)]
    pub repairs: bool,
    /// Body clip while performing (a clip name in the peon models). Falls back to the clip for
    /// `pose` when absent or when a body (e.g. an infant's) lacks it.
    #[serde(default)]
    pub anim: Option<String>,
    /// Prop held while performing (an id in `props.ron`).
    #[serde(default)]
    pub prop: Option<String>,
    /// Looping sound while performing (an id in `assets/audio/sfx/`). When absent the game picks
    /// one from the body clip (chop, stir, type, TV…); `""` means silence.
    #[serde(default)]
    pub sfx: Option<String>,
    /// Perform from a free seat nearby when there is one (watching TV from the sofa, chess from a
    /// chair pulled up to the table); otherwise from the object's own slots as usual.
    #[serde(default)]
    pub seat: Option<SeatDef>,
}

/// Where an interaction may be performed from a seat: any free slot of a `Sit` interaction tagged
/// "seating" on another object, on the same level, within `range` meters (seat to the object's
/// centre, horizontally) and facing the object to within `cone` degrees.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeatDef {
    pub range: f32,
    #[serde(default = "default_seat_cone")]
    pub cone: f32,
    /// Body clip while seated (replaces `anim`); `prop` is still held.
    #[serde(default)]
    pub anim: Option<String>,
}

fn default_seat_cone() -> f32 {
    40.0
}

impl InteractionDef {
    pub fn allowed_stage(&self, s: LifeStage) -> bool {
        if self.stages.is_empty() { s >= LifeStage::Child } else { self.stages.contains(&s) }
    }
    pub fn nominal_minutes(&self) -> u32 {
        match self.duration {
            DurationDef::Minutes(m) => m,
            DurationDef::UntilFull(_, m) => m / 2,
        }
    }
    /// Advertised gain for `need` (explicit ad, else rate × nominal duration, capped at 100).
    pub fn ad(&self, need: Need) -> f32 {
        if !self.ads.is_empty() {
            return self.ads.get(&need).copied().unwrap_or(0.0);
        }
        let r = self.rates.get(&need).copied().unwrap_or(0.0);
        (r * self.nominal_minutes() as f32 / 60.0).clamp(-100.0, 100.0)
    }
    pub fn has_tag(&self, t: &str) -> bool {
        self.tags.iter().any(|x| x == t)
    }
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub desc: String,
    pub category: Category,
    pub price: i64,
    pub footprint: (u8, u8),
    pub parts: Vec<Part>,
    #[serde(default)]
    pub slots: Vec<SlotDef>,
    #[serde(default)]
    pub interactions: Vec<InteractionDef>,
    #[serde(default)]
    pub beauty: f32,
    /// Dirt added per completed use.
    #[serde(default)]
    pub dirt_per_use: f32,
    /// Chance per completed use of breaking.
    #[serde(default)]
    pub break_chance: f32,
    /// Peons can walk through its tiles (rugs, etc.).
    #[serde(default)]
    pub walkable: bool,
    /// Emits light (for rendering).
    #[serde(default)]
    pub light: Option<(f32, [f32; 3])>,
    /// Comfort multiplier hint for UI.
    #[serde(default = "one")]
    pub quality: f32,
    /// Used up after this many completed uses (a cake), then removed.
    #[serde(default)]
    pub servings: Option<u32>,
}

// ------------------------------------------------------------------------------------------------
// Personality

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraitDef {
    pub id: String,
    pub name: String,
    pub desc: String,
    /// Need decay multipliers.
    #[serde(default)]
    pub decay: BTreeMap<Need, f32>,
    /// Score multipliers for interactions carrying a tag.
    #[serde(default)]
    pub tags: BTreeMap<String, f32>,
    /// A personality need this trait adds.
    #[serde(default)]
    pub grants: Option<Need>,
    #[serde(default)]
    pub conflicts: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuirkDef {
    pub id: String,
    pub name: String,
    pub desc: String,
    #[serde(default)]
    pub tags: BTreeMap<String, f32>,
    /// Things the peon randomly mutters about (bubble icons/words).
    #[serde(default)]
    pub mutters: Vec<String>,
}

// ------------------------------------------------------------------------------------------------
// Social

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SocialDef {
    pub id: String,
    pub name: String,
    pub minutes: u32,
    /// Need rates (per hour) for the initiator.
    #[serde(default)]
    pub rates: BTreeMap<Need, f32>,
    /// Need rates (per hour) for the target.
    #[serde(default)]
    pub target_rates: BTreeMap<Need, f32>,
    #[serde(default)]
    pub friendship: f32,
    #[serde(default)]
    pub romance: f32,
    #[serde(default = "default_min_friendship")]
    pub min_friendship: f32,
    #[serde(default)]
    pub min_romance: f32,
    #[serde(default = "default_accept")]
    pub accept: f32,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Initiator stages (empty = Child+).
    #[serde(default)]
    pub stages: Vec<LifeStage>,
    /// Target stages (empty = Toddler+).
    #[serde(default)]
    pub target_stages: Vec<LifeStage>,
    /// Caretaking interaction: scored by the target's urgency rather than the initiator's.
    #[serde(default)]
    pub care: bool,
    #[serde(default)]
    pub bubble: Option<String>,
    #[serde(default = "yes")]
    pub autonomous: bool,
    #[serde(default)]
    pub requires_trait: Option<String>,
    /// Requires both to be unrelated adults (romance).
    #[serde(default)]
    pub romantic: bool,
    /// Body clips for the initiator and the target while it plays (see [`InteractionDef::anim`]).
    #[serde(default)]
    pub anim: Option<String>,
    #[serde(default)]
    pub target_anim: Option<String>,
    /// Prop the initiator holds.
    #[serde(default)]
    pub prop: Option<String>,
    /// Only between a woman and a man (engagements and weddings, for now).
    #[serde(default)]
    pub opposite_sex: bool,
    /// Where the pair must stand with each other (see [`RelReq`]).
    #[serde(default)]
    pub requires: Option<RelReq>,
    /// The target can't say no (breaking up takes one).
    #[serde(default)]
    pub certain: bool,
}

/// A relationship a social needs; when it isn't met the social isn't offered at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelReq {
    /// Neither is spoken for (by each other or anyone else alive).
    Unattached,
    /// Engaged to each other, no wedding planned yet.
    Engaged,
    /// Engaged to each other (the wedding itself).
    Betrothed,
    /// Partners, engaged or married to each other.
    Together,
    /// They live in different households.
    LiveApart,
}

fn default_min_friendship() -> f32 {
    -100.0
}
fn default_accept() -> f32 {
    0.8
}

impl SocialDef {
    pub fn initiator_ok(&self, s: LifeStage) -> bool {
        if self.stages.is_empty() { s >= LifeStage::Child } else { self.stages.contains(&s) }
    }
    pub fn target_ok(&self, s: LifeStage) -> bool {
        if self.target_stages.is_empty() { s >= LifeStage::Toddler } else { self.target_stages.contains(&s) }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MoodletDef {
    pub id: String,
    pub name: String,
    pub value: f32,
    pub hours: f32,
    #[serde(default)]
    pub desc: String,
}

// ------------------------------------------------------------------------------------------------
// Careers

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CareerLevel {
    pub title: String,
    pub pay: i64,
    pub start_hour: u32,
    pub hours: u32,
    /// Work days, 0 = day 1 of the week (7-day week).
    pub days: Vec<u8>,
    #[serde(default)]
    pub skills: BTreeMap<Skill, f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CareerEffect {
    Performance(f32),
    Money(i64),
    Moodlet(String),
    Skill(Skill, f32),
    Log(String),
    /// Fired (or arrested) on the spot.
    Fired,
    /// Dies at work with this probability, of the given cause (the Reaper may still be talked round).
    Accident(f32, String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CareerChoice {
    pub label: String,
    #[serde(default)]
    pub check: Option<(Skill, f32)>,
    pub success: Vec<CareerEffect>,
    #[serde(default)]
    pub failure: Vec<CareerEffect>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CareerEvent {
    pub text: String,
    /// Minimum level index for this event (0-based).
    #[serde(default)]
    pub min_level: usize,
    pub choices: Vec<CareerChoice>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CareerDef {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub levels: Vec<CareerLevel>,
    pub events: Vec<CareerEvent>,
    /// Randomly redacts words in event text (Spy track).
    #[serde(default)]
    pub redact: bool,
    /// Minimum life stage (Teen tracks are part-time).
    #[serde(default = "default_career_stage")]
    pub min_stage: LifeStage,
}

fn default_career_stage() -> LifeStage {
    LifeStage::YoungAdult
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NameLists {
    pub first: Vec<String>,
    pub last: Vec<String>,
    /// Syllables for gibberish speech.
    pub syllables: Vec<String>,
}

// ------------------------------------------------------------------------------------------------
// Registry

/// All loaded content. Stored as a resource.
#[derive(Clone, Debug, Default)]
pub struct GameData {
    pub objects: Vec<ObjectDef>,
    pub traits: Vec<TraitDef>,
    pub quirks: Vec<QuirkDef>,
    pub socials: Vec<SocialDef>,
    pub moodlets: Vec<MoodletDef>,
    pub careers: Vec<CareerDef>,
    pub names: NameLists,
    pub props: Vec<PropDef>,
    pub materials: Vec<MaterialDef>,
}

pub type ObjectId = usize;

macro_rules! embedded {
    ($f:literal) => {
        ($f, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/", $f)))
    };
}

const EMBEDDED: [(&str, &str); 9] = [
    embedded!("materials.ron"),
    embedded!("objects.ron"),
    embedded!("traits.ron"),
    embedded!("quirks.ron"),
    embedded!("socials.ron"),
    embedded!("moodlets.ron"),
    embedded!("careers.ron"),
    embedded!("names.ron"),
    embedded!("props.ron"),
];

fn parse<T: for<'de> Deserialize<'de>>(name: &str, text: &str) -> Result<T, String> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(text)
        .map_err(|e| format!("{name}: {e}"))
}

impl GameData {
    /// The content compiled into the binary.
    pub fn embedded() -> GameData {
        Self::from_sources(|name| {
            EMBEDDED.iter().find(|(n, _)| *n == name).map(|(_, t)| t.to_string()).ok_or_else(|| name.to_string())
        })
        .expect("embedded game data must parse")
    }

    /// Load from a directory (for hot-reload / modding).
    pub fn load_dir(dir: &Path) -> Result<GameData, String> {
        Self::from_sources(|name| std::fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}")))
    }

    fn from_sources(mut get: impl FnMut(&str) -> Result<String, String>) -> Result<GameData, String> {
        let data = GameData {
            objects: parse("objects.ron", &get("objects.ron")?)?,
            traits: parse("traits.ron", &get("traits.ron")?)?,
            quirks: parse("quirks.ron", &get("quirks.ron")?)?,
            socials: parse("socials.ron", &get("socials.ron")?)?,
            moodlets: parse("moodlets.ron", &get("moodlets.ron")?)?,
            careers: parse("careers.ron", &get("careers.ron")?)?,
            names: parse("names.ron", &get("names.ron")?)?,
            props: parse("props.ron", &get("props.ron")?)?,
            // optional so older data directories still load
            materials: match get("materials.ron") {
                Ok(text) => parse("materials.ron", &text)?,
                Err(_) => Vec::new(),
            },
        };
        data.validate()?;
        Ok(data)
    }

    /// Cross-reference checks so content mistakes fail loudly at load time.
    pub fn validate(&self) -> Result<(), String> {
        let mut errs = Vec::new();
        let moodlet = |id: &str| self.moodlets.iter().any(|m| m.id == id);
        let has_trait = |id: &str| self.traits.iter().any(|t| t.id == id);
        let has_prop = |id: &Option<String>| id.as_ref().is_none_or(|id| self.props.iter().any(|p| &p.id == id));
        let has_material = |id: &Option<String>| id.as_ref().is_none_or(|id| self.materials.iter().any(|m| &m.id == id));
        for (owner, parts) in self.objects.iter().map(|o| (&o.id, &o.parts)).chain(self.props.iter().map(|p| (&p.id, &p.parts))) {
            for p in parts.iter().filter(|p| !has_material(&p.material)) {
                errs.push(format!("{owner}: unknown material {}", p.material.as_deref().unwrap_or_default()));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for o in &self.objects {
            if !seen.insert(&o.id) {
                errs.push(format!("duplicate object id {}", o.id));
            }
            for i in &o.interactions {
                for out in &i.outcomes {
                    let id = match out {
                        Outcome::Moodlet { id, .. } => Some(id),
                        Outcome::Accident { moodlet, .. } => moodlet.as_ref(),
                        _ => None,
                    };
                    if let Some(id) = id
                        && !moodlet(id) {
                            errs.push(format!("{}.{}: unknown moodlet {id}", o.id, i.id));
                        }
                }
                if let Some(t) = &i.requires_trait
                    && !has_trait(t) {
                        errs.push(format!("{}.{}: unknown trait {t}", o.id, i.id));
                    }
                if !has_prop(&i.prop) {
                    errs.push(format!("{}.{}: unknown prop {}", o.id, i.id, i.prop.as_deref().unwrap_or_default()));
                }
                for &s in &i.slots {
                    if s >= o.slots.len() {
                        errs.push(format!("{}.{}: slot {s} out of range", o.id, i.id));
                    }
                }
                if o.slots.is_empty() {
                    errs.push(format!("{}: has interactions but no slots", o.id));
                }
            }
            if o.break_chance > 0.0 && !o.interactions.iter().any(|i| i.repairs) {
                errs.push(format!("{}: can break but has no repair interaction", o.id));
            }
        }
        for t in &self.traits {
            for c in &t.conflicts {
                if !has_trait(c) {
                    errs.push(format!("trait {}: unknown conflict {c}", t.id));
                }
            }
        }
        for s in &self.socials {
            if let Some(t) = &s.requires_trait
                && !has_trait(t) {
                    errs.push(format!("social {}: unknown trait {t}", s.id));
                }
            if !has_prop(&s.prop) {
                errs.push(format!("social {}: unknown prop {}", s.id, s.prop.as_deref().unwrap_or_default()));
            }
        }
        for c in &self.careers {
            let check = |effs: &Vec<CareerEffect>, errs: &mut Vec<String>| {
                for e in effs {
                    if let CareerEffect::Moodlet(m) = e
                        && !moodlet(m) {
                            errs.push(format!("career {}: unknown moodlet {m}", c.id));
                        }
                }
            };
            for ev in &c.events {
                for ch in &ev.choices {
                    check(&ch.success, &mut errs);
                    check(&ch.failure, &mut errs);
                }
            }
            if c.levels.is_empty() {
                errs.push(format!("career {}: no levels", c.id));
            }
        }
        if errs.is_empty() { Ok(()) } else { Err(errs.join("\n")) }
    }

    pub fn object_id(&self, id: &str) -> Option<ObjectId> {
        self.objects.iter().position(|o| o.id == id)
    }
    pub fn object(&self, id: ObjectId) -> &ObjectDef {
        &self.objects[id]
    }
    pub fn trait_def(&self, id: &str) -> Option<&TraitDef> {
        self.traits.iter().find(|t| t.id == id)
    }
    pub fn quirk(&self, id: &str) -> Option<&QuirkDef> {
        self.quirks.iter().find(|t| t.id == id)
    }
    pub fn moodlet(&self, id: &str) -> Option<&MoodletDef> {
        self.moodlets.iter().find(|t| t.id == id)
    }
    pub fn career(&self, id: &str) -> Option<&CareerDef> {
        self.careers.iter().find(|t| t.id == id)
    }
    pub fn social(&self, id: &str) -> Option<&SocialDef> {
        self.socials.iter().find(|t| t.id == id)
    }
    pub fn prop(&self, id: &str) -> Option<&PropDef> {
        self.props.iter().find(|t| t.id == id)
    }
    /// Every body clip named by an interaction or social (for checking against the models).
    pub fn anims(&self) -> std::collections::BTreeSet<&str> {
        let objs = self.objects.iter().flat_map(|o| {
            o.interactions.iter().flat_map(|i| [i.anim.as_deref(), i.seat.as_ref().and_then(|s| s.anim.as_deref())].into_iter().flatten())
        });
        let socials = self.socials.iter().flat_map(|s| [s.anim.as_deref(), s.target_anim.as_deref()].into_iter().flatten());
        objs.chain(socials).collect()
    }
    /// The body clips a peon at `stage` can be asked to play: interactions it may use (and their
    /// seated variants), socials it may start, and socials' target clips when it may be the target.
    pub fn anims_for(&self, stage: LifeStage) -> std::collections::BTreeSet<&str> {
        let objs = self.objects.iter().flat_map(|o| o.interactions.iter()).filter(|i| i.allowed_stage(stage)).flat_map(|i| {
            [i.anim.as_deref(), i.seat.as_ref().and_then(|s| s.anim.as_deref())].into_iter().flatten()
        });
        let socials = self.socials.iter().flat_map(|s| {
            [s.anim.as_deref().filter(|_| s.initiator_ok(stage)), s.target_anim.as_deref().filter(|_| s.target_ok(stage))].into_iter().flatten()
        });
        objs.chain(socials).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_data_loads_and_validates() {
        let d = GameData::embedded();
        assert!(d.objects.len() >= 15);
        assert!(d.careers.len() >= 10);
        assert!(d.traits.len() >= 12);
    }

    /// Dirt with no way to clean it piles up until the room's Environment is stuck at zero.
    #[test]
    fn every_object_that_gets_dirty_can_be_cleaned() {
        let d = GameData::embedded();
        let stuck: Vec<&str> = d
            .objects
            .iter()
            .filter(|o| o.dirt_per_use > 0.0 && !o.interactions.iter().any(|i| i.cleans))
            .map(|o| o.id.as_str())
            .collect();
        assert!(stuck.is_empty(), "objects that get dirty but have no cleaning interaction: {stuck:?}");
    }
}
