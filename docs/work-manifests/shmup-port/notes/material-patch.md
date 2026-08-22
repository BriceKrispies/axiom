# `render/materialpatch.js` (321 lines) — the indirect-lighting composition

Slice notes for the render fan-out. Read the verdict first; the port is the
smaller half of this file.

Code, all new, nothing else in the repo touched:

| file | what |
|---|---|
| `modules/axiom-gpu-backend/src/indirect_lighting.rs` | the CPU reference + the WGSL text |
| `modules/axiom-gpu-backend/src/indirect_lighting/tests.rs` | the second transcription + the property assertions |
| `modules/axiom-gpu-backend/src/indirect_lighting/adapter_proof.rs` | CPU↔GPU parity, `#[cfg(all(test, feature = "offscreen"))]` |

Wiring the orchestrator must apply: **one line**,
`modules/axiom-gpu-backend/src/lib.rs: mod indirect_lighting;`.

---

## 1. What the file actually is

The name and the mechanism both mislead. `MaterialPatcher` is
`onBeforeCompile` surgery, so it looks like the usual Three.js scaffolding for
getting per-material lanes into a velocity pass, a depth prepass or a G-buffer.
It has nothing to do with any of those. `prepass.js` and the octahedral packing
in `glsl.js` own that, and they are already ported as
`modules/axiom-gpu-backend/src/gbuffer.rs`.

Strip the scaffolding and `materialpatch.js` is **one lighting decision**,
expressed as five GLSL functions and three injection sites:

- AO belongs on **indirect** light, never on the sun — plus one deliberate
  exception, a 0.35 fraction of it on the direct term as micro-shadowing;
- a screen-space contact ray belongs on **the sun only**, picked out of the
  light loop by `dot( lightDirView, owSunDirView ) < 0.999`;
- a screen-space reflection **replaces** the image-based specular by
  confidence, rather than adding on top of it;
- and a two-band hemispheric fill — cool sky above, warm street below, *gated
  by the normal* and by a coarse interior-volume test — is what stops shadowed
  geometry collapsing to black once AO has eaten the indirect term.

The last of those is the largest part of the file by line count and by comment
weight, and it is not advertised anywhere in the file's header.

## 2. Verdict, category by category

### Category 1 — already solved structurally by Axiom's splice. Dropped.

| source mechanism | where Axiom already has it |
|---|---|
| `onBeforeCompile` + three `String.replace` calls on `#include <lights_pars_begin>` / `<lights_fragment_begin>` / `<lights_fragment_maps>` | `scene_wgsl.rs` is a prefix, a program-shaped hole and a suffix, concatenated by `surface_program::wgsl_template::scene_shader`. Axiom owns the whole text; there is nothing to inject *into* because there is no chunk system. |
| `PATCH_VERSION = 9` folded into `customProgramCacheKey` | `surface_program/cache.rs:16-27` — the key **is** `axiom_surface::Surface::digest`, a structural content hash. A hand-bumped version integer is what you need when the cache key cannot see the shader text. |
| `_patched` (a `WeakSet` re-entrancy guard) + `prevHook` chaining | `cache.rs:164-178` dedups by digest before compiling. Patching twice is not expressible. |
| one shared `uniforms` object so a single write updates every material | `scene_wgsl.rs:93` — the lighting uniform is group 1, written once per frame, set once per pass. |
| `setScreenSize(w, h)` → `owScreenTexel` | Not carried. A screen-space consumer reads `textureDimensions` of the buffer it samples, which cannot go stale against a resize the way `setScreenSize` can. This is the same reasoning that retired `core/prewarm.js`. |
| `owWP = cameraPosition + geometryPosition * mat3( viewMatrix )` | `scene_wgsl.rs:321` — the fragment stage already interpolates `world_pos` for the fog and specular terms. The source's line is a conversion *back out of* three's view-space fragment stage; there is no term to convert. |
| `owWN = inverseTransformDirection( normal, viewMatrix )` | Same: `N` in the suffix is already world-space. |

That is roughly 90 of the file's 321 lines gone by construction.

### Category 3 — Three-specific, no analogue. Named and dropped.

