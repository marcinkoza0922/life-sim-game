# Simulation CPU performance findings

This is the original baseline. See [first optimization results](OPTIMIZATION.md) for the subsequent improvement.

Measured 2026-11-09 (America/Chicago), on an Intel Core i7-12700K, Linux x86_64,
Rust 1.99.1, optimized `profiling` build at `a12e5f6`. Three seeds (1–3),
60 warmup minutes, 1,440 measured ticks per seed; microbenchmarks use 200
iterations per seed. No other builds or tests ran during this final matrix.
This is an observational baseline on a desktop with normal OS scheduling,
not a pinned-core laboratory comparison. Raw records and toolchain/revision
metadata are in [results](results/). Reproduce with the commands in [README](README.md).

## Largest measured bottleneck: navigation floods

The tick system profile points overwhelmingly at `act/run_actions`. The opt-in
inclusive counters narrow this to repeated `NavField::flood_avoiding` calls,
used both by autonomy and action routing. Navigation and autonomy spans overlap;
their percentages must not be added.

| Residents | Mean tick (ms) | p99 tick (ms) | run_actions share | Navigation share | Autonomy share | Floods/tick |
|---:|---:|---:|---:|---:|---:|---:|
| 4 | 0.227 | 3.607 | 97.0% | 94.6% | 45.6% | 0.13 |
| 16 | 2.439 | 15.307 | 98.9% | 96.0% | 44.9% | 0.93 |
| 64 | 16.601 | 59.939 | 99.5% | 93.1% | 49.4% | 8.29 |
| 128 | 48.538 | 127.304 | 99.7% | 87.9% | 53.5% | 23.05 |

Cells are medians of per-seed metrics (not pooled percentiles). All tick cases
retain their initial living population through the measurement window. Population
scaling also changes decision frequency, crowd contention, and action mix; these
numbers are not an asymptotic complexity claim. At 128 residents, CPU tick cost
alone exceeds a 16.7 ms budget on frames that advance simulation; even the starter household has decision
spikes much larger than its median tick. Actual rendering/audio costs are additional.

## Isolated costs

| Operation | Population / square grid side | Mean (ms) | p99 (ms) |
|---|---:|---:|---:|
| autonomy | 4 | 1.585 | 1.645 |
| autonomy | 64 | 1.698 | 1.776 |
| autonomy | 128 | 1.837 | 1.913 |
| nav | 16 | 0.059 | 0.064 |
| nav | 32 | 0.251 | 0.263 |
| nav | 64 | 1.076 | 1.133 |
| nav | 128 | 4.519 | 4.691 |
| nav-house | 4 | 1.536 | 1.609 |
| nav-house | 64 | 1.528 | 1.580 |
| nav-house | 128 | 1.497 | 1.539 |
| save | 4 | 0.182 | 0.204 |
| save | 64 | 0.936 | 1.017 |
| save | 128 | 1.693 | 1.841 |

`nav-house` uses the identical frozen positions and geometry as `autonomy`;
`nav` uses empty square lots. Compare the pair to isolate the initial flood
from candidate scanning. Do not subtract medians to estimate exact exclusive cost.
Autonomy microbenchmarks rotate through a fixed post-warmup world; their action
mix differs from the evolving tick run. Save capture/JSON serialization is an
occasional millisecond-scale operation at high population, not the dominant tick
cost. The remaining schedule systems have much smaller aggregate costs; optimizing
their query loops first would miss the measured hotspot.

## Why navigation is expensive, and what to try next

Code inspection explains the measured attribution; the following optimization
ideas have not been implemented or A/B measured:

1. **Avoid repeated full floods.** `autonomy::candidates` starts a 200-distance
   Dijkstra flood on every evaluation. Action starts, `path_to`, `path_next_to`,
   and rerouting build additional fields (often with 400-distance budgets).
   Reuse a field between scoring and starting a selected action where valid;
   use goal-directed search for single-target routes. Invalidate caches after
   lot edits/occupancy changes and account for dynamic avoidance.
2. **Precompute traversability/adjacency for static lot geometry.** Every popped
   node checks eight neighbors via `Lot::can_step`, including floor, occupancy,
   wall/opening, terrain and stair queries; diagonals make several such queries.
   Cache static edges by lot revision, keeping dynamic blockers separate.
3. **Reuse navigation storage.** Every flood initializes distance and predecessor
   arrays for all seven supported levels, plus a heap. A 32×32 lot allocates
   56 KiB for those two arrays alone; a 128×128 lot allocates 896 KiB. Reusable
   buffers/generation stamps could reduce allocation and initialization costs.
   These allocation sizes are from code inspection, not an allocator profile.
4. **Reassess decision frequency after navigation is addressed.** Autonomy scans
   object interactions and social targets, builds candidates and sorts them;
   critical-need checks may invoke it again. Cache or stagger decisions only
   after preserving deterministic behavior and measuring the gameplay tradeoff.

## Validation and limits

- All 81 non-ignored simulation tests pass with the profiling feature.
- Three explicit stress tests pass: 64 residents for one day across three seeds,
  navigation/path reconstruction through 128×128 lots, and crowded deterministic
  replay plus save/restore continuation.
- Focused Clippy for all sim targets passes with and without profiling; diff
  whitespace checks pass. The new nested-span counter reset test passes.
- `perf record` was attempted with hardware and userspace software events, also
  outside the sandbox; host permissions prohibit sampling. Attribution therefore
  comes from scheduler timers, opt-in coarse spans, and isolated benchmarks,
  not sampled instruction stacks. No kernel settings were changed.
- This work measures simulation CPU only. It does not rank renderer, GPU, UI,
  audio, asset-loading or whole-game startup bottlenecks. Crowded populations
  intentionally share the starter house and spawn location; this stresses
  contention rather than provisioning a realistic large household.
- Stress checks have no timing thresholds and do not require all residents of
  an undersupplied crowded house to survive indefinitely.
