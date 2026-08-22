# `render/bloom.js` — the Jimenez/COD bloom pyramid

Slice: **the bloom pyramid** (spine, `modules/axiom-gpu-backend`).
Source: `src/render/bloom.js` (215 lines), plus the one bloom line of
`src/render/composite.js:116-117` and the settings at `src/render/index.js:206,349,357-358`.

Delivered — `modules/axiom-gpu-backend/src/bloom_pyramid/`:

| file | what | gated? |
|---|---|---|
| `mod.rs` | `BloomTuning`, `SOURCE_SETTINGS`, `CONSTRUCTOR_DEFAULTS` | no |
| `prefilter.rs` | `owBloomPrefilter`, `karisWeight`, `owLum`, the knee floor | no |
| `filters.rs` | the 13-tap downsample (both arms), the 9-tap tent, the blend, the combine, the tap tables | no |
| `schedule.rs` | `setSize` mip sizing; the per-level radius/weight schedule | no |
| `half_storage.rs` | `Rgba16Float` round-to-nearest-even (the mips are `HalfFloatType`) | no |
| `reference.rs` | the whole pyramid over a CPU image — the semantic definition | no |
| `wgsl.rs` | `BLOOM_PYRAMID_WGSL` (shared filters) + `BLOOM_PASSES_WGSL` | wasm32/offscreen |
| `chain.rs` | `BloomPyramid` — the real wgpu passes over `Rgba16Float` mips | wasm32/offscreen |
| `parity.rs` | CPU↔GPU parity on a real adapter, three tiers | test+offscreen |

63 tests. The six always-compiled files are **100%** regions / lines / functions /
branches (`cargo llvm-cov --branch -p axiom-gpu-backend --lib`). `post_chain.rs`
was **not** edited. `agx.rs` / `exposure.rs` were **not** touched.

---

## 1. This was a faithfulness pass, and almost nothing survived it

`post_chain.rs` already carried "a bloom". It was not this algorithm. Every row
below is a real behavioural divergence, now fixed:

| | `post_chain` (still, unchanged) | `bloom.js` (now ported) |
|---|---|---|
| structure | 1 level, half res | pyramid, 6 levels (5 on the low tier) |
| downsample | none — the bright pass is a 1-tap copy | 13-tap at ±2 and ±1 texel |
| firefly guard | none | Karis luminance average, level 0 only |
| prefilter driver | **Rec.709 luma** | **max channel** |
| prefilter denominator | `4·knee` | `4·knee + 1e-5` |
| prefilter clamp | ratio clamped to `0..=1` | unclamped |
| exposure | none | taps scaled by the metered scalar **before** the threshold |
| clamp | none | `min(24)` after the karis combine |
| blur | separable 9-tap Gaussian, H then V | 9-tap tent upsample back up the chain |
| accumulation | n/a | 50/50 **alpha blend**, not a sum |
| wide levels | n/a | radius `0.62`, weight `0.34` on the top two |
| combine | `scene + glow·intensity`, then a per-channel rolloff | `hdr += max(bloom,0)·max(strength,0)`, pre-tonemap |
| storage | 8-bit sRGB | `Rgba16Float` |
| threshold / knee / strength | app-authored `FrameBloom` | `1.6` / `0.9` / `0.14` |

**The driver is the one to notice.** Luma-driven, a red tracer at `(1.9, 0, 0)`
measures `0.40` and never blooms under a threshold of `1.6`. Max-channel driven
it measures `1.9` and blooms as hard as a white light of the same peak. That is
the source's stated reason for the choice, and it is why muzzle flashes and
tracers read as lights rather than as bright paint.
`prefilter::a_saturated_red_blooms_because_the_driver_is_the_max_channel` pins it.

Grouping traps specifically hunted, per the brief:

- `/ max(w0+…+w4, 1e-5)` is **three divisions**, not a reciprocal multiplied
  three times, on both sides.
- `t = uTexel * uRadius` is `(1/w) * radius`, **not** `radius / w`.
- Every `(a+b+c+d)` is left-associated as GLSL groups it; the plain arm's four
  accumulations are in the source's order.
- `clamp` is written out as `min(max(…))` in the WGSL, since a builtin may factor
  differently.
- `l - thr` is named once because GLSL's `l - thr + knee` already shares it.

## 2. G1: the bright pass does NOT see HDR, and this module cannot make it

`RenderCapability::HdrTargets` has landed, and it is a **capability bit only**:

