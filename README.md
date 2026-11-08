# Life Sim (working title) — on the Hearth engine

A lightly comedic 3D life simulator / virtual dollhouse written in Rust on a custom archetype ECS.
See [`docs/DESIGN.md`](docs/DESIGN.md) for the full design.

## Run

```sh
cargo run --release -p game --bin lifesim            # play (starter family, seed 7)
cargo run --release -p game --bin lifesim -- --seed 3 --minutes 300   # different family, 5h in
```

### Controls

| Input | Action |
|---|---|
| LMB on a household member | select |
| LMB on object / other peon / floor | action menu for the selected peon (Shift+LMB on household member to interact) |
| RMB drag / Q E | rotate camera |
| MMB drag / WASD / arrows | pan |
| Wheel / `+` `-` | zoom |
| PgUp / PgDn | change level (basements are negative) |
| Space, 1–4 | pause, speed (1x / 2x / 3x / Ultra) |
| F5 / F6 / F7 | Live / Buy / Build mode (time pauses while building) |
| R | rotate object / stairs being placed |
| V | wall mode: cutaway → down → up |
| Tab / Home | next peon / focus camera on selected peon |
| Delete | sell hovered object (Buy mode) |
| L | story log |
| F12 | screenshot to `screenshots/` |
| Esc | close menu / cancel tool / back to Live |

### Debug tools (in game)

| Key | Window |
|---|---|
| F1 | ECS inspector: every entity and a Debug dump of each component |
| F2 | Autonomy: the selected peon's scored options ("why did they do that?"), boredom list |
| F3 | Overlays: room ids, object occupancy, walk paths; tile/room info under the cursor |
| F4 | Profiler: frame time, per-system sim timings, draw/batch counts |
| F8 | Sim settings & cheats: free will, lifespan scale, aging/bills/accidents, +money, max needs, age up, hot-reload `assets/data` |

## Headless tooling (`simctl`)

```sh
cargo run --release -p simctl -- run --days 3 --seed 1          # daily JSON summaries, time budget per peon, story log
cargo run --release -p simctl -- run --days 3 --seed 1 --trace Priya   # every action start/finish with needs
cargo run --release -p simctl -- check --seeds 40 --days 6      # invariant sweep (starvation, collapse, idling, bankruptcy)
cargo run --release -p simctl -- render --hour 21 --level 1 --out out/night.png   # offscreen PNG of the lot
```

`lifesim` also has test hooks for automated captures of the full UI:
`--screenshot-after FRAMES PATH --mode live|buy|build --level L --debug --cam YAW,PITCH,DIST --menu-on OBJECT_ID`.

## Tests

```sh
cargo test --workspace                     # ECS, sim units, end-to-end scenarios, build/buy tools
cargo +nightly miri test -p hearth_ecs     # validate the ECS's unsafe storage code
```

## Layout

| Crate | What |
|---|---|
| `crates/hearth_ecs` | custom archetype ECS (entities, queries with runtime borrow checks, resources, events, deferred commands, schedule) |
| `crates/hearth_render` | wgpu 30 renderer: instanced PBR-lite, sun shadows, point lights, MSAA, offscreen capture |
| `crates/sim` | the simulation — deterministic, headless; needs, autonomy, actions, nav, lot/rooms, careers, aging, death, economy |
| `crates/game` | the `lifesim` binary plus scene building, camera, tools and egui UI |
| `crates/simctl` | headless runner, invariant checker, offscreen renderer |
| `assets/data` | RON content: objects, traits, quirks, socials, moodlets, careers, names |
