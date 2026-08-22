# GTAO — `src/render/gtao.js` (324 lines) → `modules/axiom-gpu-backend/src/gtao/`

Ground-truth ambient occlusion (Jimenez et al. 2016): the visibility-arc
integral, its velocity-reprojected temporal accumulator, and its depth-aware
separable bilateral. This is the pass that supplies the contact darkening the
reference has in every corner, under every ledge and where every prop meets the
ground, and that `axiom-street-agx.png` has none of.

## What landed

| file | what |
|---|---|
| `src/gtao.rs` | module root: the tuning constants, the two conventions, `frame_phase`, and the not-yet-bound expiry check |
| `src/gtao/reference.rs` | the CPU reference — the semantic definition, 24 `pub(crate)` fns, all branchless, all covered |
| `src/gtao/wgsl.rs` | `GTAO_WGSL` (binding-free library) + `GTAO_CORE_PASS_WGSL` / `GTAO_TEMPORAL_PASS_WGSL` / `GTAO_BLUR_PASS_WGSL` |
| `src/gtao/parity.rs` | CPU↔GPU parity over eight entry points, plus a compile check of the three passes |

`src/gtao.rs` + `src/gtao/` (no `mod.rs`) is the `cascade.rs` + `cascade/`
shape this crate already uses, and it means this slice created no shared file.

## Wiring the orchestrator must add

```
modules/axiom-gpu-backend/src/lib.rs:
    // Ground-truth ambient occlusion: `render/gtao.js` as WGSL plus its CPU
    // reference. Three passes -- the horizon-arc integral, a
    // velocity-reprojected temporal accumulator, and a depth-aware separable
    // bilateral -- over `gbuffer`'s normal/depth/velocity. Pure arithmetic and
    // the pass text; nothing binds them yet.
    mod gtao;
```

Nothing else. No `Cargo.toml`, no `app.toml`, no `scene_wgsl.rs` change.

## Entry points

**WGSL** (`pub(crate) const`, concatenate `GTAO_WGSL` in front of any pass):

- `axiom_gtao_vs(@builtin(vertex_index) u32) -> @builtin(position) vec4<f32>`
- `axiom_gtao_core_fs(@builtin(position) vec4<f32>) -> @location(0) vec2<f32>`
- `axiom_gtao_temporal_fs(@builtin(position) vec4<f32>) -> @location(0) vec2<f32>`
- `axiom_gtao_blur_fs(@builtin(position) vec4<f32>) -> @location(0) vec2<f32>`

**Rust** (`crate::gtao::reference`), the load-bearing few:

```rust
fn view_pos(uv: [f32; 2], depth: f32, proj_inv: &[f32; 16]) -> [f32; 3]
fn arc(h: f32, n: f32, cos_n: f32, sin_n: f32) -> f32
fn radius_px(radius: f32, p11: f32, resolution_y: f32, depth: f32) -> f32
fn step_offset(step: usize, noise2: f32, radius_px: f32) -> f32
fn horizon_update(cos_h: f32, ds: [f32; 3], v: [f32; 3], inv_r2: f32) -> f32
fn horizon(taps: &[Tap], p: [f32; 3], v: [f32; 3], inv_r2: f32) -> f32
fn slice_frame(normal: [f32; 3], v: [f32; 3], dir2: [f32; 2]) -> SliceFrame
fn slice_direction(slice: usize, noise: f32) -> [f32; 2]
fn slice_visibility(cos_h_neg: f32, cos_h_pos: f32, frame: &SliceFrame) -> f32
fn resolve_visibility(sum: f32) -> f32
fn temporal_weight(feedback: f32, history_uv: [f32; 2], history_depth: f32, current_depth: f32) -> f32
fn temporal_clamp(history_ao: f32, current_ao: f32, neighbours: [f32; 4]) -> f32
fn blur_accumulate(centre: [f32; 2], taps: &[[[f32; 2]; 2]; 3]) -> (f32, f32)
fn blur_output(sum: f32, wsum: f32, apply_curve: bool, intensity: f32) -> f32
fn store_rg16f(value: [f32; 2]) -> [f32; 2]
```

## Bindings each pass expects

**Core** — group 0: `{0}` uniform `{ proj_inv: mat4x4, texel: vec2, resolution:
vec2, params: vec4, p11: vec4 }` (112 B), `{1}` `GBufferChannel::Depth`
(`R32Float`), `{2}` `GBufferChannel::Normal` (`Rgba16Float`), `{3}` a **nearest**
sampler. Target: `RG16Float`.

