# Life-Sim Game — Design Document

Working title: **life-sim-game** (engine codename: **Hearth**)

A lightly comedic 3D life simulator / virtual dollhouse. The player builds homes,
creates households of *peons*, and guides (or neglects) them from infancy to death.
Written in Rust on a custom, fit-to-purpose ECS.

---

## 1. Pillars

1. **Autonomous but steerable.** Peons live on their own; the player nudges, queues and overrides.
2. **Readable personalities.** You should be able to tell a Neat Theatrical peon from a Lazy Paranoid one by watching for a minute.
3. **Comedy through systems, not scripts.** Jokes emerge from traits, quirks, career events and accidents. Silly needs are seasoning, not the meal.
4. **Building is play.** Multi-story homes with basements, roofs and terrain, on a forgiving tile grid.
5. **Everything is data.** Objects, interactions, traits, careers and events live in RON files so content can grow without engine changes.

## 2. Decisions (from interview, 2026-11-08)

| Area | Decision |
|---|---|
| Engine | Custom archetype ECS + wgpu 30 + winit 0.30; crates for math (glam), glTF, audio (kira), UI (egui 0.36) |
| Platform | Linux first (Vulkan). Cross-platform later — avoid platform-specific code |
| Visual style | Semi-realistic PBR; greybox placeholders first, glTF art later |
| Camera | Free orbit/zoom/pan, wall cutaway, upper-floor hiding |
| MVP | One household, one lot; build/buy, needs, autonomy, aging, a few careers |
| Characters | Rigged base mesh per life stage + morph targets + swappable hair/clothing |
| Time | 1 real s = 1 game min at speed 1; speeds 1/2/3 + pause; lifespan ~60–90 days (configurable) |
| Life stages | Infant → Toddler → Child → Teen → Young Adult → Adult → Elder |
| Needs | Core 8 + trait-granted needs (silly ones come only from personality, low weight) |
| Personality | 5 axes (temperament) + 3–5 traits + 1 quirk |
| Autonomy | Utility AI with object advertisements |
| Control | Player action queue + free-will level (Off/Low/Full); peons may grumble/refuse |
| Build grid | 1 m tiles, walls on tile edges (+diagonals), levels −2..+4 |
| Build tools | Walls/floors/doors/windows, stairs, multi-story, basements, roofs, terrain |
| Furniture | Data-driven interaction sets with slots, state (dirty/broken/upgraded) |
| Economy | Funds, costs, bills, depreciation |
| Careers | Rabbit-hole + one comedic choice event per shift |
| Social | Friendship + romance axes, marriage, pregnancy/adoption, genetics |
| Death | Darkly comedic: old age + silly accidents, bargainable reaper, ghosts |
| Speech | Gibberish voice barks + icon bubbles |

## 3. Architecture

```
crates/
  hearth_ecs     Custom archetype ECS: World, entities, components, queries,
                 resources, events, commands, schedule. No game knowledge.
  hearth_render  wgpu renderer: meshes, PBR-lite shading, shadows, instancing,
                 offscreen capture (screenshots). No game knowledge.
  sim            The game simulation. Pure logic on hearth_ecs — no rendering,
                 no windowing, fully deterministic given a seed. Testable headless.
  game           The playable binary (`lifesim`): window, input, camera, build
                 tools, egui UI, sim→render bridging, debug overlays.
  simctl         Headless CLI: run the sim for N days, run scenario files,
                 dump event logs / stats as JSON, render screenshots offscreen.
assets/
  data/          RON content: objects, traits, quirks, careers, needs, names
  models/        peon_<stage>.glb rigged bodies (generated; embedded in the game binary)
  blender/       make_peons.py: builds the bodies, rig, hair and clips headlessly
docs/
```

Dependency direction: `game → {sim, hearth_render, hearth_ecs}`, `simctl → {sim, hearth_render}`,
`sim → hearth_ecs`. The sim never depends on the renderer. This keeps the simulation
deterministic, fast to test, and runnable at thousands of ticks per second.

### 3.1 hearth_ecs

**Storage: archetype tables.** Entities with the identical component set share a table;
each component is a type-erased column (`BlobVec`). Iteration over a query visits only
matching tables and walks dense columns — ideal for per-tick systems (need decay,
autonomy scoring, movement) that touch every peon.

