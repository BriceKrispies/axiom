# `sky/volumetrics.js` — finishing the half-done port

Source: `C:/dev/Claude-of-Duty/src/sky/volumetrics.js` (527 lines).
Target: `apps/shmup/src/sky/volumetrics.rs`.
Golden: `apps/shmup/tests/sky_volumetrics/{capture.mjs,golden.json}`.
Test: `apps/shmup/tests/sky_volumetrics_port.rs`.

Written as a **separate** golden set (`sky_volumetrics/`, not `sky/`) because a
sibling agent was concurrently editing `tests/sky/capture.mjs`,
`tests/sky/golden.json`, `sky/dome.rs` and `sky/clouds.rs`. None of those were
touched here.

## 1. The audit

The module doc on the pre-existing 246-line port claimed four things were
"deliberately not ported". Judged against the source, **one** of the four was a
real GPU boundary. Two whole functions were missing and were not mentioned at
all.

| claim in the old module doc | verdict |
|---|---|
| `SkyPass` / `hdrTarget` / render-target wiring (`Volumetrics` class) | **Legitimate — partly.** Allocating a framebuffer and binding a sampler is not arithmetic. But `resize`'s half-resolution sizing, `render`'s `frame % 64` dither phase, and the history ping-pong plus the first-frame `uBlend = 0` latch are ordinary arithmetic and state that decide what the shaders compute. Those were unfinished work, not plumbing. Now ported. |
| `skRayFor` — "needs a camera projection/view matrix this crate has no type for yet" | **Not legitimate.** One `mat4 * vec4`, a perspective divide, an upper-3x3 rotate, a normalise. The matrices are *inputs*, exactly as the atmosphere LUTs are inputs to `raymarch_sky`. "No type for it yet" is a reason to add the type. Now ported, with a `Mat4`. |
| `RESOLVE_FRAG` — "stateful, frame-to-frame GPU buffer logic with nothing to port as a pure function" | **Not legitimate, and factually wrong.** A fragment shader *is* a pure function of its samplers; the state lives in the render targets outside it. The 3x3 neighbourhood min/max, the widened clamp, the off-screen `w = 0` reject and the final `mix` are all plain arithmetic. Now ported, with the three `texture()` fetches as closures. |
| `skSunVisibility` / `CSM_GLSL` — "no CPU representation of a shadow-map texture atlas" | **Not legitimate.** Cascade selection, the shadow-matrix transform, the projective divide, the border reject, the depth bias and the four Vogel taps are all CPU maths. Exactly **one** thing needs a GPU: `texture(owCsmMaps, vec3(uv, layer)).r`. Now ported, with that fetch as a closure — the shape the crate had already established for `uTransmittanceLut` and `fwidth`. |

Not mentioned by the old doc, and simply absent:

- **`skUpsample`** (`volumetrics.js:325-341`) — the depth-aware bilateral
  4-tap upsample. Never mentioned, never ported.
- **`COMPOSITE_FRAG`'s marched branch** — only the `VOL_ANALYTIC` fallback
  existed. The path the game actually runs was missing.
