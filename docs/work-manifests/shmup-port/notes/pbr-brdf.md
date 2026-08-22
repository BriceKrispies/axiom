# G2 — `LightingModel::Physical`, the Cook-Torrance BRDF

**Slice:** the fourth `axiom_surface::LightingModel`, carrying three.js r180's
`MeshStandardMaterial` BRDF into the main pass's one lit shader. Closes **G2**
and the live half of **G3**.

## What landed

| Where | What |
|---|---|
| `crates/axiom-surface/src/lighting_model.rs` | `LightingModel::Physical = 3`, **appended**; `ALL` is `[_; 4]` |
| `crates/axiom-surface/src/channel.rs` | `Roughness`/`Metallic` documented live, under exactly one model |
| `modules/axiom-gpu-backend/src/surface_program/wgsl_template.rs` | `const AXIOM_LIGHT_PHYSICAL: u32 = 3u;` |
| `modules/axiom-gpu-backend/src/scene_wgsl.rs` | the `axiom_pbr_*` BRDF in the prefix; the material derivation, two accumulators and one `select` in `fs` |
| `modules/axiom-gpu-backend/src/surface_program/emit_lighting.rs` | the doc that used to say `metallic` is inert |
| `modules/axiom-gpu-backend/src/surface_program/parity_lighting.rs` | five new proofs on a real adapter |
| `modules/axiom-canvas2d-backend/src/frame_packet_raster.rs` | the Canvas 2D degradation, declared at the site |

## The design is the existing one, unchanged

`LightingModel` is a discriminant a generated program **states** (a nullary
`axiom_lighting_model()` returning a literal), and `fs` spends it on multipliers
and `select`s. The fourth model adds **one `select`** at the end of the light
loop and no pipeline: because the model is a per-program compile-time constant,
a Lambert program dead-strips the whole physical arm and a physical one
dead-strips the Blinn-Phong arm. Four models across N surfaces is still N
programs, not 4N.

Every legacy line in `fs` is byte-for-byte what it was. `Physical` does not
reshape `diffuse_gate` or `specular_gate`; it lets them produce whatever they
produce and then **replaces** the accumulated result. That is why
`lambert_specular_reproduces_the_pre_model_shader_pixel_for_pixel` and
`an_unlit_surface_is_unmoved_by_every_light_in_the_frame` pass **untouched** —
they were not edited, and they still reconstruct the pre-model `fs` by deleting
the same one gate `select` and three gate multiplies (`edits == 4`).

## Transcribed from the GLSL text

Sources, read as text:

- `ShaderChunk/common.glsl.js` — `BRDF_Lambert`, `F_Schlick`, `RECIPROCAL_PI`,
  `EPSILON`, `pow2`, `saturate`.
- `ShaderChunk/lights_physical_pars_fragment.glsl.js` — `V_GGX_SmithCorrelated`,
  `D_GGX`, `BRDF_GGX`.
- `ShaderChunk/lights_physical_fragment.glsl.js` — the `PhysicalMaterial` setup.
- `ShaderChunk/lights_pars_begin.glsl.js` — `getHemisphereLightIrradiance`.
- `ShaderLib/meshphysical.glsl.js` — `totalDiffuse + totalSpecular + emissive`.

Grouping preserved exactly. Specifically **not** rewritten:

- `0.5 / max( gv + gl, EPSILON )` and `RECIPROCAL_PI * a2 / pow2( denom )` are
  real divisions, not reciprocal-multiplies.
- `F * ( V * D )` keeps its parentheses.
- `f0 * ( 1.0 - fresnel ) + ( f90 * fresnel )` keeps its second pair.
- `saturate`, `clamp` and `mix` are written out (`min(max(x,0),1)`,
  `x*(1-a)+y*a`), because GLSL pins their factoring and WGSL does not.
- `F_Schlick` takes the Epic `exp2` variant that three actually ships, **not**
  the `pow(1 - dotVH, 5)` original sitting commented above it.

**Roughness remap: checked, not assumed.** `BRDF_GGX` does
`float alpha = pow2( roughness ); // UE4's roughness`, so alpha is roughness
*squared*, and the roughness it squares is
`min( max( roughnessFactor, 0.0525 ) + geometryRoughness, 1.0 )` — the `0.0525`
floor and the `geometryRoughness` specular-AA term from
`lights_physical_fragment` are both ported, the latter as
`max(abs(dpdx(geo_n)), abs(dpdy(geo_n)))` off the *non-perturbed* normal, as the
source specifies.

`metalnessFactor` is **not** clamped, because the source does not clamp it.
Roughness needs no clamp: the source's own `max`/`min` bound it on both sides.

## Deliberate divergences

1. **No indirect specular.** three's `RE_IndirectSpecular_Physical` needs an
   environment map; without one its `radiance` and `iblIrradiance` are both
   `vec3(0.0)` and it contributes nothing, which is exactly what this pass
   contributes. An IBL probe is its own capability, not a line here.
2. **Radiometric scale.** The physical model carries the source's `1/PI` on
   every diffuse term and the other three models do not, so a physical surface
   is ~PI times dimmer under the same light. That is the source's unit system —
   light intensities ported from a three.js scene are already in it — and it is
   documented on the variant. Mixing models in one frame mixes two unit systems.