- `Entity { index: u32, generation: u32 }` — generational handles, stale handles detected.
- `EntityMeta` maps entity → (archetype id, row).
- Archetype graph caches add/remove transitions (`edges`) so structural changes are O(columns).
- `Component` is any `'static + Send + Sync` type (blanket impl).
- **Queries:** `world.query::<(&Pos, &mut Vel, Option<&Name>)>()` with filters
  `With<T>` / `Without<T>`. Aliasing is validated at query build (same type mutably twice → panic).
  `Entity` itself is fetchable.
- **Resources:** singleton typed values (`world.resource::<Clock>()`).
- **Events:** double-buffered `Events<T>` resources; readers keep a cursor.
- **Commands:** deferred spawn/despawn/insert/remove, applied at stage boundaries,
  so systems can iterate and mutate structure safely.
- **Schedule:** ordered stages of `fn(&mut World)` systems. Single-threaded by design for
  now — sim is cheap at household scale and determinism matters more than parallelism.
  The access model leaves room for a parallel executor later.
- **Change ticks (planned):** per-row added/changed ticks for `Changed<T>` filters. Not needed yet —
  the renderer rebuilds its instance list each frame, which is cheap at household scale.
- **Runtime borrow checks:** queries borrow `&World` and take per-component-type shared/unique
  borrow flags, so a system can hold several non-conflicting queries and resources at once.

Not planned: relations, observers, reflection. (A small hand-written inspector trait
in the game crate covers debug UI needs.)

### 3.2 hearth_render

- wgpu 30 on Vulkan. HDR-ish linear pipeline into an sRGB surface.
- **Shading:** PBR-lite (GGX specular, Lambert diffuse, metallic/roughness),
  one directional sun with a shadow map, hemispherical ambient. Real IBL later.
- **Meshes:** CPU `MeshData` (pos/normal/uv/color) → GPU buffers; handle-based `MeshId`.
  Per-instance data (model matrix, base color, roughness/metallic, flags) in an instance buffer;
  draws are batched by mesh.
- **Placeholders:** procedural primitives (box, cylinder, capsule, plane, wedge) used for
  furniture greyboxes until glTF art lands (and for greybox peons if the models fail to load).
- **Skinned meshes:** `hearth_render::model` loads binary glTF (skeleton, skinned meshes, up to two
  position morph targets, T/R/S animation clips). Clips are sampled on the CPU; skinning matrices
  for every pose go into one storage buffer per frame, alongside an 8-entry colour palette per draw
  that vertex material slots index (skin, hair, shirt, pants, shoes). Skinned draws are instanced
  by mesh and cast shadows like static ones.
- **Capture:** render any frame to an offscreen texture and read it back to PNG. Used by
  `simctl render`, by `--screenshot` in the game, and by visual regression checks.
- Dollhouse specifics (in `game`, using instance flags): wall cutaway (walls facing the
  camera rendered as short stubs), hide levels above the active floor, ghost preview for
  build tools.

### 3.3 sim

Modules:

| Module | Responsibility |
|---|---|
| `time` | `Clock` (ticks, minutes, day, speed), fixed-step driver. 1 tick = 1 game minute |
| `data` | RON loading → registries (`ObjectDefs`, `TraitDefs`, `CareerDefs`, …) |
| `lot` | Tile grid per level, walls (edge-based), floors, doors/windows, stairs, terrain heights, room detection |
| `nav` | A* on the tile grid across levels; walls block edges, doors open them, stairs link levels |
| `peon` | Peon components: identity, life stage, age, needs, personality, mood, skills, inventory |
| `needs` | Need decay (stage × trait modifiers), critical thresholds, moodlets |
| `objects` | Object instances, slots, interaction defs, state (dirt, broken, upgrade level) |
| `autonomy` | Advertisement scoring, action selection, free-will level |
| `actions` | Action queue, action state machine (walk → use → finish), player orders |
| `social` | Relationships (friendship, romance), social interactions, family tree |
| `life` | Aging, stage transitions, pregnancy, genetics, death & ghosts |
| `career` | Jobs, schedules, rabbit-hole shifts, events with choices, promotions, pay |
| `economy` | Funds, purchases, bills, depreciation |
| `events` | Narrative log (`SimLog`) of everything notable — drives UI notifications and test assertions |

**Determinism:** all randomness goes through a seeded `SimRng` resource (PCG). Given the
same seed, initial state and player commands, the sim produces identical results. Tests and
`simctl` rely on this.

## 4. Simulation Design

### 4.1 Time

- 1 tick = 1 game minute; 1440 ticks/day. Speeds: 1× (1 tick/s), 2× (3 ticks/s), 3× (10 ticks/s),
  plus an uncapped "ultra" speed used when everyone is asleep or at work (and by simctl).
