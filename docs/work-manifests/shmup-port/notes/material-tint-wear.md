# `materials/shader.js` tint / wearMaterial / final channel remap → `material_shader/tint_wear.rs`

Stage 4i. The **tail** of `MAIN_FRAGMENT`: what the wear mask selects, the tint
multiply, the roughness remap, and the assignment of `alb`/`orm`/`nShade` into
the six `SurfaceOut` channels the lighting stage consumes.

| | |
|---|---|
| source | `C:/dev/Claude-of-Duty/src/materials/shader.js:588-590`, `:620-634`, `:671-673`, `:697-778` |
| also | `src/materials/index.js:203-207`, `src/materials/library.js:328,335,377` |
| module | `modules/axiom-gpu-backend/src/material_shader/tint_wear.rs` (1293 lines) |
| WGSL | `TINT_WEAR_WGSL` |
| parity | `material_shader::tint_wear::parity`, `--features offscreen` |

## The WGSL entry points

All free functions taking explicit arguments — no globals, no `params.slots`, no
assumed binding index. `axiom_mat_channels`/`axiom_mat_finish` name `SurfaceOut`,
which `surface_program::wgsl_template::SURFACE_PRELUDE_WGSL` declares.

```wgsl
fn axiom_mat_wear_albedo(alb: vec3<f32>, wear_color: vec3<f32>, wear_mask: f32,
                         wear_material: vec4<f32>) -> vec3<f32>
fn axiom_mat_wear_orm(orm: vec3<f32>, wear_mask: f32, wear_material: vec4<f32>) -> vec3<f32>
fn axiom_mat_normal_strength(n: vec3<f32>, amp: f32) -> vec3<f32>
fn axiom_mat_tint(alb: vec3<f32>, tint_color: vec3<f32>) -> vec3<f32>
fn axiom_mat_roughness_remap(roughness: f32, rough_p: vec4<f32>) -> f32
fn axiom_mat_alpha_cut(alpha: f32, alpha_test: f32) -> bool
fn axiom_mat_channels(alb: vec4<f32>, orm: vec3<f32>, shade_normal: vec3<f32>,
                      emission: vec3<f32>, material_opacity: f32,
                      alpha_mask: f32) -> SurfaceOut
fn axiom_mat_finish(alb: vec4<f32>, orm: vec3<f32>, shade_normal: vec3<f32>,
                    emission: vec3<f32>, tint_color: vec3<f32>, rough_p: vec4<f32>,
                    material_opacity: f32, alpha_mask: f32) -> SurfaceOut
```

**Call order** matters and is not enforceable from inside one layer:
`axiom_mat_normal_strength` is applied by the *sampling* layers at
`shader.js:299`/`:357`/`:369`; `axiom_mat_wear_*` by the `masks` layer inside
`OW_VCOL_MASKS` (`:588-590`), i.e. **before** cloth and **before** tint;
`axiom_mat_finish` last. `axiom_mat_finish` internally holds tint → roughness
remap → assign, which is the source's own order at `:621`, `:624`, `:626-628`.

`alpha_mask` is `0.0`/`1.0`, the runtime stand-in for the source's compile-time
`#ifdef OW_ALPHA_MASK`, so one program serves both. At either endpoint the `mix`
is exact, so neither case pays a rounding for the other's existence.

## The `wearMaterial` metalness bug, ported fixed and pinned

`DEFAULT_PARAMS.wearMaterial` is `[ roughness, METALNESS, unused, tint amount ]`
at full wear mask. Its metalness **used to default to `0.5`**, so every worn edge
on concrete, plaster, brick, timber, hessian and the road turned half metal and
picked up a specular tint it has no business having. Only the metal library
entries, which set their own `wearMaterial`, should ever raise it.

`DEFAULT_WEAR_MATERIAL = [0.42, 0.0, 0.0, 0.5]` carries the fixed value.
`the_wear_material_metalness_default_is_zero_not_the_half_metal_bug` asserts the
constant, asserts that a fully-worn concrete texel comes out at metalness `0.0`,
and **exhibits the bug** — the same call with `0.5` in lane 1, showing the
half-metal result — so the two can never be confused by a future reader.

Note the asymmetry the source builds in: the albedo lerp is scaled by
`wear_material.w` (the *tint amount*, `0.5`) but the roughness and metalness
lerps are not — they go all the way to the wear material at full mask.