3. **Canvas 2D degrades to Lambert**, declared at the site. That arm is a
   per-triangle centroid sampler with no view vector; G17's stated policy is
   legibility, not parity, and `RenderCapability::Specular` is the mechanism
   that already says so.

## What happened to the legacy specular lane

`in.specular` — the instance-stream lane derived from the *legacy*
`Material::roughness`, riding the emissive vec4's fourth component — is
**untouched and still drives the other three models**. `Physical` never consults
it: `specular_gate` is `model == LAMBERT_SPECULAR`, so `gloss` is already an
exact zero for it, and the physical sum replaces the result anyway. Pinned by
`the_legacy_specular_lane_does_not_reach_the_physical_model`, which sweeps the
lane `0 → 0.5 → 1` and gets three bit-identical pixels from a physical surface
and three different ones from a `LambertSpecular` surface on the same rig.

## What was measured, and the tolerances

All on a real Vulkan adapter, `--features offscreen`.

| Proof | Measured |
|---|---|
| `the_physical_model_renders_its_documented_result` | GPU vs a **hand-derived closed form** (on the unit rig `V` collapses to exactly `0.25` and `D` to `RECIPROCAL_PI / a2`): worst delta **< 1e-6**, budget `1e-4` |
| `the_physical_brdf_matches_a_transcription_of_the_source_glsl` | GPU vs an `f64` re-transcription of the GLSL, off-axis, 3 lights of both kinds, 5 (roughness, metalness) pairs: worst delta **2.97e-4**, budget `1e-3` = **3.4x**, asserted in the test so it cannot rot |

The closed form is a *different route to the same number* — algebra collapsed by
hand rather than a second reading of the same text — which is the mitigation for
this port's "one person wrote both transcriptions" failure mode. The two
instruments agree.

The 2.97e-4 gap is not transcription: `D_GGX`'s denominator
`dotNH² · (a2 - 1) + 1` is a catastrophic cancellation when `a2` is small and
`dotNH` near 1, so an `f32` shader and an `f64` reference part company there by
far more than either one's own epsilon. The low-roughness case is kept in the
sweep for exactly that reason.

Behavioural proofs, also on hardware:

- `a_ggx_lobes_width_is_the_authored_roughness_not_a_fixed_exponent` — the
  Blinn-Phong control measures `cos^48(cos 30°)` to 1e-4 (so the control is the
  real lobe), and the GGX lobe **brackets** it: roughness 0.1 keeps <10% of what
  `cos^48` keeps, roughness 0.6 keeps >10x. A rename cannot land on both sides
  of a fixed exponent.
- `metallic_is_inert_under_every_model_but_physical` — bit-identical across
  `metallic ∈ {0, 0.5, 1}` for the three legacy models, three different pixels
  under `Physical`.
- The metal/dielectric split: a dielectric's highlight is colourless to 1e-4
  across R and B; a metal's blue lane exceeds 2.5x its red, carrying the base
  colour's hue.

## Goldens

**None moved, and none needed to.** `crates/axiom-surface/tests/surface_golden.rs`
passes unedited: the lighting code is a `u16` *value* in the record, not a
length, so appending a fourth variant shifts no byte and moves no digest. (The
goldens in the working tree are already re-recorded — by the `SurfaceKind`
sibling, for its own header change, with its reason beside it. That is not this
slice.)

## Coverage

`axiom-surface` and `axiom-gpu-backend` are both at 100.00% regions / lines /
functions after the change (`cargo llvm-cov --branch -p …`, nightly MSVC). The
new spine code is one enum variant, one array element and WGSL string data; the
new Rust is all offscreen-gated test code.

## Notes for the orchestrator

- The workspace does **not** compile in the shared checkout right now:
  `modules/axiom-gpu-backend/src/bloom_pyramid/mod.rs` declares `chain`, `wgsl`
  and `parity` modules whose files do not exist, and `cascade/adapter_proof.rs`
  passes a `depth_slice` field this `wgpu` version's
  `RenderPassColorAttachment` does not have. Both are sibling slices mid-flight,
  neither is mine. **Everything above was verified in a clean worktree at
  `HEAD` with only this slice applied**; the shared checkout carries the same
  edits. To reproduce once the siblings land:

  ```sh
  CARGO_TARGET_DIR=…/pbr cargo test -p axiom-gpu-backend --features offscreen
  CARGO_TARGET_DIR=…/pbr cargo test -p axiom-surface -p axiom-canvas2d-backend -p axiom-render
  ```

  Run the two GPU-touching packages **one at a time**: two offscreen test
  binaries contending for the same adapter concurrently is what a combined
  `-p a -p b` invocation does, and it fails spuriously.
- I edited `surface_program/mod.rs` despite the brief's "do not edit mod.rs":
  one test there asserted a literal `vec![3, 3, 3]` program count per model,
  which a fourth model necessarily changes. It is now written against
  `LightingModel::ALL.len()`, so the *next* model needs no edit at all.
- Naming: the variant is `Physical`, after three's own `PhysicalMaterial` /
  `lights_physical` chunks. `10-convergence-plan.md` item 6 calls it
  "Cook-Torrance"; the doc comment says both.
- Still open from G2's neighbourhood: **G8** (no MRT) blocks the split-sum /
  IBL work that would give this model an indirect specular, and **G11** (no
  surface-program lane on skinned draws) means soldiers and the viewmodel still
  cannot select `Physical` at all.