`MaterialPatcher.isLit` — five `m.isMeshStandardMaterial` duck tests. Axiom
states participation in lighting as a **value**: `axiom_surface::LightingModel`,
read by `axiom_lighting_model()` and turned into `gathers`/`diffuse_gate`/
`specular_gate` in the suffix. "Does this material run the lighting pipeline" is
a discriminant the program already carries. `dispose()` and
`material.needsUpdate = true` go with it.

### Category 2 — real capability Axiom lacks. Ported.

Everything else, and it is genuinely absent: the main pass has **no AO input,
no contact-shadow input, no reflection input and no fill bands.** Its entire
indirect term is `hemi * ambient_shade` (`scene_wgsl.rs:665-669`).

| source | port | signature |
|---|---|---|
| `owSampleAO()` | `sample_ao` | `(feat_x, ao_texel_r, ao_strength_x) -> f32` |
| `owContactShadow( vec3 )` | `contact_shadow` | `(feat_y, light_dot_sun_view, contact_texel_r) -> f32` |
| `owMultiBounce( float, vec3 )` | `multi_bounce` | `(ao, albedo) -> [f32; 3]` |
| `owSpecularOcclusion( float, float )` | `specular_occlusion` | `(ao, rough) -> f32` |
| `owSunBounce( vec3 )` | `sun_bounce` | `(world_normal, sun_dir_world) -> f32` |
| `owInteriorGate( vec3, float )` | `interior_gate` | `(world_pos, ao, &IndirectUniforms) -> f32` |
| the two `directLight.color *=` lines | `direct_light` | `(color, receive_shadow, sun_shadow, contact, ao, ao_strength_x) -> [f32; 3]` |
| the whole `lights_fragment_maps` body | `indirect` | `(IndirectIn, &IndirectUniforms) -> IndirectOut` |
| the `USE_ENVMAP` SSR block | `ssr_blend` | `(radiance, feat_z, roughness, ssr) -> [f32; 3]` |

WGSL entry points, same shapes, in `INDIRECT_LIGHTING_WGSL`:
`axiom_indirect_{sample_ao, contact_shadow, multi_bounce, specular_occlusion,
sun_bounce, interior_gate, direct_light, apply, ssr_blend}`, over one
`struct AxiomIndirectU` whose group/binding the main pass assigns.

## 3. Where each term lands in Axiom's suffix

This is the seam the orchestrator needs when it wires the pass.

| three's name | Axiom's peer | state |
|---|---|---|
| `irradiance` | `hemi * ambient_shade` (`scene_wgsl.rs:665-669`), which `scene_wgsl.rs:732-738` already records as being three's `getHemisphereLightIrradiance` expression exactly | **live** — the AO multiply, both fill bands and the sun bounce all land here today |
| `iblIrradiance` | none — no PMREM | zero |
| `radiance` | none — `scene_wgsl.rs:818-823` states `indirectSpecular` is an exact zero without an environment map | zero |
| `directLight.color` | `lt.col.rgb * lt.col.w * atten` inside the light loop | **live** |

So the two-band fill, the interior gate, the multi-bounce AO and the
micro-shadow are all wireable **now**. The image-based half is ported and
multiplies nothing until a probe exists.

## 4. Deferrals, each with its expiry check

Per the brief's rule that a deferral without an expiry check becomes a defect:

1. **The AO and contact texels.** `sample_ao`/`contact_shadow` take the sampled
   value, so they are complete — nobody *produces* the value. `render/gtao.js`
   (324) and `render/contact.js` are unported.
   **Expires when** a `gtao` module lands in `axiom-gpu-backend`; at that point
   `scene_renderer.rs` must bind its output and `scene_wgsl.rs`'s suffix must
   call `axiom_indirect_sample_ao`.
2. **`iblIrradiance` / `radiance` / SSR.** Ported, correct, currently
   multiplying zero.
   **Expires when** `render/probe.js` (306) lands and the main pass gains a
   PMREM binding. Do **not** delete these lanes meanwhile — a zero input is not
   a reason to drop a term, and `ssr_blend` on a zero `radiance` degenerates to
   pure addition, which is exactly the double-counting the source wrote this
   term to avoid.