- Movement is interpolated in the renderer between ticks for smoothness.

### 4.2 Life stages & aging

| Stage | Default days | Notes |
|---|---|---|
| Infant | 3 | Cannot walk; needs carried out by caretakers (bottle, crib, changing) |
| Toddler | 4 | Walks badly, learns to talk/potty |
| Child | 8 | School (rabbit-hole), homework |
| Teen | 8 | School, part-time jobs, drama |
| Young Adult | 15 | Careers open |
| Adult | 20 | |
| Elder | 12 ± random | Old-age death window opens at end |

Total ≈ 70 days at default. A global `lifespan_scale` multiplies all durations
(Short 0.5 / Normal 1.0 / Long 2.0 / custom). Birthdays can be triggered early with a cake.

### 4.3 Needs

Each need is 0–100 (100 = satisfied). Base decay per game hour, multiplied by life-stage and
trait modifiers. Below 30 = "low", below 10 = "critical" (strong autonomy weight, moodlets).
At 0, consequences: Bladder → accident, Energy → pass out, Hunger → (eventually) death.

**Core needs:** Hunger, Energy, Bladder, Hygiene, Social, Fun, Comfort, Environment.

**Trait-granted needs** (exist only on peons with the trait; low weight by design):

| Need | Granted by | Satisfied by |
|---|---|---|
| Drama | Theatrical | Arguments, gossip, dramatic monologue at mirror |
| Validation | Vain | Compliments, mirror admiring, posting selfies |
| Chaos | Gremlin | Kicking over trash, unplugging things, pranks |
| Narration | Bookish | Narrating own life aloud, journaling |
| Vigilance | Paranoid | Checking locks, peering out windows |

Silly needs decay slowly, are capped in autonomy weight (`weight ≤ 0.35` of core needs),
and never cause death — only mild moodlets.

**Mood** = weighted average of needs + sum of active moodlets (timed buffs/debuffs such as
"Ate Something Burnt", "Saw a Ghost", "Promoted!"). Mood affects career performance,
willingness to obey, and social success.

### 4.4 Personality

