# First navigation optimization

Commit `30d91d4` on `profiling-stress` adds a search-local traversal cache.
It is rebuilt for every flood, so public lot edits, door changes, object moves,
terrain edits and save/restore need no new invalidation mechanism.

The cache stores packed wall/door/corner flags and lazily memoizes walkability
and ground heights. Dijkstra expansion order, heap tie-breaking, stair traversal,
dynamic avoidance and distance budgets are unchanged. Normal builds and the
profiling feature use the same optimized search.

## Measured results

Same machine, optimized profile and 51-case/three-seed matrix as the original
[baseline](FINDINGS.md); no other builds or tests ran during the new matrix.
These are desktop observations rather than pinned-core laboratory measurements.
Raw candidate results and revision metadata are in `results/optimized.*`.

| Workload | Population / grid side | Baseline mean (ms) | New mean (ms) | Mean reduction | Baseline p99 (ms) | New p99 (ms) |
|---|---:|---:|---:|---:|---:|---:|
| tick | 4 | 0.227 | 0.042 | 81.7% | 3.607 | 0.552 |
| tick | 16 | 2.439 | 0.279 | 88.6% | 15.307 | 1.638 |
| tick | 64 | 16.601 | 2.617 | 84.2% | 59.939 | 10.810 |
| tick | 128 | 48.538 | 12.270 | 74.7% | 127.304 | 42.634 |
| nav-house | 64 | 1.528 | 0.247 | 83.8% | 1.580 | 0.261 |
| nav | 16 | 0.059 | 0.056 | 4.8% | 0.064 | 0.079 |
| nav | 32 | 0.251 | 0.245 | 2.5% | 0.263 | 0.315 |
| nav | 64 | 1.076 | 1.136 | -5.5% | 1.133 | 1.264 |
| nav | 128 | 4.519 | 3.051 | 32.5% | 4.691 | 3.209 |
| autonomy | 64 | 1.698 | 0.726 | 57.3% | 1.776 | 0.836 |
| save | 128 | 1.693 | 1.843 | -8.9% | 1.841 | 1.984 |

Medians are computed independently over the three seeds, as in the matrix runner.
Compare all workloads with:

```sh
python3 tools/profiling/compare.py tools/profiling/results/baseline.jsonl tools/profiling/results/optimized.jsonl
```

All 51 cases retain identical operation checksums, initial/final populations and
navigation/autonomy call counts relative to the baseline. This optimization makes
each flood cheaper; it does not reduce how many floods the simulation performs.
Save is included as a control workload; serialization is unchanged. Its measured
latency increased 9–17%, showing material run-to-run variation or allocator/cache
state effects. Small differences such as the 5.5% slowdown on the empty 64×64
lot are therefore inconclusive without a controlled paired rerun. This change
primarily benefits furnished-house traversal; it does not consistently improve
all empty-grid cases. The household tick reductions are much larger than those
control differences.

## Correctness checks and tradeoffs

- Full simulation suite and all three opt-in stress tests pass.
- New tests exhaustively compare every adjacent movement against `Lot::can_step`
  across the starter house and 16 randomized multi-level lots with walls, doors,
  windows, occupancy, missing floors and terrain slopes. They also rebuild after
  direct lot collection edits that do not bump the revision.
- A frozen pre-optimization search produces identical complete distance and
  predecessor arrays for blocked/invalid starts, avoidance, stairs and bounded
  floods. Thus equal-cost route tie-breaking is preserved in those cases.
- Clippy for all sim targets passes with and without profiling.
- Extra temporary storage is about 18.4 KiB per flood at 32×32, or 289.8 KiB at
  128×128, in addition to the existing distance/predecessor arrays and heap.
  Cache setup still happens for short searches. The per-search design avoids
  persistent stale-cache risks at the cost of repeating setup and allocations.

At 128 residents, navigation now accounts for about 52% of tick time, and
autonomy about 68% (inclusive, overlapping). Candidate processing deserves
closer attribution alongside further navigation work.

Next, measure reusing fields between autonomy and action starts, or a persistent
traversal snapshot with explicit invalidation. Those changes can reduce repeated
work further but require stronger tracking of lot and occupancy mutations.
Renderer, GPU, audio and UI costs remain outside this CPU simulation benchmark.
