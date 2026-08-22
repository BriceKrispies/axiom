# `render/probe.js` + `render/env.js` — the "indirect-lighting pair"

Slice files written: `modules/axiom-gpu-backend/src/probe.rs`,
`modules/axiom-gpu-backend/src/env.rs`. Nothing built, nothing tested, nothing
committed (final-wave brief).

## 1. Correction to the brief: `probe.js` is not a light probe

**`src/render/probe.js` (306 lines) contains zero indirect lighting.** It is
`RenderProbeScene` — a procedural *blockout validation scene*: an fBm noise
field baked into albedo/normal/ORM `DataTexture`s, a 120x120 asphalt ground,
fourteen concrete blocks along a street, twenty-two crates, four metal spheres
of graded roughness, three emissive lamps with point lights, and a
`SHOT_KEEPOUT` table that stops the random street swallowing a named shot
camera. Its own header: *"This exists ONLY so the render subsystem can be
developed and screenshotted before the world subsystem lands… Nothing here is
shipped content."* `index.js:1218` (`_ensureProbe`) adds it on frame <= 4 and
deletes it once six foreign meshes appear.

It is therefore **not ported by this slice**, for two reasons, in order of
severity:

1. **It cannot legally live in a module.** Every geometric quantity in it comes
   from `rng.range` / `rng.int` on the app's xoshiro128\*\* `Rng`
   (`src/core/rng.js` -> `apps/shmup/src/rng.rs`). A `modules/` crate may not
   depend on an app, and re-implementing the RNG inside `axiom-gpu-backend`
   would create the seventh copy of a primitive this port has already spent a
   consolidation pass collapsing (`crate::jsmath`).
2. It is content, not backend. Its correct Axiom home is
   `apps/shmup/src/render/probe.rs` (a directory that does not exist yet).

Recommend the orchestrator re-list it as an **app-tier** slice. It is
self-contained — `makeSurface` + `footprintClear` + `build()`, importing only
`three` — and worth ~1 agent.

## 2. Where the indirect lighting actually lives

The boot log the brief quotes (`[render] indirect gate: 2 interior volumes`) is
`index.js:1215`. Provenance of every part:

| source | what it owns | ported here |
|---|---|---|
| `materialpatch.js:206-238` `owInteriorGate` | the volume test + blend | yes |
| `materialpatch.js:249-256` `owSunBounce` | the warm anti-sun wrap | yes |
| `materialpatch.js:148-186` (`lights_fragment_maps` injection) | the two-band fill + the IBL budget | yes |
| `index.js:1091-1152` `_updateBounceFill` | the CPU side producing the band colours | yes (`bounce_fill_bands`) |
| `index.js:1156-1216` `_updateRooms` | building footprints -> volumes | yes (`interior_volumes`) |
| `materialpatch.js` `owSampleAO`, `owMultiBounce`, `owSpecularOcclusion`, `owContactShadow`, `MaterialPatcher` | AO / contact-shadow plumbing | **no** — `materialpatch.js` / `gtao.js` slices |
| `env.js` (all 106 lines except PMREM) | the fallback IBL equirect | yes (`env.rs`) |

**Overlap warning for the orchestrator.** `_updateRooms` and
`_updateBounceFill` live in `render/index.js`, which is the frame-graph
sibling's file. I ported the *arithmetic* of both into `probe.rs` because they
are the gate's own definition; the sibling should **call** them, not re-derive
them. If the sibling has also transcribed them, keep one and delete the other —
do not keep both.

## 3. The "2 interior volumes", derived

The brief guessed the two volumes come from the level's fifteen interior light
anchors. They do not, and the real arithmetic is a nice cross-check on the port:

`_updateRooms` keeps buildings with `spec.enterable === true`, then **drops
`collapse` or `ruin`** ("a collapsed or ruined shell is open to the sky: it must
keep its skylight, or the one room in the level with a hole in its roof is the
one that reads as a cave"). Axiom's ported level has exactly three enterable
buildings — `W2`, `E1`, `E3` (`apps/shmup/src/world/layout.rs:317, :462, :542`)
— and `E3` carries `ruin: true`. **3 - 1 = 2.** The fifteen anchors
(`apps/shmup/src/world/system.rs:265`, `interior_anchors`; the count is pinned
in `apps/shmup/tests/world_system/golden.json`) are hanging bulbs *inside* those
shells. A port that built two volumes from fifteen anchors would match the log
and be wrong.

The volume is the building's own footprint (`spec.x/z/w/d`), `y` from `-0.8`
(below the ground slab, so the floor plate counts as interior) to the roof deck
less `0.06` — or, when a setback publishes a start floor, that floor's height
less `0.06`, because a terrace is outdoors and sits inside the footprint.

The gate keys off **depth inside the box**, not containment, and that is the
whole trick: a facade's outer skin is at depth 0 and its inner skin one wall
thickness in, so `smoothstep(0.06, 0.30, d)` separates the two faces of the
same wall without per-room geometry.

## 4. `FrameAmbient`: replace the blend, keep the carrier, add one lane

`axiom_host::FrameAmbient` is a strength-folded `sky`/`ground` pair a backend
applies as `mix(ground, sky, up)`. That is the *same quantity* as `owSkyFill` /
`owGroundFill`, and **not** the reference's IBL. Three differences, in order of
visual weight:

1. **The blend is wrong, not merely ungated.** `FrameAmbient` lerps; the source
   deliberately does not. The two bands are independently gated by two
   smoothsteps (`owFillDir = (-0.95, 0.85, -0.05, 0.7)`), with a comment saying
   why: *"Lerping them put a warm street bounce on every wall and made shadows
   come out warmer than the sun that cast them."* On a vertical wall
   (`up = 0`) the source gives `smoothstep(-0.95, 0.85, 0) = 0.5416` of the sky
   band and `smoothstep(-0.05, 0.7, 0) = 0.0127` of the ground band — a sum of
   `0.554`, not a partition of unity. A `mix` forces `1.0` and hands ~46% of
   that wall's fill to the warm band. **This is the single cheapest change that
   would move Axiom's look**, and it needs no new frame data at all.
2. **Both bands are gated by the interior volume and occluded by `sqrt(ao)`** —
   `sqrt`, never `ao`: *"a fill term that AO can drive to zero is not a fill, it
   is just another way to make a black hole."* `FrameAmbient` has no gate, which
   is precisely why Axiom's interiors read flat.
3. **A third term has no room in a hemisphere pair**: the warm anti-sun wrap
   (`owSunBounce`), a directional term scaled off the *ground* band.

Minimal honest contract: keep `FrameAmbient`'s two colours (they are
`bounce_fill_bands`'s two outputs) and add **one** neutral lane — a
`FrameIndirect` peer of `crates/axiom-host/src/frame_ambient.rs` carrying
`fill_dir: [f32; 4]`, `fill_gain: [f32; 2]`, `indirect: [f32; 4]`, the level
transform `[f32; 4]` and up to 10 volumes. Everything else the fragment stage
already has.

Note also that `apps/shmup` currently feeds `FrameAmbient` from
`scene/sky_look.rs` — an **invented** fixed-exposure Reinhard (`display()`,
`EXPOSURE = 1.0`) that throws away the 25000-lx scale — and not from
`sky/system.rs`, whose correct `ambient_color` / `indirect_scale` /
`exposure_bias` have zero consumers. `bounce_fill_bands` is written to take
exactly those three, so wiring it up also retires the invented transform.

## 5. Traps found and how each was handled

- **`THREE.DataUtils.toHalfFloat` TRUNCATES.** `env.js` stores into a
  `Uint16Array` of `HalfFloatType`, so storage width is part of the algorithm —
  and the conversion is the fox-toolkit table method,
  `baseTable[e] + ((f & 0x7fffff) >> shiftTable[e])`, with **no rounding term**.
  This crate's existing `bloom_pyramid::half_storage::to_half_bits` rounds to
  nearest even, because that is what an `Rgba16Float` *attachment* does. They
  are different functions and disagree on roughly half of all inputs; a test in
  `env.rs` measures the disagreement over 4096 sampled radiances and asserts
  truncation never lands further from zero. Reusing the existing helper would
  have been this port's "wrong implementation propagated by citation" defect
  again.
  The f64 -> f32 -> f16 double narrowing is also the source's: `clamp` runs in
  f64, `_tables.floatView[0] = val` narrows to f32, then the table truncates.
  Consequence worth knowing: `toHalfFloat(Infinity)` is `0x7bff`, **not** `Inf`,
  because the clamp to +-65504 runs first.
- **`Vector3.divideScalar` is a reciprocal multiply** —
  `return this.multiplyScalar( 1 / scalar );` (`three/src/math/Vector3.js:559`).
  All three normalisations in `_updateBounceFill` go through it, so here the
  *source* is the reciprocal-multiply and transcribing it as a division would be
  the usual trap run backwards. Written as `1.0 / m` then multiplied, and pinned
  by a test that reproduces the whole chain bit-for-bit on a divisor (49) where
  the two forms differ. The two genuine divisions in that function
  (`_ambLevel / 0.15`, `bounceFill / max(groundFill, 1e-6)`) stay divisions.
  Also asymmetric on purpose: the first `divideScalar`'s `Math.max` has **no**
  `1e-6` floor; the second and third do.
- **Dead computation ported.** `skyRadiance`'s `cosTheta = Math.max(-0.2,
  dir.y)` is read by exactly one expression that re-clamps at 0, so the -0.2
  floor can never influence a result. Ported as written, with a test that proves
  it dead across the whole band it could bite in.
- **`-0.0` selects the sky arm.** `env.js` branches on `dir.y < 0.0`, which is
  false at `-0.0`; reproduced and asserted.
- **`Math.hypot` deliberately NOT called here.** `_updateRooms` recovers the
  level yaw with `Math.hypot(c, sn)`, which in V8 is a max-scaled Kahan sum and
  not `f64::hypot` (this port measured 37.5% disagreement on metre-scale
  triples). The faithful primitive is `apps/shmup/src/jsmath.rs::hypot`, which a
  module may not reach, so `LevelTransform::from_world_axis` takes the axis
  length as a **parameter**. The app supplies it.
- **`/ 1.12` stays a division** in `owSunBounce`; the WGSL is asserted not to
  contain `0.892857`.
- **The `1e-4` guard is added to all three components** of the anti-sun vector
  (`vec3(-sun.x, 0.28, -sun.z) + vec3(1e-4)`), not just the horizontal ones.
- **Grouping preserved.** The hemisphere term is
  `( sky * skyG + ground * gndG * indoor ) * ( fillAo * gain.x )` — the sky band
  carries `indoor` *inside* `skyG` while the ground band takes it as a third
  factor. Same factors, different association. And the two `irradiance +=`
  statements are returned as **separate lanes** (`IndirectTerms.hemisphere` /
  `.sun_bounce`) so a caller can apply them in the source's order rather than
  summing them first.
- **The viewmodel exception.** `index.js:1410-1429` sets `owIndirect.z = 0` for
  the view-scene pass, because the gate is a world-space test and the
  viewmodel's world position is the camera's — *"standing in a shop would drop
  the weapon's whole indirect term at once."* The same block scales both bands
  by `settings.viewFillOcclusion`. Documented on the module; the frame graph
  owns honouring it.

## 6. Deferred, with expiry checks

- **PMREM.** `PMREMGenerator.fromEquirectangular` is a multi-pass GPU prefilter
  inside three, and there is nothing in Axiom for it to feed: the frame contract
  has no environment lane and the lighting model has no specular IBL term.
  `env.rs` builds the equirect and stops.
  **Expires when** either a `FrameEnvironment` lane lands in
  `crates/axiom-host/src/` or a specular-IBL term lands in the lighting suffix —
  both of which are `modules/axiom-gpu-backend/src/scene_wgsl.rs`.
  `env.rs::tests::nothing_in_the_present_path_builds_this_environment_yet`
  greps the render paths and fails when it does, so the deferral cannot expire
  silently. (The four "not ported" claims this port has already found rotting
  are the reason the check is a test and not a comment.)
- **`probe.js`** — see §1. Expires when `apps/shmup/src/render/` exists.

## 7. Tests — written, NOT RUN

Per the final-wave brief, nothing was built. Every figure below is therefore
either derived from the source text (safe) or an **estimate** (flagged).

- `env.rs` — 13 CPU tests. All are exact-value or structural assertions against
  the JS; none needs a GPU, because **`env.js` contains no GLSL at all**. There
  is consequently no `env` parity proof to write, and that is a property of the
  source rather than an omission.
- `probe.rs` — 15 CPU tests plus one GPU parity proof
  (`indirect_probe_wgsl_agrees_with_the_cpu_reference_on_a_real_adapter`),
  shaped exactly like `agx::parity` / `surface_program::parity`, using the shared
  `crate::test_gpu::TestGpu::shared()` fixture and `test_gpu::validating` for the
  error scope. Three entry points so a miss is attributable:
  `probe_gate_fs` (indoor / gate / wrap / raw room depth), `probe_hemi_fs`
  (the two-band fill + IBL scale), `probe_wrap_fs` (the anti-sun term).

### The tolerance, and why it is looser than the maths suggests

`TOLERANCE = 3.0e-5` scaled-relative; `EXPECTED_WORST_UNVERIFIED = 8.0e-6`.
**Both unverified.** The account:

1. `axiom_probe_indoor` opens with `world_pos.x * xf.x + world_pos.z * xf.y +
   xf.z` at street-scale coordinates (the sweep reaches ~25 m). One f32 ULP at
   25 is `1.9e-6`, and a GPU may contract that into two `fma`s where Rust may
   not — so `lx`/`lz` can differ by ~`2e-6` **absolute** before anything else.
2. `room_depth` is a difference of those, inheriting that absolute error while
   its own magnitude collapses to zero at a volume boundary, which is where the
   interesting samples are.
3. `smoothstep(0.06, 0.30, d)` divides by `0.24` (~4.2x) and the cubic's slope
   peaks at 1.5, so `2e-6` of position becomes up to `1.3e-5` of `indoor`.
4. The gate and the fill are near-unit-slope in `indoor`, so that lands
   undiminished on a result of magnitude <= 1.

So the budget is dominated by **coordinate magnitude**, not by conditioning.
If the measured figure needs more than `3.0e-5`, the correct response is not a
bigger constant — it is to ask whether the level transform should be evaluated
camera-relative, which is a real design question the measurement would be
raising.

The integration pass owns replacing `EXPECTED_WORST_UNVERIFIED` with the number
the adapter reports. The assertion message says so verbatim.

## 8. Wiring the orchestrator must apply

```
modules/axiom-gpu-backend/src/lib.rs: mod probe;
modules/axiom-gpu-backend/src/lib.rs: mod env;
```

Both are pure and compile on every target (the `agx` / `exposure` precedent);
only their parity mods are behind `#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]`.
Expect `dead_code` warnings until a frame lane consumes them — the same state
`agx`, `exposure`, `cascade` and `bloom_pyramid` are in, and deliberately not
silenced with `#[allow]`, because the warning disappearing is how you know the
wiring landed.

No `Cargo.toml`, `module.toml`, `scene_wgsl.rs` or app change is needed or was
made.