3. **The room volumes.** `interior_gate` is the *test*; the *builder* is
   `RenderSystem._updateRooms` (`render/index.js:1167-1215`), which recovers the
   level→world yaw from two transformed points and publishes one
   `(cx, cz, hx, hz)` + `(y0, y1)` pair per enterable, un-collapsed,
   un-ruined building.
   **Expires when** `render/index.js` is ported. Until then `indirect[2]` is 0
   and the gate degrades to its AO arm — which is what the original does before
   the world appears, not a stub.

## 5. Cross-slice findings

### 5.1 `crate::cascade`'s `mix` is not the GLSL spec's `mix`

`cascade/shading.rs:27` and `cascade/adapter_proof.rs:85` both define

```rust
fn mix(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }
```

under a doc comment reading "GLSL `mix(a, b, t)` — written out, never a builtin
whose factoring is unspecified". The GLSL ES 3.0 spec §8.3 defines
`mix(x, y, a)` as **`x⋅(1−a)+y⋅a`**. The two are algebraically equal and
numerically different, and this crate's other five hand-written `mix`es
(`material_shader/{masks, tint_wear, macro_variation, pom}.rs`) all use the spec
form.

It matters more than a ULP would normally: the CSM slice wrote the *same* wrong
form on **both** sides — the Rust reference and the WGSL it is proved
against — so its adapter proof passes while comparing one misreading against
itself. That is precisely the failure mode `_wiring-queue.md` records for
`sky/volumetrics`, arriving a second time.

`bloom_pyramid/reference.rs:129` uses the same form; whether that is wrong
depends on whether its source calls GLSL `mix` or `MathUtils.lerp` (which is
`(1-t)x + ty` and therefore agrees with the spec form anyway). Not audited here.

**Not fixed by this slice** — `cascade/` is another agent's file and this wave
forbids touching one. Handed to the orchestrator.

### 5.2 The `0.999` sun test is needed *more* in Axiom, not less

`cascade/adapter_proof.rs:22-24` drops
`dot( lightDirView, owSunDirView ) < 0.999` on the grounds that "Axiom has one
shadow-casting directional light, so there is no loop to pick the sun out of."
That is true of the **shadow map** and false of the **light loop**:
`scene_wgsl.rs:745-758` iterates up to 16 lights and applies its one
`shadow_factor` to *every* directional among them. A second directional (a
muzzle flash modelled as one, a fill key) would therefore receive the sun's
cascade *and*, without this test, the sun's contact ray.

`indirect_lighting::contact_shadow` keeps the test. The two slices should be
reconciled when either is wired — either the loop learns which light is the
sun, or both drop it and Axiom's second directional is knowingly wrong.

### 5.3 `material_shader`'s `aoStrength` has found its call site