**Temporal** — `{0}` uniform `{ texel: vec2, params: vec2 }` (16 B), `{1}` the
core's output, `{2}` the previous frame's *un-blurred* accumulator, `{3}`
`GBufferChannel::Velocity`, `{4}` nearest sampler. Target: the other history
buffer.

**Blur** — `{0}` uniform `{ texel: vec2, direction: vec2, params: vec4 }` (32 B),
`{1}` the source, `{2}` nearest sampler. Run twice: `direction = (texel.x, 0)`
with `params.x = 0`, then `direction = (0, texel.y)` with `params.x = 1`.

Five `RG16Float` targets total (`rtRaw`, `rtBlur`, `rtFinal`, two history), and
**the history must never be the blur's target** — `render()`'s comment: *"the
history must stay un-blurred or the accumulator smears more every frame."*

The consumer contract (`materialpatch.js`, a sibling slice): the final `r` is
sampled at `gl_FragCoord.xy * screenTexel`, run through
`mix(1.0, max(ao, 0.25), aoStrength.x)`, and multiplies **indirect** light only,
plus a `0.35`-weighted nibble at the direct term.

## The parity budgets — DERIVED, NEVER MEASURED

Per the final-wave brief, nothing in this wave compiled. Both budgets are
reasoned from the arithmetic and are labelled as unverified in the source.