- `hdr_target.rs` resolves and grants the bit at bind (`live_gpu_binding.rs:456-462`).
- `surface_encode::scene_target_format` still returns `surface.add_srgb_suffix()`
  — an 8-bit sRGB format.
- `offscreen.rs:14` still has `COLOR_FORMAT = Rgba8UnormSrgb`.
- `post_chain::PostChain::new` is still handed that format for the scene, the
  ping and the pong.

So a fragment that emits `4.0` is **still clamped to white before any post pass
samples it**, and a bloom thresholding at `1.6` still cannot rank two blown
highlights. `post_chain.rs`'s module docs already say this in prose (lines
17-52) and remain accurate; what changed with the capability is that the split
is now *declarable*, not that the plumbing moved.

Nothing in this slice can lift it: the clamp happens in the scene pass upstream,
and rewiring the scene target is `live_gpu_binding.rs` / `offscreen.rs`'s line.
**Owed to whoever owns those two files.** Every target in `chain.rs` is
`Rgba16Float`, so the pyramid itself is ready the moment the scene target is.

## 3. Storage width is part of the algorithm

`pass.js`'s `hdrTarget` is `THREE.HalfFloatType`, so all six downsamples and all
five blended upsamples round to `f16` on store — eleven quantisations in a chain
whose job is to accumulate. `half_storage.rs` models that with a branchless
round-to-nearest-even pair, exercised over **all 65 536** half bit patterns.

Without it the end-to-end parity would have measured ~5e-4 for a reason that is
not the shader's, and the tolerance would have been measuring the storage while
claiming to measure the port. With it, the whole-chain delta is one `f16` ULP.

`half_storage` is really `hdr_target.rs`'s topic, not a bloom's. It lives inside
the slice because it has exactly one consumer; **lift it whole** the moment a
second pass needs it.

## 4. Measured tolerances

Vulkan adapter, this machine. Both are asserted every run, so neither can rot.

| tier | measured | budget | ratio |
|---|---|---|---|
| tap tables (`bloom_down_tap` / `bloom_up_tap` rendered back) | `0` | bit-for-bit | — |
| filter arithmetic (taps handed in by uniform) | **1.907e-6** = `2^-19` | `4e-6` | 2.1x |
| whole chain (`BloomPyramid` vs `reference::render`, 64x64) | **6.1035e-5** = `2^-14`, one `f16` ULP | `3e-4` | 4.9x |

The three tiers are separate on purpose: folding them into one number would let
the loosest hide the other two. The chain number is *not* the sampler — the tent
runs at `uRadius = 0.62` and a texture unit need only carry 8 bits of subtexel
precision, which would have cost three orders of magnitude more; this adapter
carries far more, and the test is what will say so if a future one does not.

Also pinned: **a zero-strength bloom is bit-identical to no bloom**, on both
sides (`filters::a_zero_strength_bloom_is_bit_identical_to_no_bloom`,
`parity::a_zero_strength_bloom_is_bit_identical_on_the_gpu`), including the
negative-strength floor. The one exception is stated rather than hidden: a scene
channel of `-0.0` normalises to `+0.0`, which GLSL's `hdr += …` does too.

## 5. `lut.js` and `env.js` are NOT this chain

Checked, and they are not:

- **`lut.js`** is the procedural 33³ colour-grading LUT — ASC-CDL slope/offset/
  power, split tone, luminance-preserving saturation, highlight desaturation, a
  filmic S-curve. It runs **after** AgX, in display-referred space
  (`composite.js:133+`). That is the exposure/AgX slice's neighbourhood, not the
  bloom's; it should go to whoever owns `agx.rs`.
- **`env.js`** is fallback image-based lighting — an analytic sky baked to a
  half-float equirect and run through PMREM, used only until `sky/` hands over
  a real environment. It is a lighting input, upstream of the whole frame graph.

Neither shares a function, a uniform or a target with `bloom.js`.

## 6. Overlap with the exposure/AgX slice — the seam, precisely

None in code; one seam in ordering, and it is easy to get wrong.
`composite.js:107-117` is:

```glsl
hdr *= exposure;                                  // the SCENE is scaled here
vec3 bloom = max( texture2D( tBloom, vUv ).rgb, vec3( 0.0 ) );
hdr += bloom * max( uGrade.x, 0.0 );              // a plain ADD
```