`material_shader/compose.rs:89-94` records that
`axiom_masks_ambient_occlusion` — `( owORM.r - 1.0 ) * owAoAmt + 1.0`, from
`shader.js:678` — is defined and uncalled, because the `masks` agent found it
belongs "at the lighting stage, where the engine applies AO to indirect
diffuse". **That stage is this module.** The material's own ORM occlusion and
this module's screen-space AO both multiply the same indirect term; the source
applies them at the same place (`shader.js` writes `owORM.r` into three's
`aomap_fragment`, which runs inside `lights_fragment_maps` immediately before
this file's injection). Composing them is a wiring decision for whoever lands
the suffix change, and it needs `SurfaceOut` to grow an `ao` lane —
`10-convergence-plan.md` §2 already lists that as a gap.

## 6. Transcription notes

Written from the GLSL text in `EXTRA_PARS` and the three injected bodies, never
from the Rust. What was specifically hunted for and preserved:

- **`owSunBounce`'s `/ 1.12` is a division.** Five of the ten `sky/` defects
  were a division rewritten as a reciprocal multiply. It stays a division on
  both sides.
- **`owMultiBounce`'s Horner nesting** `ao * ( ao * ( ao * a + b ) + c )`.
- **`owInteriorGate`'s `min( min( A, B ), min( C, D ) )`** pairing, not a
  four-way chain.
- **The fill add is one vector sum scaled once:**
  `( skyFill * skyG + groundFill * gndG * indoor ) * ( fillAo * fillGain.x )`.
- **The sun-bounce add's scalar chain is left-to-right:**
  `sunBounce * fillGain.y * fillAo * indoor`.
- **The direct light takes two successive multiplies**, shadow then
  micro-shadow. `(c * s) * m` is not `c * (s * m)`. `direct_light` therefore
  returns the multiplied colour rather than a single gain, which is the only
  way to keep the order.
- **The `1e-4` in `owSunBounce` lands on all three components**
  (`+ vec3( 1e-4 )`), including the constant `0.28` — not just the two sun
  lanes.
- **`smoothstep( 0.62, 0.14, roughness )` has `e0 > e1` on purpose.** Written
  out, because both GLSL and WGSL leave the builtin indeterminate there.
- **`mix`, `clamp`, `smoothstep` and `normalize` are written out on both
  sides**, in the spec's factoring. `normalize` is `v / sqrt(dot(v, v))` rather
  than an `inverseSqrt` multiply — neither spec pins the builtin, so pinning
  both sides to one factoring is the property parity can assert.

### The `owAo < 1.0` guard

Reproduced as a value select over the whole block, not argued away. It *is* an
exact identity at `ao == 1` (the multi-bounce fit evaluates to ~0.9998 and the
source's own `clamp(…, vec3(ao), vec3(1.0))` pulls it to exactly 1; the
specular occlusion is `pow(1, k) = 1`), but "happens to be" is not a
transcription, and `mix(1, 1, s)` collapsing to exactly 1 is a property of the
rounding rather than of the algebra. Reproducing the guard costs one index.
`multi_bounce_is_the_exact_identity_at_full_visibility` pins the claim anyway.

### Two "uniforms" that are constants

Grepped across all of `C:/dev/Claude-of-Duty/src`:

- **`owAoStrength` is never written** after the constructor. Every frame of the
  original runs at `(1.0, 0.6)`, so the micro-shadow fraction the direct light
  receives is a fixed `1.0 * 0.35`.
- **`owFillDir` is never written at all.** Always
  `(-0.95, 0.85, -0.05, 0.7)`.

Only `owFillGain`, `owSkyFill`, `owGroundFill`, `owIndirect`, `owRoomXf`,
`owRooms`/`owRoomsY` and `owFeat` are driven per frame (`index.js:1093-1215`,
`1338-1370`). Both are still carried as parameters, because the source carries
them and a frame graph may yet drive them; their shipped values are pinned by
`the_never_written_uniforms_are_the_values_every_frame_of_the_original_runs`
so the fact cannot rot.

`owFeat.w` ("ao power") is declared `1.0` and **read by nothing**. Carried,
named, unread — dead computation in the source is still part of the source.

## 7. Tolerances — UNVERIFIED

This wave does not build, so nothing here has touched hardware. The adapter
proof declares two constants, both expectations derived from what this crate has
already measured on the same device:

| constant | value | basis |
|---|---|---|
| `TOLERANCE` | `2.0e-6` | the middle of this crate's measured band for plain f32 chains — 4e-7 (`material_shader::masks`, one ULP) to 7.6e-6 (`material_shader::uv_mode`, an `fma` contraction). These functions contain several `a*b + c` shapes a driver may contract. |
| `POW_TOLERANCE` | `3.0e-5` | exactly `surface_program::parity_transcendental::POW_TOLERANCE`, which **was** measured for `pow` on this device. Applies to `specular_occlusion` and to the `radiance` lane downstream of it. |

**The orchestrator must run this and replace both with the measured worst delta
plus a margin.** If the real delta is more than 10x under a constant, tighten
it — a tolerance looser than the hardware needs hides the next regression. The
one place the two sides could genuinely diverge by more than rounding is
`sun_bounce`'s `normalize`, which is why it is written out on both sides rather
than left to the builtins.

## 8. What a future agent should not redo

- Do not re-add a screen-texel uniform. The resize hazard it carries is real
  (`setScreenSize` is called from one place in `index.js` and nothing enforces
  it) and `textureDimensions` removes it.
- Do not "simplify" `mix` to `x + (y - x) * a`.
  `the_wgsl_keeps_the_sources_own_factoring` and
  `the_two_lerp_factorings_are_not_the_same_number` exist to make that diff
  carry its own evidence.
- Do not fold `direct_light` into a single gain.
- Do not delete the `iblIrradiance`/`radiance`/SSR lanes because they are
  currently zero. See §4.2.
