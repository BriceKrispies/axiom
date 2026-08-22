# `sky/index.js` + `sky/fullscreen.js` → `apps/shmup/src/sky/{system,fullscreen}.rs`

## What landed

| new file | source |
|---|---|
| `apps/shmup/src/sky/system.rs` | `src/sky/index.js:1-872` |
| `apps/shmup/src/sky/fullscreen.rs` | `src/sky/fullscreen.js:1-101` |
| `apps/shmup/tests/sky_system/capture.mjs` + `golden.json` | — |
| `apps/shmup/tests/sky_system_port.rs` | — |

`system.rs` is the whole `SkySystem` facade minus the GPU object graph:
weather/fog state, the shared uniform block, `setTimeOfDay`/`setTimeRate`/
`setWeather`/`cloudShadowAt`, the full `_updateCelestial` ephemeris-to-lighting
chain, `_applyWeather`/`_applyFog`/`_placeLight`/`_applyLightIntensities`, and
the `update`/`lateUpdate` per-frame drive including the dirty/age bake
bookkeeping.

## The golden is a genuine oracle — the real class, really constructed

This is the important thing about this slice and it is worth reusing. Unlike
every other file in `src/sky/`, `index.js` contains **no GLSL**. It is plain
JavaScript arithmetic end to end. So `tests/sky_system/capture.mjs` does not
transcribe anything: it imports the original `SkySystem`, calls the real
`async init(ctx)` against a stubbed WebGL surface, and reads the real fields
back.

The stub surface is exactly the set of renderer methods that return nothing and
whose results nothing reads (`setRenderTarget`, `render`, `compile`, plus
`xr: { enabled: false }` so `PMREMGenerator.fromEquirectangular` does not
throw). `THREE.Vector3/Vector4/Matrix3/Color/DirectionalLight/Scene/
PerspectiveCamera` are all real, and `SkyLuts`/`SkyDome`/`Volumetrics`/
`Celestial` are all really constructed. Nothing measured is downstream of a
stub — `_updateCelestial`, which computes every published value, touches no
GPU object at all.

**This technique should be tried before assuming a subsystem needs
transcription.** It took one probe script to discover that the whole facade
runs headless in Node.

## What is pinned, and how tightly

`tests/sky_system_port.rs`, 17 tests, at `1e-9` relative (the figure
`sky_port.rs` already uses; `transmittanceToSpace` chains `exp`/`sqrt` per
channel and `Celestial` chains `sin`/`cos`/`acos`/`atan2`, none bit-guaranteed
between V8 and Rust libm). Integers, booleans and vertex data are exact.

- **`setTimeOfDay` over 110 hours** (0.00→24.00 in 0.25 steps plus the wrap
  cases `25.5`, `-3.25`, `-0.5`, `48.75` and the hours the source's comments
  argue about). Every call compares the *entire* state: both lights' colour,
  intensity and position, `keyLight`, `ambientColor`, `indirectScale`,
  `exposureBias`, `_beamGain`, `_beamLuminance`, `_baseSunIntensity`, both
  transmittance triples, both dirty flags, and all 30 fields of the shared
  block including the 3x3 celestial matrix.
- **`setWeather`** — 10 patches, each checking the weather struct, the fog
  struct and the whole shared block.
- **`cloudShadowAt`** — 60 (hour, cloudTime, x, z) samples.
- **`update`** — 24 frames of drift/easing, plus a 20-frame `timeRate = 3.0`
  sweep from 19:00 that crosses the beam floor, the sun/moon key handover and
  the night ramps, plus a negative-rate sweep.
- **`lateUpdate`** — three real camera poses, matrices in and out.
- **The bake state machine** — the env bake is gated on `_envAge > 0.2` and on
  the sun having moved 0.35 degrees; the sky bake is not. Both pinned.
- **`fullscreen.js`** — the triangle, the UVs, the size clamp.

## Findings and traps hit

1. **`THREE.MathUtils.lerp` is not GLSL `mix`.** `lerp(x, y, t)` is
   `(1 - t) * x + t * y`; `mix` is `a + (b - a) * t`. `atmosphere.rs` already
   exports `gl_mix` with the second form, and `index.js` uses `MathUtils.lerp`
   six times and `mix` zero times. `system.rs` defines its own `three_lerp`
   with a comment saying exactly this, because reaching for the ready-made
   `gl_mix` is the obvious wrong move.
2. **`radToDeg` is `radians * (180 / PI)`, not `radians * 180 / PI.`** `altDeg`
   feeds four `smoothstep` edges whose outputs multiply the key light.
3. **`MathUtils.smoothstep(x, min, max)` has the *reverse* argument order of
   GLSL `smoothstep(edge0, edge1, x)`.** The body is otherwise identical, so
   `system.rs` forwards to `atmosphere::smoothstep` with the arguments flipped
   rather than growing a second copy of the polynomial.