- **`MARCH_FRAG`'s prologue** — the depth read, the `sky ? uFog.w : min(depth *
  rayLen, uFog.w)` clip, the `maxT <= 0.02` early-out returning
  `vec4(0,0,0,1)`, and the interleaved-gradient dither. `raymarch_fog` took
  `max_t` and `dith` as parameters and no one computed them.

Net: the old file was ~47% of the source and, more importantly, **0% pinned** —
see §4.

## 2. What was added

| source | new symbol |
|---|---|
| `skRayFor` (144-151) | `ray_for`, plus `Mat4` (+ `from_three_elements`, `mul_vec4`, `mul_vec3_upper3x3`) |
| `skVogel` (163-167) | `vogel` (already present) |
| `skSunVisibility` (174-199) | `sun_visibility` + `CsmUniforms` |
| `MARCH_FRAG` prologue (211-221) | `march_frag`, `ray_max_distance`, `march_dither`, `march_frame_phase` |
| `RESOLVE_FRAG` (287-309) | `resolve_frag`, plus `Vec4` |
| `skUpsample` (325-341) | `upsample` |
| `COMPOSITE_FRAG` marched (344-388) | `composite_marched`, `composite_transmittance` |
| `Volumetrics.resize` (469-470) | `half_res_size` |
| `Volumetrics.render` (496-504) | `TemporalState` / `FrameTargets` |
| `uFog.w`, `uFogExt` | added to `FogUniforms` (both were missing) |

Two new value types, both this module's own vocabulary in the crate's
established style (`crate::materials::noise`'s module doc explains why each
GLSL-porting module owns its minimal vector type rather than sharing one):

- `Vec4` — `RESOLVE_FRAG` carries radiance *and* transmittance in one `vec4`
  and clamps/mixes all four channels together.
- `Mat4` — row-major `[row][col]`, matching `celestial::Mat3`.
  `Mat4::from_three_elements` converts from THREE's column-major
  `Matrix4.elements`. This is the storage-order trap; the golden emits THREE's
  own arrays so the conversion is exercised rather than asserted.

## 3. Two real faithfulness bugs found in the *existing* code

Both were shared by the old Rust **and** by the (dead) volumetrics
transcription in `tests/sky/capture.mjs` — the exact failure mode a second
transcription cannot catch. Found by reading the GLSL, not by a test.

1. **`raymarch_fog`'s accumulation used a reciprocal multiply.** The source is
   `L += T * j * sigmaS * ( 1.0 - aT ) / sigmaE;` — a real divide. The port had
   `.scale(1.0 / sigma_e)`. `x / s` and `x * (1/s)` differ in the last bits, and
   this is inside a 40-56 step accumulation. Fixed to
   `.div(Vec3::splat(sigma_e))`.
2. **`composite_analytic` folded two multiplies into one.** The source is
   `inscatter_expr * ( uFog.x / max( 1.0e-6, uFog2.x ) ) * mono` — two separate
   vector multiplies. The port computed `ratio * mono` first and multiplied
   once. Fixed to two `.scale(...)` calls.

Same class of thing in `ray_for` and `sun_visibility` as written: every `/` in
the GLSL is a `Vec3::div(Vec3::splat(s))` here, never `scale(1.0/s)`. The
capture script does the same.

Trap sweep for this file, by name: no `Float32Array` anywhere in
`volumetrics.js`; no `sign()`/`signum` (only `step()`, which is ported as
`gl_step` with the edge-first argument order); no `Math.hypot`; no Euler
composition; matrix storage order handled as above; no `rng.fork()` and no
seeds (this file draws nothing).

## 4. What is pinned, and how honestly

**This slice previously had no test at all.** `tests/sky/capture.mjs` contains
a full hand-transcription of the volumetrics shaders — and **never calls any of
it**. Nothing in `tests/sky/golden.json` comes from those functions
(`node -e "Object.keys(require('./golden.json'))"` lists no volumetrics key),
and `tests/sky_port.rs` contains zero volumetrics assertions. That
transcription is dead code. So was the whole module.

The new golden covers, from `capture.mjs` (30 top-level sections, 48 KB):

- every `SHARED` scalar (`fog_ambient` incl. the vanishing-key `max(1e-4, …)`
  guard, `fog_phase`, `fog_inscatter_phase` incl. the gain-1 identity,
  `fog_near_ramp`, `fog_density` across both sides of the `<= 0.001` early
  return, `height_integral` across both sides of the `|x| < 1e-4` branch);
- `ray_for` against real THREE `PerspectiveCamera` matrices, plus a degenerate
  projection that exercises the `max(1e-6, -vd.z)` clamp;
- `vogel`, and `sun_visibility` across three cascade sets x eight cases —
  four-tap paths in all three cascades (one of which is deliberately
  non-affine so `sc.xyz / sc.w` is a real projective divide), the
  past-last-split return, the `proj.z` out-of-range return, the uv-border
  return, and `owCsmParams.x` at 1, 0.62 and 0;
- `ray_max_distance`, `march_dither`, `march_frame_phase`;
- `raymarch_fog` over five rays x four step counts, with the density noise both
  on and off (which changes *which* steps the `dens <= 1e-4` test skips, not
  just the numbers), and a second pass with the real cascade code inside the
  loop;
- `march_frag` including the `maxT <= 0.02` early-out;
- `resolve_frag` including two cases whose reprojection leaves `[0,1]`;
- `upsample`, `composite_transmittance`, `composite_analytic`,
  `composite_marched`;
- `half_res_size` and a six-frame `TemporalState` sequence with a reset.

### The limitation, stated plainly

`volumetrics.js` is WebGL2 fragment-shader source held in JS template strings.
**Not one line of it can be imported and called** — it only ever runs on a
browser GPU. So there is no oracle, and `capture.mjs` cannot record what the
original does: it hand-transcribes each shader body into plain JS instead.

That means this test pins the Rust against **a second careful reading of the
GLSL**, and nothing more. *It cannot catch a mistake both transcriptions
share.* If `volumetrics.rs` and `capture.mjs` misread the same line the same
way, the suite is green and the port is wrong — which is precisely how the two
bugs in §3 survived in the old code. Auditing correctness means reading three
things side by side: the GLSL, `capture.mjs`, and `volumetrics.rs`. Every
function in both transcriptions carries its `volumetrics.js:<first>-<last>`
range so that read is mechanical.

Two things in the golden *are* genuine oracles: `cloudMacro`, imported from the
original `clouds.js` (it exports a real CPU twin) underneath the cloud-shadow
chain; and the camera matrices, produced by a real THREE `PerspectiveCamera`
and stored in THREE's own column-major element order.

### Tolerances

- **Exact (bit-identical)** for anything built only from `+ - * /`, comparisons
  and `floor`: `fog_ambient`, `fog_near_ramp`, `ray_max_distance`,
  `march_dither`, `march_frame_phase`, `upsample`, `sun_visibility` (its result
  is `mix(1, s*0.25, strength)` over an integer tap count), `half_res_size`,
  `TemporalState`. IEEE-754 `+ - * /` is bit-identical in V8 and Rust.
- **1e-12 relative** for everything touching `exp`/`sin`/`cos`/`sqrt`, which are
  not bit-guaranteed across V8 and Rust's libm. The march chains up to 56 of
  them per ray; 1e-12 leaves roughly two decimal digits of headroom over the
  worst observed accumulation while still catching any algebraic divergence
  (which moves these numbers by whole digits, not in the twelfth).

### The `step()` cliff, and why the golden is not brittle

`sun_visibility` ends in four `step(recv, tap)` compares. A discontinuity in a
golden is dangerous: a 1-ULP libm difference on either side could flip a tap by
a full `1.0`. Two mitigations, both deliberate:

- the synthetic shadow-map sampler is a smooth low-gradient affine function of
  `uv`, not a hash, so a last-bit perturbation of the tap uv moves the sampled
  depth by ~1e-17;
- the fixtures were then **measured**: the minimum `|recv - tap|` over every
  golden case, including the 40-step march that calls `sun_visibility` inside
  its loop, is **5.9e-4**. That is ~12 orders of magnitude of margin.

The synthetic samplers (`csmDepth`, `smoothVis`, `sampleCurrent`,
`sampleHistory`, `sampleVelocity`, `sampleDepthTex`, `sampleVolumeTex`) are test
scaffolding, not ported source, and are duplicated verbatim in
`sky_volumetrics_port.rs` so the two sides feed the functions identical inputs.

## 5. What is still not ported, and what it would take

Only GPU object lifetime remains: `SkyPass` construction, `hdrTarget`
allocation, uniform binding, `dispose()`. There is no arithmetic left in them.
A future WGSL/render-integration slice needs that wiring, not this CPU
reference.

The closures do **not** model what a real sampler does around the fetch:
bilinear filtering, wrap/clamp addressing outside `[0,1]`, and the fp16 storage
of the HDR march/history targets. A caller supplying nearest-neighbour `f64`
lookups gets the algorithm, not the sampler. This is the same boundary
`super::luts`' module doc already draws for fp16 LUT quantization, and it is
recorded here rather than left implicit.

`uCloudParams.z` (`detail_gain`), `cirrus_coverage` and `cirrus_opacity` are set
to zero in the test's `CloudParams`: `skCloudShadow` — the only clouds entry
point the fog pass calls — does not reference them.

## 6. Orchestrator notes

- No `mod.rs` / `lib.rs` / `Cargo.toml` change is needed: `sky/mod.rs` already
  declares `pub mod volumetrics;`.
- `sky/mod.rs`'s module doc (lines 28-34) still says `skRayFor`'s camera-matrix
  ray reconstruction and `skSunVisibility`'s cascade sampling are "not ported,
  in any of these modules". **That is now false** and should be corrected. It
  was not edited here because `mod.rs` is shared and a sibling agent was live in
  the same directory. Suggested replacement for that paragraph:

  > **Not ported, in any of these modules:** the THREE.js-side plumbing each
  > source file also carries — `SkyDome`/`Volumetrics`' render-target and
  > uniform wiring, and `SkyPass`/`fullScreenGeometry` (`fullscreen.js` in
  > full). These are GPU object lifetimes with no portable computation. Where a
  > shader body needs a genuinely GPU-only *input* — a screen-space derivative,
  > a shadow-map atlas, a history buffer — the port takes it as an explicit
  > parameter or closure instead; see [`dome`]'s and [`volumetrics`]'s module
  > docs.
