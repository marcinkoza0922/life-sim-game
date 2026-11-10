# Navigation reuse and social scoring optimization

Commits `66c94a6`, `c55f85d` and `127c2ac` on `profiling-stress` reduce repeated navigation
and decision work after the [first traversal optimization](OPTIMIZATION.md).

## What changed

- Candidate evaluation can hand its navigation field to the selected action's
  startup in the same peon's step. Reuse is restricted to a matching start and a
  fully exhausted reachable frontier. A truncated 200-distance scoring field
  cannot replace a 400-distance route search; incomplete fields fall back to a
  fresh search. Player orders, later social rerouting and dynamic-avoidance
  searches retain their existing behavior. No field is cached across peon steps.
- Partner queries during a read-only decision use one lazy lookup over current
  relationships. The ordinary `committed_to` query also uses a scalar minimum
  instead of allocating and sorting relationship vectors. Both preserve the
  lowest living partner when multiple commitments exist.
- Initiator age/trait eligibility is checked once per social definition.
  Social need-gain scores, helps lists and initiator trait multipliers are
  computed on the first eligible target and reused for other targets. Target
  needs, eligibility, acceptance, boredom and partner-specific penalties are
  still evaluated with the same arithmetic order and deterministic sorting.
- The profiler now emits a final serialized-state fingerprint outside the timed
  window. `compare.py --verify-state` checks fingerprints, iteration counts,
  populations and operation checksums for every seeded case before reporting
  latency changes. Fingerprints are stable FNV-1a values, not cryptographic hashes.

## Back-to-back comparison

The reference is freshly rebuilt `648b30a` with the new profiler fingerprint
output and CPU-affinity runner copied in; simulation sources are unchanged. Its metadata is marked dirty
because of that instrumentation-only patch. The candidate is `1a9d96c` (simulation changes through `127c2ac`).
Both run the existing 51-case matrix with three seeds, 60 warmup ticks, 1,440
measured ticks per tick case and 200 iterations per microbenchmark. The reference
and candidate matrices ran serially after our builds/tests, on the same i7-12700K.
Both matrices pin benchmark processes to logical CPU 8, a performance core.
Builds are not pinned. CPU frequency and other desktop load are not isolated,
so small differences remain observational. Earlier unpinned trial runs showed
large variation on unchanged controls, motivating the affinity option. These
fresh pinned baseline numbers differ from the first report's unpinned timings;
compare the before/after columns here rather than mixing the two datasets.

| Workload | Residents | Before mean (ms) | After mean (ms) | Mean reduction | Before p99 (ms) | After p99 (ms) |
|---|---:|---:|---:|---:|---:|---:|
| tick | 4 | 0.026 | 0.023 | 13.8% | 0.291 | 0.250 |
| tick | 16 | 0.164 | 0.138 | 15.9% | 0.901 | 0.761 |
| tick | 64 | 1.742 | 1.538 | 11.7% | 8.531 | 7.676 |
| tick | 128 | 9.127 | 5.924 | 35.1% | 38.053 | 22.257 |
| autonomy | 4 | 0.272 | 0.232 | 14.6% | 0.702 | 0.522 |
| autonomy | 64 | 0.382 | 0.347 | 9.2% | 0.730 | 0.718 |
| autonomy | 128 | 0.563 | 0.501 | 11.0% | 1.120 | 0.848 |
| nav-house | 64 | 0.210 | 0.210 | 0.3% | 0.227 | 0.415 |
| save | 128 | 1.964 | 1.770 | 9.9% | 4.429 | 2.853 |

Cells are independent medians across the three seeds, not pooled percentiles.
A negative reduction is a slower measurement. Save remains an unchanged control
workload. Candidate scoring includes its navigation flood, so its reduction is
not an exclusive social-code measurement. Empty-grid and house-only navigation
results remain in the raw reports; they provide additional context for machine
variation and the small completion-tracking change in the search loop. The
unchanged save control improved by roughly 10%, so similarly small changes
should not be attributed entirely to this optimization. The flood-count
reductions and matching replay fingerprints do not depend on machine timing.

| Residents | Before floods/tick | After floods/tick | Flood count reduction |
|---:|---:|---:|---:|
| 4 | 0.07 | 0.07 | 0.0% |
| 16 | 0.55 | 0.55 | 0.0% |
| 64 | 5.83 | 5.83 | 0.0% |
| 128 | 14.48 | 14.48 | 0.0% |

These call-count reductions are independent of machine speed. Decision counts
are identical, and all 51 seeded cases have matching final-state fingerprints,
operation checksums and populations. The optimization removes redundant work
without reducing decisions or changing the simulated outcome in this matrix.

Reproduce and verify the recorded comparison:

```sh
python3 tools/profiling/compare.py tools/profiling/results/before-reuse.jsonl tools/profiling/results/reuse.jsonl --verify-state
```

The intermediate eager scoring version also preserved all fingerprints, but
small-household measurements exposed unnecessary setup for unused socials. The
final implementation builds both score entries and the partner index lazily.
Results above cover that final implementation only.

## Validation and limits

- All 87 simulation tests and all three opt-in stress tests pass.
- New tests check bounded-search completion, exact route fallback when the target
  is beyond the scoring budget, and rejection of a field from another start.
- Cached social gates and acceptance probabilities match the ordinary methods
  across every pair/social in a populated neighborhood, including synthetic
  multiple commitments, dead partners and a missing entity generation. Partner
  selection also matches the original allocating/sorted implementation.
- Clippy passes for all sim targets with and without profiling. The comparison
  tool rejects an intentionally mismatched state fingerprint in a smoke check.
- These remain simulation CPU measurements. Renderer/GPU/audio/UI are unmeasured.
  No long-lived lot or relationship caches were introduced; their invalidation
  would require a separate design. Crowd rerouting and other remaining floods
  still contribute substantial work at high populations.