The bloom is exposure-scaled too — but by its **own** prefilter, thirteen taps at
a time, inside `downsample_karis`. So the wiring must apply the metered exposure
to the scene before `filters::combine`, and must **not** apply it again to the
bloom. Twice squares the metering; not at all makes the threshold mean something
different at every time of day, which is the exact failure the ordering exists to
prevent. This is documented at `filters::combine`.

`BloomTuning::exposure` is a plain `f32` because
`texture2D( tExposure, vec2( 0.5 ) ).r` reads a **1x1 `THREE.FloatType`** target
(`exposure.js:144-148`) — full `f32`, so a uniform scalar is the same number and
the texture buys nothing. It is exactly `exposure.rs`'s `key / (1.2 · 2^ev)`.

## 7. Not ported, deliberately

- **The render-scale sub-rect.** `post_chain` allocates at full tier size and
  draws into the lower-left `live` fraction; `chain.rs` sizes its mips from the
  source extent it is given. Threading `live` through eleven passes has its own
  parity story and belongs with the frame-graph wiring.
- **`FrameBloom::radius`.** The source fixes the tent radius at `1.0`/`0.62` by
  schedule, so the engine's authored radius has no analogue in this algorithm.
  Whoever wires this must decide whether `FrameBloom` grows a `levels` lane and
  loses `radius`, or whether the pyramid keeps its own tuning struct.

## 8. Wiring owed

- `modules/axiom-gpu-backend/src/lib.rs`: **`mod bloom_pyramid;`** — I added this
  one line myself (with its comment, beside `mod hdr_target;`). Without it the
  slice cannot compile, cannot be covered, and cannot be proven, which would have
  cost more than the merge conflict it risks. Nothing else in `lib.rs` moved.
- Nothing binds `BloomPyramid` yet — the same state `agx.rs`, `exposure.rs`,
  `gbuffer.rs` and `cascade.rs` are in. It expects a scene view + extent and a
  level count (`schedule::LEVELS_HIGH` / `LEVELS_LOW`), and yields
  `output()` / `output_size()` for a composite to sample.

## 9. Cross-slice findings

- **The offscreen suite has hit a per-process ceiling on wgpu instance creation,
  and the next slice to land will trip it too.** Twenty places in
  `modules/axiom-gpu-backend/src/` call `wgpu::Instance::default()`; roughly
  fifty `#[test]`s each acquire a fresh instance + adapter + device from one.
  Past about that count in one process this machine's driver dies with a
  `STATUS_ACCESS_VIOLATION`, intermittently, **inside whichever GPU test is
  running when the count is reached** — during this slice, a sibling's
  `gbuffer::gpu_tests::the_prepass_writes_every_channel_the_consumers_will_bind`.

  Measured, so the diagnosis is not a guess:

  | configuration | result |
  |---|---|
  | `bloom_pyramid` alone (63 tests) | green 3/3 |
  | `gbuffer` alone | green 2/2 |
  | `bloom_pyramid` + `gbuffer` | green 3/3 |
  | full suite, `--skip bloom_pyramid` | green 4/4 |
  | full suite, `--skip bloom_pyramid::parity` (my 58 CPU tests stay) | green 2/2 |
  | full suite, everything | 1 green / 4 crash |

  Two mechanisms ruled out by measurement: it is **not a race**
  (`--test-threads=1` crashes at the same rate), and it is **not hold time**
  (acquiring per test, so each device drops at once, is exactly as flaky as one
  `OnceLock` device held for the module). It is the number of *creations*.

  What this slice did about it: contribute the minimum possible — a `OnceLock`,
  so the module creates **one** instance for all five of its GPU tests instead
  of five. That is the smallest footprint of any GPU module in the crate and it
  is the right shape anyway.

  What it did **not** do, and what is owed: one shared instance + device fixture
  for the crate's whole offscreen test suite. That spans ~20 sibling files and
  is the orchestrator's call, not a slice's. Until it lands, `cargo test -p
  axiom-gpu-backend --features offscreen` is intermittently red for reasons
  unrelated to whatever changed.
- `crates/axiom-surface/src/surface_kind.rs:78` carries an `engine_no_branching`
  finding (a `match` on `SurfaceKind`). Pre-existing, another slice's file.
- `modules/axiom-gpu-backend/src/cascade.rs` did not compile for a window during
  this slice (`project` unresolved at 676/684); it was fixed by its owner before
  I finished. Noting only so the timing is not mistaken for this slice.