- **Axes (0–10, sum-capped 25):** Neat, Outgoing, Active, Playful, Nice. These bias
  advertisement scores and need decay (an Active peon's Fun decays faster when sitting).
- **Traits (3–5, from data):** e.g. Neat, Slob, Lazy, Romantic, Paranoid, Theatrical, Vain,
  Gremlin, Bookish, Foodie, Hothead, Cheerful, Gloomy, Night Owl, Bookworm, Clumsy.
  Each trait: need modifiers, interaction score multipliers (by tag), granted needs, unlocked
  interactions, conflicting traits.
- **Quirk (exactly 1):** small comedic behavior hook, e.g. *Afraid of Chairs* (Comfort from chairs
  negated, stands at dinner), *Talks to Plants*, *Hums Constantly*, *Collects Spoons*,
  *Suspicious of Birds*, *Narrates Weather*.

### 4.5 Objects & interactions

Each object definition (RON):

```ron
ObjectDef(
    id: "sofa_basic",
    name: "Slouchmaster 3000 Sofa",
    category: Seating,
    price: 450,
    footprint: (2, 1),
    mesh: Placeholder(Box(size: (2.0, 0.8, 0.9), color: (0.45, 0.30, 0.25))),
    slots: [ Slot(name: "seat0", offset: (-0.5, 0.0, 0.0), facing: 0.0), ... ],
    interactions: [
        Interaction(
            id: "sit", name: "Sit", duration: Minutes(30), repeatable: true,
            ads: { Comfort: 25 }, tags: ["seating", "rest"],
        ),
        Interaction(
            id: "nap", name: "Nap", duration: UntilNeed(Energy, 70),
            ads: { Energy: 15, Comfort: 10 }, tags: ["rest"],
        ),
        Interaction(
            id: "search_cushions", name: "Search Cushions for Change",
            duration: Minutes(15), ads: { Fun: 5 }, tags: ["mischief"],
            outcomes: [ Chance(0.3, Money(5..40)), Chance(0.02, Moodlet("found_remote")) ],
        ),
    ],
    states: (dirt_rate: 0.01, break_chance: 0.0),
)
```

Interactions specify: required life stages, required/forbidden traits, slot usage,
the need advertisement (what it *claims*) vs. actual effect per minute (may differ:
"Watch Infomercial" advertises Fun 20, delivers Fun 8), skills trained, outcomes,
and an animation key.

### 4.6 Autonomy (utility AI)

Every *think* (when idle, or every 30 min during low-priority actions):

1. Gather candidate (object, interaction) pairs available to this peon (stage, traits, reachable).
2. Score each: `Σ_need  ad[need] × urgency(need_value) × trait_mult × axis_bias`
   where `urgency` is a steep curve (an ad for a nearly-full need is worth little).
3. Divide by `1 + distance/attenuation`; penalize objects in use, dirty or broken.
4. Pick weighted-random from the top 3 (more deterministic when needs are critical).

**Free will levels:** *Off* — peon only acts on orders (critical needs still trigger grumbling);
*Low* — acts autonomously only for low/critical needs; *Full* — always.

**Player orders** go to the front of the queue. A peon may *refuse* if a need is critical
and the order doesn't help it, or if it clashes with a trait (Lazy + "Work Out") — refusal
chance is mood-dependent, shows a grumble bubble, and is logged.

### 4.7 Social

Per ordered pair: `friendship` (−100..100) and `romance` (0..100). Labels derived from values
(Acquaintance, Friend, Best Friend, Enemy, Crush, Partner, Spouse). Interactions (Chat, Joke,
Insult, Flirt, Hug, Propose…) have acceptance chances from mood, traits and relationship.
Family links: parents, children, spouse. Pregnancy (romantic YA/Adult pairs) or adoption.

**Genetics:** appearance is a gene vector (morph weights, skin/hair/eye colors). Child =
per-gene random parent pick + small mutation. 1–2 traits may be inherited.

### 4.8 Careers

Rabbit-hole careers with schedules (work days, hours) and 10 levels each. During each shift,
one event fires with 2–3 choices; choices check skills/traits and have comedic outcomes
(performance, money, moodlets, relationships, rarely an injury). Promotion requires performance
plus level-specific skill thresholds.

Tracks: Culinary, Medical, Business, Science, **Supervillain**, **Mime/Clown**, **Cryptid Hunter**,
**Influencer**, **Spy** (event text is partially `[REDACTED]`), **Criminal** (pickpocket → mafia boss,
small chance of getting caught).

Skills: Cooking, Charisma, Logic, Fitness, Creativity, Mischief, Handiness.

### 4.9 Economy

Starting funds by household size. Purchases (objects, build), selling at depreciated value,
bills every 3 days (scaled by lot value). Unpaid bills → warnings → repo-man visit (comedic).

### 4.10 Death

- **Old age:** probability per day after Elder threshold.
- **Accidents:** starvation, electrocution (Handiness repair failure), laughing too hard
  (Fun overflow event while Cheerful), vending-machine incident, hubris (Supervillain failure).
- **The Reaper** appears; a household member may plead (Charisma check) or challenge to
  rock-paper-scissors for a reprieve.
- Ghosts leave an urn/tombstone object and may haunt the lot at night (mostly harmless pranks).

## 5. Building

- Lot: W×D tiles (default 32×32), levels −2..+4, level height 3 m.
- **Walls** on tile edges (`EdgeKey(level, x, z, dir)`), plus diagonals per tile. Walls carry
  wall coverings per side. Doors/windows are wall-attached objects occupying an edge.
- **Floors** per tile per level; upper floors require support (walls below or a foundation).
- **Basements:** levels < 0 are dug out — no terrain there; the tile volume becomes
  walls+floor automatically.
- **Stairs:** occupy 1×N tiles, link level L to L+1 in navigation, cut a hole in the floor above.
- **Rooms:** flood fill over tiles bounded by walls per level → `RoomId`. Room score
  (beauty, size, dirt) feeds the Environment need.
- **Roofs:** auto-roof over room outlines (gable/hip/flat) + manual roof pieces.
- **Terrain:** vertex heightmap with raise/lower/flatten brush; building flattens under footprints.
- Every build action is an undoable command; costs charged on commit.

## 6. Presentation

- **Camera:** orbit target on the active level; RMB rotate, MMB/WASD pan, wheel zoom, PageUp/Down
  change level. Walls modes: Up / Cutaway / Down.
- **UI (egui, themed later):** needs panel, peon portraits, action queue, mood & moodlets,
  career panel, build/buy catalog, clock/speed controls, notification feed.
- **Speech:** gibberish syllable generator + pitch by age/sex; bubbles show icons of topics.

## 7. Debug & Test Tooling

Built in from day one:

- `simctl run --days N --seed S` — headless fast-forward, prints a daily summary and writes
  a JSON event log. Catches starvation loops, stuck actions, broken economy.
- `simctl scenario <file.ron>` — set up a lot + household, run, assert conditions
  (e.g. "no peon dies of hunger in 10 days", "toddler reaches child stage").
- `simctl render --out shot.png` — renders a lot offscreen to PNG (visual checks without a window).
- In-game: `F1` debug inspector (entity list, components, needs graphs), `F2` autonomy scores
  for the selected peon (why did they do that?), `F3` nav/room overlay, `F12` screenshot,
  time controls up to "ultra".
- `--screenshot-after <frames> <path>` CLI flag for automated window captures.
- Unit tests for the ECS and every sim system; deterministic property tests via seeds.

## 8. Roadmap

| Milestone | Content |
|---|---|
| **M0 Foundation** | Workspace, ECS core + tests, renderer (meshes, lighting, capture), window+camera, egui, simctl |
| **M1 Living box** | Lot grid, walls/floors, a few objects, peons with needs, autonomy, nav, actions, time controls |
| **M2 Life** | Aging & stages, traits/quirks, moodlets, social & relationships, speech bubbles |
| **M3 Build/Buy** | Build tools (walls, doors, windows, floors, stairs, levels, basements), catalog, economy |
| **M4 Careers** | Career tracks + events, skills, school |
| **M5 Family & Death** | Pregnancy, genetics, death, reaper, ghosts |
| **M6 Creator** | Character creator with morphs (glTF base meshes), clothing/hair |
| **M7 Art pass** | Real assets, shadows/IBL polish, animations |
| **M8 Neighborhood** | Multiple lots, visitors, community lots |
| **M9 Roofs & terrain polish**, saves, settings, audio |

## 9. Status (2026-11-08)

**M0 done, M1 done, parts of M2–M4 in.** Everything below is playable in `lifesim`:

- Hearth ECS with tests and a clean Miri run; schedule with per-system timings.
- Renderer: instanced PBR-lite, shadowed sun with day/night cycle, lamp & room lights, MSAA, ACES,
  transparent ghosts, offscreen capture.
- Starter house: two stories + basement, ~35 greybox objects, a family of four (2 adults, child, infant).
- Needs (8 core + 5 personality needs), moodlets, mood; traits (20), quirks (10), personality axes.
- Utility-AI autonomy with advertisements, deficit capping, boredom, critical-need handling;
  player orders with refusals; free will Off/Low/Full (global and per peon).
- Socials (18) incl. baby care; relationships with labels; crying babies wake parents; nanny when
  no grown-up is home.
- Careers: 11 tracks (incl. Supervillain, Mime, Cryptid Hunter, Influencer, Spy with [REDACTED],
  Criminal) with comedic choice events, pay, promotions, firing.
- Aging through 7 stages, death by old age/starvation, one-time Reaper bargain, grief.
- Economy: purchases, sell-back, bills every 3 days, Repo-Gnome.
- Build mode: rooms, walls, delete, doors, windows, floors, stairs, basement digging. Buy mode with
  catalog, placement ghosts, rotation, selling.
- Tooling: simctl run/trace/check/render; in-game inspector, autonomy, overlays, profiler, settings.

Added since:

- **School:** Child/Teen students take the bus Moonday–Fryday 8:00–14:00 (breakfast and lunch included),
  grades from mood + homework (kids' desk), truancy, Fryday report cards, graduation at Young Adult.
- **Family:** "Try for Baby" (either partner may carry), 2-day pregnancy (× lifespan scale), births with
  twins, adoption agency, household cap of 8. **Genetics:** `Genes` (skin, hair, eyes, height, build)
  separate from rendered `Appearance`; skin blends, hair/eyes pick a parent (darker hair slightly
  dominant), rare throwbacks; newborns usually inherit a parent's trait.
- **Ghosts:** the dead get a tombstone in the yard (Mourn / Talk to Grave) and may haunt at night:
  translucent, drifting through walls, smearing ectoplasm, spooking the living. Selling the grave lets them move on.
- **Crowding:** walkers look one stride ahead; they wait for, then route around, people standing in the way
  (squeezing past after ~15 min), and keep right when passing someone head-on.
- **Saves:** whole-world RON snapshots with exact entity handles (`World::spawn_empty_at`); object defs by
  string id. Save → load → continue is bit-identical to never saving (tested). Systems that consume
  randomness iterate in sorted entity order so ECS storage layout never changes outcomes.
- **Roofs:** gable/hip/flat roofs on rectangles, auto-roof (greedy rectangles over top-floor rooms), roof tool,
  procedural meshes with overhangs and gable-end walls. **Terrain:** corner heightmap with raise/lower/
  flatten/smooth brush; floors/walls flatten and lock their corners; objects need flat ground; steep steps block nav.
- **Voices:** formant-synthesized gibberish (no assets) via rodio, pitch by life stage, voiced once per bubble.
- **Create-a-Household:** names, stages, genes/looks, traits, quirk, axes (25-point budget), careers, live
  line-up preview on the lawn, Move In to the starter house.
- **Neighborhood:** townies who live on other lots drop by during the day (or when invited), socialize,
  raid the fridge and go home; neighborhood window lists neighbors and relationships.

- **Rigged bodies:** one glTF per body type (infant, toddler, child, teen, adult, elder) generated by
  `assets/blender/make_peons.py` (skin-modifier body, bone-heat weights, face, 4 hair meshes rigid on the
  head, `slim`/`heavy` shape keys from per-bone normal offsets, clips idle/walk/sit/lie/use/dance/exercise).
  `game::peons` maps `Posture` to a clip, `Appearance` to palette/morphs/hair/height; ghosts reuse it
  translucent. `simctl lineup --pose P` renders every stage side by side for review.
- **Clip cross-fades:** `peons::Animator` (owned by the game, keyed by entity, pruned when a peon isn't
  drawn) keeps each peon's clip phase; on a clip change it fades from the last drawn pose over
  `BLEND_SECS` (0.3 s, smoothstep) and eases the root offset/yaw with it, since the sim snaps a peon onto
  a seat or bed as the posture changes (jumps over 3 m, e.g. coming home, still snap).

- **Interaction clips & props:** interactions and socials name a body clip (`anim:`; socials also
  `target_anim:`) and a held prop (`prop:`, greyboxed from parts in `assets/data/props.ron`, in a
  palm-centred grip frame). The sim stores both on `Posture` while performing; the game plays the
  clip when the body has it (else the pose's clip, and no prop). 26 clips for toddler-and-up bodies:
  eat, chop, stir, scrub, type, write, phone, phone_call, selfie, read, orate, talk, laugh, argue,
  hug, mourn, shower, water, kick, fix (crouched), cradle, peekaboo, admire, plus demeanor variants
  idle/walk_sad (mood < −35), idle/walk_proud (Theatrical/Vain in a good mood), idle_nervous
  (Paranoid). `simctl lineup --anim CLIP --prop ID --demeanor D` previews them.

- **Seated, floor and lying clips:** watch (TV from a seat, remote in hand), watch_floor, sit_chess /
  chess (standing), sit_phone (toilet), sit_lookout (Paranoid armchair vigil), play_floor (toys),
  sleep (beds, sofa nap, crib) and bathe. Chair clips put the origin on the seat like `sit`; floor
  clips on the floor.
- **Using an object from a seat:** an interaction's `seat: (range, cone, anim)` lets it be performed
  from any free slot of a `Sit` + "seating" interaction on another object, within `range` m of the
  object and facing it to within `cone`° (TV: 5 m from the sofa; chess: a chair pulled up to the
  table). The seat slot is reserved on `CurrentAction::seat` and the peon sits there with the seat
  clip; with no free seat, the object's own slots are used (TV: on the floor; chess: standing).
  Autonomy and the pie menu count a free reachable seat as a free spot. `simctl render --order
  OBJECT:INTERACTION` stages the household doing something (e.g. `tv_basic:watch`) before rendering.

- **Eye colour:** `Appearance::eyes` is expressed from `Genes::eyes` (old saves default to brown); the
  models have a tinted `iris` material (palette slot 6) under a smaller dark pupil.
- **Infant clips:** infants get their own sitting-base versions of the clips they play: `sleep` (crib,
  arms up by the head), `play_floor` (toys), and as the target of baby care `laugh` (coo, peekaboo),
  `drink` (bottle), `drowsy` (rocked), `kick_back` (diaper change; toddlers have it too), plus
  `idle_sad` as crying. The clip test now checks each body against the clips its stage can be asked for
  (`GameData::anims_for`). `simctl lineup --target -3.6,0 --dist 1.6` frames the infant.

**Known gaps / next up:** playable multi-lot
neighborhoods (switching households, community lots) — townies are the first step; multi-tile object
picking polish; hygiene still occasionally bottoms out in long headless runs (warning, not a failure).
