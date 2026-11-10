# GPU profiling — 2026-11-09

Hardware: NVIDIA RTX 3060 Ti, driver 595.112.07, Vulkan, release build.
Timing tables were captured in the `character-models` development checkout. The
GPU changes are isolated onto `main` for the PR; character assets and shader
features differ, so these numbers describe that development fixture rather than
a benchmark of the PR branch itself.
Command: `cargo run --release -q -p simctl -- render --profile 100`.
Default seed 1, 60 simulated minutes, default camera, cutaway walls, 1600×1000.
Each case warms up 20 frames and measures 100 fixed-scene frames. Default geometry:
475 draws, 117 batches, 18 skinned instances. Raw results: `out/gpu-profile.txt`
(local generated output).

| Case (additional arguments) | Shadow median ms | Main median ms | Sum median ms | Sum p95 ms |
|---|---:|---:|---:|---:|
| Default | 0.103 | 6.718 | 6.822 | 9.562 |
| `--no-ao` | 0.111 | 0.976 | 1.086 | 1.172 |
| `--hard-shadows` | 0.103 | 6.676 | 6.781 | 7.665 |
| `--no-shadows` | 0 | 6.672 | 6.672 | 7.564 |
| `--indirect-light` | 0.104 | 13.195 | 13.301 | 14.574 |
| `--size 800x500` | 0.105 | 2.526 | 2.638 | 3.156 |
| `--size 1920x1080` | 0.104 | 8.151 | 8.257 | 9.118 |
| `--greybox` | 0.028 | 5.521 | 5.548 | 6.025 |
| Default repeat | 0.103 | 6.726 | 6.830 | 7.765 |
| `--no-ao --hard-shadows` | 0.103 | 0.825 | 0.929 | 1.013 |
| `--no-ao --no-shadows` | 0 | 0.812 | 0.812 | 2.289 |
| `--no-ao --greybox` | 0.030 | 0.768 | 0.798 | 0.886 |
| `--hour 21` | 0.050 | 6.450 | 6.501 | 7.349 |
| `--hour 21 --no-ao` | 0.053 | 0.852 | 0.906 | 0.920 |

## Findings

1. **Proxy AO is the first optimization target.** Removing AO saves about 5.74 ms,
   or 84% of the measured pass budget. Night shows a similar 5.60 ms difference.
   `shader.wgsl::indirect` traces four rays per shaded fragment through up to 128
   proxies; indirect lighting doubles the rays and adds bounce lighting, costing
   another 6.48 ms over defaults. Resolution scaling supports a substantial fragment
   workload: quartering pixels reduces main time from 6.72 to 2.53 ms.
2. **Shadow-map rendering is small here.** It costs about 0.10 ms. With AO disabled,
   hard filtering saves roughly 0.15 ms in the main pass; removing shadows saves
   about 0.27 ms including the shadow pass. Filtering is a secondary target.
3. **Object/character complexity is secondary in this view.** Greybox reduces
   batches from 117 to 11 and removes skinning, but saves only 1.27 ms with AO,
   or 0.29 ms without AO. This comparison also changes silhouettes, materials,
   and proxy bounds, so it does not isolate skinning, triangle, or draw-call costs.

## Next experiments

- Reduce the per-fragment proxy search with a spatial grid or another local
  candidate list. Compare against identical images and the fixed scene benchmark.
- Give AO its own hit path: current nearest-hit handling computes a hit normal
  even when bounce lighting is disabled. Measure guarding that work behind GI.
- Test reduced-resolution AO with depth/normal-aware upsampling. This requires
  render-graph changes; evaluate contact-detail quality and edge artifacts.
- After AO, isolate triplanar texture sampling, opaque/transparent work, and
  skinning with additional instrumentation or a hardware capture. Current
  pass-level timestamps cannot identify ALU, cache, bandwidth, or occupancy limits.

## Measurement limits

These GPU timestamps cover the shadow and main passes, including main-pass MSAA
resolve. Their sum excludes inter-pass work, CPU scene building, buffer uploads
before the passes, UI, presentation, and PNG readback/encoding. Every sample waits
for query readback; timings describe isolated pass costs rather than live-game
throughput. Desktop GPU activity and clock changes affect tails; baseline medians
repeated within 0.01 ms but p95 varied. No GPU clocks were locked. Character art
was being edited concurrently in the workspace, so comparisons that depend on
exact geometry should be rerun after those edits settle. One initial run hit an
allocation failure; the isolated retry and full matrix completed successfully.

## Spatial AO optimization and quality settings

Implemented after the initial captures:

- A 16×8×16 world-space grid stores a 128-bit candidate mask per cell. Each
  proxy's world AABB is expanded by 3.05 m (the 3 m ray reach plus normal offset
  and rounding margin). A fragment looks up one cell for all its rays, then scans
  only the set bits. Rotated and nonuniformly scaled proxies use conservative
  world bounds. Dense cells retain every proxy; there is no new capacity cutoff.
- AO-only hits skip the normal transform and hit bookkeeping needed for color
  bounce. The existing sphere/box tests and nearest-hit order are preserved.
- Low/Medium/High AO uses 2/4/8 rays respectively. Medium preserves the previous
  four-ray default. The regular lighting menu, F8, persisted `graphics.ron`, and
  `simctl --ao-quality low|medium|high` expose the choice. Old preferences load
  Medium. With indirect lighting enabled, AO shares its eight-ray trace; lowering
  the saved AO quality does not lower indirect-light quality or cost.

Fresh pre-change default baseline: **6.821 ms** total median (main 6.716 ms).
Post-change captures use the same hardware, scene/camera, 20 warmup frames and
100 samples at 1600×1000. Raw output: `out/gpu-ao-optimized.txt`.

| Configuration | Shadow median ms | Main median ms | Sum median ms | Sum p95 ms |
|---|---:|---:|---:|---:|
| Low AO | 0.103 | 1.568 | 1.671 | 1.723 |
| Medium AO (default) | 0.103 | 2.405 | 2.512 | 2.937 |
| High AO | 0.103 | 4.258 | 4.364 | 4.891 |
| Indirect light | 0.104 | 4.367 | 4.474 | 5.075 |
| AO disabled | 0.111 | 0.974 | 1.085 | 1.183 |
| Night, Medium AO | 0.050 | 2.142 | 2.193 | 2.627 |
| Night, Low AO | 0.053 | 1.536 | 1.590 | 1.956 |

Default GPU pass time falls about **63%**, from 6.821 to 2.512 ms. Low saves a
further 33% relative to Medium. Indirect lighting falls from the earlier 13.301
to 4.474 ms (about 66%). AO-disabled timing remains essentially unchanged.
The grid adds CPU construction and 32 KiB of uniform mask data per frame, which
these pass timestamps do not measure. Maximum proxy count and approximate box
occlusion remain unchanged.

Validation: CPU coverage checks include rotated, negatively/nonuniformly scaled
boxes, negative coordinates, empty grids, and all 128 mask bits in dense cells.
GPU captures compare spatial tracing pixel-for-pixel against an exhaustive scan
at all three quality levels, with and without bounce lighting. The existing
contact-darkening/color-bounce capture test also passes. Low/Medium/High scene
renders were inspected (`out/ao-low.png`, `out/ao-medium.png`, `out/ao-high.png`);
Low retains contact shading with coarser sampling. Renderer/game/simctl tests:
57 passed in the development checkout; 46 passed after isolating the changes on
main for the PR. Clippy passes with warnings denied on both checkouts.