## sRGB: three's curve, and the measurement that qualifies the trap

`tint` and `wearColor` are hex sRGB reaching the shader through
`new THREE.Color(hex)`, i.e. three's `SRGBToLinear` (`three.core.js:6491`):

```js
( c < 0.04045 ) ? c * 0.0773993808 : Math.pow( c * 0.9478672986 + 0.0521327014, 2.4 )
```

`srgb_hex_to_linear` uses **three's form**, computed in `f64` (JavaScript's
width, and `THREE.Color` stores `f64`) then narrowed once to the `f32` the
uniform carries. That ordering is the storage-width answer for this site.

**Measured, and it qualifies the trap.** Comparing three's form against the GLSL
`(c + 0.055) / 1.055` form over all 256 byte values:

| width | bytes where the two forms agree |
|---|---|
| `f64` | **2** — only `0` and `255` |
| `f32` (what the uniform carries) | **256** — all of them |

So the divergence the brief names is real at `f64` — 254 of 256 — and sits
**below the resolution of the uniform that transports it**. Three's form is
still what is used, because it is what the source computes; but the honest
record is that on this path the fix was correctness, not pixels. Pinned by
`the_two_srgb_forms_differ_in_f64_but_not_once_narrowed_to_the_uniforms_f32`,
which computes both forms rather than asserting about them.

## The roughness remap's order

`roughness` is `[scale, offset, minimum]`, reaching the shader as
`owRoughP = (scale, offset, detile, minimum)` (`shader.js:833-840` — `detile`
rides in `.z` and belongs to the `detile` layer). The source, `shader.js:624`:

```glsl
orm.g = clamp( orm.g * owRoughP.x + owRoughP.y, max( owRoughP.w, 0.015 ), 1.0 );
```

Scale, **then** offset, **then** the minimum as the *lower clamp bound* — so the
answer to "is the `max` before or after the offset" is **after**. The minimum is
itself floored at a hard `0.015`, because tile, glass and painted metal must stay
glossy enough to catch a highlight. `the_roughness_remap_offsets_before_it_floors`
computes both wrong orders (floor-first → `0.9`, offset-before-scale → saturates
to `1.0`) against the correct `0.54` rather than describing them in a comment.

## `normalStrength` scales xy, never z

`n.xy *= owNormalAmp` on a **tangent-space** normal, with no renormalise at the
site — the source renormalises later, after the detail layer has added its own
xy. Scaling z too would renormalise to something plausible and subtly flatten
every surface. Applied identically at all three sampling sites (`:299` triplanar,
`:357` base, `:369` the de-tile second sample), so it is one function here.

## `alphaMask` is a cutout

`alphaMask` is only a **define**: it routes `owAlbedo.a` into `diffuseColor.a`
(`shader.js:632-634`). The threshold is three's material property, and the only
library entry that sets `alphaMask: true` — `foliage` — sets `alphaTest: 0.45`
beside it (`library.js:328`, `:335`). three's `<alphatest_fragment>` is:

```glsl
if ( diffuseColor.a < alphaTest ) discard;
```

Strictly less-than, and a **discard** — `alphaToCoverage` is not set anywhere in
the source, so the smoothstep arm is dead. `FOLIAGE_ALPHA_TEST = 0.45` and
`axiom_mat_alpha_cut` is the predicate; the discard itself must be issued by the
composition step, because a helper cannot return one. See the open items below.

## What was deliberately not carried as a parameter

three multiplies `owAlbedo.rgb` by the material's `diffuse`, `owORM.g` by its
`roughness` and `owORM.b` by its `metalness`. `materials/index.js:203-207`
constructs **every** extended material at `color: 0xffffff, roughness: 1,
metalness: 1`, and no `three:` block in `library.js` overrides any of the three
— so all three are the identity for every surface that exists. They are
documented at the `channels` doc comment rather than carried as three
always-`1.0` parameters, which would be a widened API hosting nothing.
`material_opacity` **is** a parameter, because it genuinely varies
(`library.js:377` sets `0.22` on glass).

## Verification

- **CPU reference** written from the GLSL text, in `f32` throughout (the GPU's
  width), with `mix` and `clamp` written out to their GLSL/WGSL specs —
  `x*(1-a) + y*a` and `min(max(x,low),high)`, not the `x + (y-x)*a` or
  `f32::clamp` rearrangements, which are different functions.