| tier | entry points | expected budget | reasoning |
|---|---|---|---|
| ordinary | `leaf`, `geom`, `direction`, `temporal`, `blur` | **`3e-5`** scaled | one `exp`/`cos` over unit-scale values, a few ULP each, plus one permitted `fma` contraction in the blur's seven-tap sum: `~3e-7`. `3e-5` is 100x that, deliberately generous for a first run. |
| `acos` in the chain | `slice`, `integral`, `horizon` | **`5e-4`** scaled | `d(acos)/dx = 1/sqrt(1-x²)` is unbounded at the poles and `cosH` genuinely reaches `0.9997` on a close tap — 41x amplification. A 2-ULP `inverseSqrt` (which WGSL permits and Rust's `1.0/sqrt` does not do) is `2.4e-7`, so `~1e-5` into the horizon angle and `~2e-5` out. `5e-4` is 25x that. |

Both are **too loose to keep**. The integration run should report the real
per-entry-point numbers (the assertion message carries all eight), tighten each
budget to roughly 2-3x the measurement, and add a `MEASURED_WORST` beside it in
the shape `crate::agx` uses.

Scoring is `|got - want| / max(|want|, 1)`, agx's form, so the intermediate-valued
lanes (`view_pos` reaches ~170 at the far end of the depth sweep) are scored on
the same scale as the unit ones.

## Findings — five things a "reasonable" transcription would have got wrong

1. **The file header says two slices. `#define OW_SLICES 3` says three.** The
   prose is stale. Pinned by
   `gtao::tests::the_slice_count_is_the_defines_not_the_headers`.
2. **The constructor's radius and intensity are dead.**
   `new THREE.Vector4(0.9, 1.35, 0, 0.4)` and `new THREE.Vector2(0, 1.25)` are
   both overwritten every settings apply by `index.js:855-856` with
   `aoRadius: 1.35` and `aoIntensity: 1.1` (`index.js:386,389`). The step loop's
   own comment reasons about *"a 1.35 m radius"*, which is the settings value.
   All four are recorded as named constants so the dead pair cannot be mistaken
   for the live one.
3. **There is no thickness term.** `uParams` is documented
   `x radius y intensity z frame w thickness` and `AO_CORE` reads **only `.x` and
   `.z`**. The thickness heuristic is the falloff blend in `horizon_update`:
   `fall = clamp(len²/r², 0, 1)` **squared**, used as the third argument of
   `mix(c, cosH, fall)` — so a tap at the full radius contributes *nothing* and a
   tap at the origin contributes its raw cosine. That quartic ramp is what stops
   a distant silhouette occluding like an infinitely deep wall. Both dead lanes
   are ported and named.
4. **`glsl.js`'s header comment about velocity is wrong.** It says the delta
   *"can be added directly to a uv to reproject into the previous frame"*; both
   `gtao.js` and `taa.js` write `huv = vUv - vel`, and `taa.js`'s own fallback
   (`vel = vUv - prevUv`) settles it. The shaders are right.
5. **The source's step-distribution comment overstates by one tap.** It claims
   the quadratic ramp *"puts the first three inside six pixels"*; at zero jitter
   the taps are at 1, 3 and **9** px. The point it is making — that eight linear
   steps put the first tap sixteen pixels out and sampled *no* contact in the
   frame — is exactly right, and is what is ported. Pinned in
   `reference::tests::the_step_distribution_is_quadratic_with_a_one_pixel_floor`.

## Three convention corrections, all WebGPU's `v`, all named

The source runs on WebGL (framebuffer `v` up). Every one of these is a
silent-wrong-picture bug rather than a compile error.

1. `NDC_UV_V_SIGN` — `owViewPos` reconstructs from `uv * 2 - 1`, which is only
   NDC if `v` runs up. The pass applies `vec2(uv.x, 1.0 - uv.y)` at all three
   call sites; the transcribed function is left exactly as the GLSL writes it so
   the correction stays visible at the caller.
2. `SCREEN_STEP_V_SIGN` — **the important one.** `sliceDir = vec3(dir2, 0.0)` is
   a *view-space* vector, so stepping along `+dir2` must move the sample **up**
   the screen, i.e. toward smaller `v`. Get it wrong and `cosHPos`/`cosHNeg` swap
   relative to `orthoDir`, which is precisely the failure the source's own
   comment warns about: *"Getting this the wrong way round collapses the
   visibility arc on every grazing surface."* It would not read as a sign bug; it
   would read as GTAO simply not working on any wall.
3. `gl_FragCoord.y` is measured from the **bottom**; `@builtin(position).y` from
   the top. Corrected as `resolution.y - position.y` before either noise, so the
   dither pattern is the source's rather than its mirror.

Plus `gbuffer::VELOCITY_TEXTURE_V_SIGN` on the temporal reprojection, which was
already named by the G-buffer slice.

## Storage width

Every target is `hdrTarget(w, h, { type: THREE.HalfFloatType, format:
THREE.RGFormat })` — **RG16Float**. The chain core → temporal → blur-H → blur-V
quantises three times, and the history read is a fourth. It is the *depth*
channel that matters: `f16`'s step at 30 m is 1.6 cm, and the blur's
`exp(-|Δd|·22/d)` turns that into a ~1% weight change. `reference::store_rg16f`
is the quantisation and is pinned by test.

## Deferred, with its expiry check

**Nothing in the crate binds this pass.**
`gtao::tests::nothing_in_the_present_path_compiles_this_yet` reads
`scene_renderer.rs` and `post_chain.rs` and fails the moment either mentions
`gtao`. What would make it live: `live_gpu_binding`/`offscreen` rendering
`gbuffer`'s targets, then five `RG16Float` targets and the three passes above in
order. **Delete that test in the same change as the wiring** — a deferral without
an expiry check is how four defects in this port were born.

## For the orchestrator

- **`bloom_pyramid::half_storage` has earned its lift.** Its header says *"the
  moment a second pass needs it, lift it whole"*; this is that second pass, and
  `reference::store_rg16f` reaches across into it. Its real home is
  `hdr_target.rs` (a property of a float attachment, not of a bloom). Not done
  here: it means editing `bloom_pyramid/mod.rs` and `hdr_target.rs`, both other
  slices' files. Files to change: `src/bloom_pyramid/half_storage.rs` (move),
  `src/bloom_pyramid/mod.rs` (drop the `mod`), `src/hdr_target.rs` (add it),
  `src/bloom_pyramid/*.rs` + `src/gtao/reference.rs` (retarget the path).
- **`owIGN` now has two CPU references.** `cascade::shading::ig_noise` is the
  same function from `csm.js` (`owIGNoise` there is byte-identical to `owIGN` in
  `glsl.js`), but `mod shading` is private to `cascade`, so it is unreachable.
  `gtao::reference::ign` duplicates it. One of them should win — probably lifted
  beside `half_storage`, since a third consumer (SSR, TAA) is coming.
- **`frame_graph` already names this module.** `frame_graph/schedule.rs` lists
  `"crate::gtao"` as `FramePass::Gtao`'s owner and reserves a `StepTarget::PassOwned`
  ping-pong pair gated on `pipeline.runs_gtao()`. The module path this slice
  created matches; no rename needed on either side.
- **Nothing else needed from a sibling.** `gbuffer.rs` supplies everything this pass
  reads, and its `VELOCITY_TEXTURE_V_SIGN` was already the right shape.