4. **`setWeather`'s `Object.assign` spill.** `Object.assign(this.weather,
   patch)` runs *before* the three fog-only keys are pulled out of the patch,
   so `fogDensity`/`fogHeight`/`shaftGain` land on `this.weather` too, where
   nothing ever reads them. Modelled as three `Option<f64>` fields and pinned,
   rather than dropped — it is observable, and dropping a field nobody reads is
   how a port stops being diffable.
5. **`update`'s hour is not re-normalised.** `setTimeOfDay` does the
   `((h % 24) + 24) % 24` double modulo; `update`'s `(hour + rate*dt) % 24`
   does not, so a negative `timeRate` walks `hour` negative and keeps it there.
   Pinned with a test that names the quirk.
6. **`setTimeRate(NaN)` freezes the sun**, because `hoursPerSecond || 0` is
   falsy for NaN as well as for `±0`. Same shape as `jsmath::or_one`.
7. **`fullscreen.js` is *nearly* all plumbing — but not entirely.** The
   subsystem's module doc listed it as unported "in full". Three things in it
   are computation and are now ported: the shared triangle's vertex data and
   its `1e8` bounding sphere, `SKY_VERT`'s `vUv = position.xy * 0.5 + 0.5`, and
   `hdrTarget`'s `Math.max(1, w | 0)` size clamp. That last one is a real trap:
   `| 0` is ECMAScript `ToInt32`, which **wraps** modulo 2^32, while a Rust
   `as i32` cast **saturates**. They disagree at `1e21`, where JS gives
   `-559939584` and Rust would give `i32::MAX`. `fullscreen.rs` has a
   `to_int32` and the golden pins the disagreeing rows.
8. **The source's "19:20 golden hour" comments do not correspond to the
   shipped `SITE`.** At `celestial.js`'s latitude 45 / day-of-year default,
   19.2 h still puts the sun 4.6 degrees up and the beam floor is inactive.
   The floor is live between roughly -3 and +2 degrees, i.e. 19.5-20.0 h. The
   test says so rather than asserting the comment.

## The photometric contract, and what an engine-side EV100 pass must assume

The source logs `1 unit = 25000 lx` at the end of `init`, and
`atmosphere::SCENE_LUX` is that number. Everything `system.rs` publishes is on
that scale:

- `base_sun_intensity = SUN_ILLUMINANCE_TOP (128000/25000 = 5.12) *
  smax * discFraction * beamGain`
- `sun_light.intensity = base_sun_intensity * (0.58 + 0.42 * cloudOcclusion) *
  SUN_KEY_GAIN (1.55)`
- `ambient_color = hue * (0.15 * base_sun_intensity + 0.9 * moonIntensity)`
- `shared.sky_rolloff.0 = max(kneeFrac * beamLuminance, 0.02 + 6 * moonI)`

**No exposure curve is applied anywhere in this module.** `exposure_bias` is
published in EV and the source's own doc says the renderer *adds* it to
`settings.exposureBias` (`index.js:98-100`) — it is a metering instruction, not
something already folded into the numbers above. So: a sibling engine-side
EV100 pass must treat these as scene-referred illuminance in 25000-lx units and
consume `exposure_bias` **additively**, or the sky will double-count it. That
assumption is written into `system.rs`'s module doc so the next reader finds it
at the site.

`SUN_KEY_GAIN = 1.55` is worth flagging to that sibling: it is a deliberate,
documented non-physical gain on the **directional light only** (not the
scattered irradiance, not the sky, not the discs), paying for level albedos
measured 1.1 stops darker than the model assumes. It is *inside* the published
`sun_light.intensity` and *outside* `base_sun_intensity`.

## Not ported (and why each is only a lifetime)

- `SkyLuts`/`SkyDome`/`Volumetrics`/`PMREMGenerator` construction and disposal,
  the equirect render target, `render.setEnvMap`. The maths those objects run
  is already ported in `luts.rs`/`dome.rs`/`volumetrics.rs`;
  `SkySystem::sky_view_params()` marshals the shared block into
  `luts::bake_sky_view`'s argument so a caller owning render targets has
  nothing left to derive. `bake_sky`/`bake_env` keep the dirty/age bookkeeping.
- `ctx.scene.add` / `render.addLight` / `registerPass`, and `dispose()`.
- `this.volumetrics.reset()` in `setTimeOfDay` (`index.js:399`) — a
  temporal-history-buffer invalidation on the volumetric pass. It has no CPU
  state the facade owns; whoever wires the real pass calls it off the
  `sky:changed` payload.
- The two `console.info` traces.
- From `fullscreen.js`: the module-level `BufferGeometry`/`Scene`/`Camera`/
  `Mesh` singletons, `blit`, the `SkyPass` class, and every
  `WebGLRenderTarget` option.

## Wiring the orchestrator must add

```
apps/shmup/src/sky/mod.rs: pub mod fullscreen;
apps/shmup/src/sky/mod.rs: pub mod system;
```

`sky/mod.rs`'s "Not ported" paragraph should also drop `fullscreen.js` from its
"in full" list — it is now partially ported, for the reason in finding 7.
