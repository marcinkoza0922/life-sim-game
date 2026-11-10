# CPU profiling and capacity tests

See the [original baseline](FINDINGS.md) and [first optimization results](OPTIMIZATION.md).

Run from the repository root. No new dependencies or graphics device are required. The matrix enables the
`sim/profiling` feature for coarse thread-local counters; normal builds contain
neither the counters nor the span timers.

```sh
python3 tools/profiling/run.py --out out/baseline.jsonl
python3 tools/profiling/compare.py out/baseline.jsonl out/candidate.jsonl
cargo nextest run -p sim --test stress --run-ignored ignored-only
```

The matrix builds once with the optimized `profiling` Cargo profile and runs serially.
It covers 4/16/64/128 residents, repeated autonomy decisions, navigation on
16/32/64/128 square lots and the actual starter house, and save capture plus JSON serialization. Three seeds
(default) provide both repeatability and workload variation. Preserve the JSONL;
console output is the median of each seed's mean and p99, not a pooled percentile.
Do not run competing builds or benchmarks while collecting comparison baselines.
Record CPU, revision, Rust version, power settings, and background load with results.

For a single workload:

```sh
cargo build -q --offline --profile profiling -p sim --example profile --features profiling
target/profiling/examples/profile tick 64 1440 1
target/profiling/examples/profile autonomy 64 200 1
target/profiling/examples/profile nav 128 200 1
target/profiling/examples/profile nav-house 64 200 1
target/profiling/examples/profile save 64 200 1
```

Arguments: mode, population (or grid side for `nav`), iterations, seed.
Starter populations have a minimum of four. Additional adults have no job and
spawn together outside the starter house: this deliberately stresses contention,
social candidate scans and routing rather than representing a balanced large house.
Sixty warmup ticks are excluded. Setup/content loading is excluded from all timings.
Autonomy repeatedly scores a frozen post-warmup world, rotating through residents;
it does not execute the proposed actions. Navigation uses an empty ground-floor
lot with a 200-tile distance budget (covers these grids). `nav-house` uses the same positions and 200-tile budget as
autonomy, isolating its initial flood on the actual house geometry. Save includes capture,
serialization, allocation and destruction of the resulting string; it excludes I/O.

JSON includes mean/p50/p95/p99/max operation latency, initial/final living population,
seed, iterations, wall time and per-system arithmetic means for tick workloads.
With the profiling feature, `inclusive_spans` reports call counts and total
microseconds for navigation floods and autonomy scoring. These are inclusive:
autonomy includes navigation, so never sum them. The matrix also writes a
`.meta.json` sidecar with revision, dirty status, toolchain and environment.
System times include ECS deferred flushes. Report collection runs outside the tick
latency window, but affects cache state and wall time. Schedule timers themselves
remain enabled as in normal gameplay. Very small system measurements include timer
noise. Maxima and tails are sensitive to OS scheduling; repeat before concluding.
These measurements cover simulation CPU costs, not rendering, audio or GPU latency.

For call stacks on Linux (perf must be available and permitted):

```sh
perf record -F 199 -g --call-graph dwarf -o /tmp/life-sim-perf.data -- \
  target/profiling/examples/profile tick 64 10000 1
perf report --stdio -i /tmp/life-sim-perf.data --no-children --percent-limit 1
```

Symbols are retained in the profiling build. Keep raw perf data outside git.
For an allocation/CPU-heavy operation substitute `autonomy`, `nav` or `save`.
The ignored nextest tests verify finite bounded needs/positions during a crowded
three-seed one-day soak, serializability, clock progress, crowded deterministic replay/save-restore, and large-grid route
reachability/path endpoints. They intentionally have no machine-specific timing
thresholds or survival requirement for an undersupplied 64-person house.