- **Parity** on a real adapter over 6 fragment entry points × 16 input sets ×
  4 lanes, with `Rgba32Float` readback. Inputs hit both ends of every gate: wear
  mask exactly `0.0` and `1.0`, alpha mask exactly `0.0` and `1.0`, the roughness
  clamped at both the floor and `1.0`, the cutout on both sides, negative
  normals, a non-white tint. `the_samples_exercise_both_ends_of_every_gate`
  asserts that, so the parity cannot go vacuous.
- **Tolerance `2.4e-7`, derived from a measurement.** Worst absolute lane delta
  on a **Vulkan** adapter: `5.9604645e-8` — exactly `2^-24`, one f32 ulp at
  `1.0`, which is the single-rounding difference between the CPU's
  `roughness * scale + offset` and the GPU's contracted `fma` of the same. Every
  other lane came back bit-identical. `2.4e-7` is four ulp, i.e. **4.03x** the
  measurement, inside the 10x ceiling.
  `the_tolerance_is_not_looser_than_the_hardware_needs` re-measures every run and
  fails on drift in either direction.
- **Coverage 100.00%** — 408 regions, 38 functions, 276 lines, zero missed
  (`cargo llvm-cov --lib --branch`, MSVC nightly). The parity module is
  `--features offscreen` only, so the CPU reference is covered by tests that run
  in the default gate.

## Open items for the orchestrator / siblings

1. **`orm.r` (AO) has no `SurfaceOut` lane.** The source's AO path is
   `float ambientOcclusion = ( owORM.r - 1.0 ) * owAoAmt + 1.0;`
   (`shader.js:678`), applied to `reflectedLight.indirectDiffuse`. `SurfaceOut`
   is `base_color / roughness / metallic / normal / emission / opacity` — there
   is nowhere to put it, and `aoStrength` was not in this layer's assigned
   parameter set. `channels` therefore **drops `orm.r`**. Either `SurfaceOut`
   gains an `ao` lane (a contract change, like the `world_pos` one) or the
   weathering/cavity layers' AO output is unobservable. Flagged, not worked
   around.
2. **The normal is in the wrong space, and the source is not the one that is
   wrong.** `owNormalV` is a **view-space** normal (`normal = owNormalV`, and
   three's `normal` is view-space at that chunk). Axiom's `SurfaceOut.normal` is
   **tangent-space** — `scene_wgsl.rs:465-479` builds a cotangent frame from
   screen-space derivatives and transforms it. `axiom_mat_channels` passes the
   normal straight through, so whichever layer produces `nShade` must produce it
   in **tangent space**, not apply the source's `owP2V`.
3. **`scene_wgsl.rs:404-408` already discards at `albedo.a < 0.5`,** before the
   surface program runs, gated on `CAP_ALPHAMASK`. That is a *different* cutout
   at a *different* threshold from the source's `0.45`, and it would pre-empt
   this layer's. Composition needs to decide which one owns the cutout.
4. **`scene_wgsl.rs:469` replaces `surface.normal` entirely** when
   `CAP_NORMALMAP` is set (`select(surface.normal, textureSample(normal_tex …))`).
   A draw running the runtime material shader must have that capability **off**,
   or every layer's normal work is discarded.
5. **Twelve GPU harnesses.** `surface_program::parity::ParityGpu` is `pub(super)`
   to `surface_program`, so `material_shader` cannot reach it and this layer
   carries its own ~200-line copy. Every sibling will too. A shared
   `material_shader` parity harness should be hoisted once the fan-out closes;
   that is not something a layer may do mid-fan-out by editing a shared file.
6. **Every layer is dead code until composition.** `cargo clippy --lib` reports
   `dead_code` on all eleven landed layer files (22 items in this one) because
   nothing calls them yet, and CI runs clippy with `-D warnings`. This is the
   composition step's to resolve — an `#[allow(dead_code)]` in a layer file
   would be a suppression, not a fix.
7. **Two sibling files did not compile under `--features offscreen`** while this
   was written — `uv_mode.rs:1099` (`{worst:e?}` is an invalid format spec) and
   `macro_variation.rs:1453-1477` (`rendered` moved into an `FnMut` closure).
   The parity run was therefore done in a throwaway copy of the crate under the
   scratchpad rather than by editing anyone's file; it is reported, not touched.
