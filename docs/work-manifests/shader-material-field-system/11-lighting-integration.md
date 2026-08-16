# 11 — Lighting integration

## Objective

Let a surface participate differently in lighting **without becoming arbitrary
shader code**, by wiring the three-variant `LightingModel` discriminant from `04`
into the generated program — and, in doing so, finally give
`RenderPipelineKind::UNLIT` something behind it.

## The responsibility split, stated first

The brief asks which lighting responsibilities belong where. The repository
already answers most of it; this manifest changes exactly one line of that table.

| Responsibility | Owner | Evidence |
|---|---|---|
| Lights as scene/frame data | `crates/axiom-host` — `FrameLight`, `MAX_LIGHTS = 16`, packed into a 608-byte UBO | `frame_packet.rs`, `scene_renderer.rs:66` |
| Ambient / sky / fog / bloom / grade as frame look | `crates/axiom-host` — `FrameRenderLook` and its five payloads, each with a **CPU reference definition** the WGSL mirrors | `frame_ambient.rs`, `frame_sky.rs`, `frame_depth_fog.rs`, `frame_bloom.rs`, `frame_postprocess.rs` |
| Shadowing | `modules/axiom-gpu-backend` — one directional cascade, 5×5 PCF | `scene_renderer.rs:270-288` |
| The BRDF | `modules/axiom-gpu-backend` — Lambert + Blinn-Phong, `SPECULAR_POWER = 48.0` global | `scene_renderer.rs:116` |
| **Surface response** | **`crates/axiom-surface`** — the channels, and `LightingModel` | this work |
| Generic field expressions | `crates/axiom-field` — and they know nothing about lights | `01` |

**Nothing about lights moves.** The surface layer does not name a light, a
shadow, an ambient term or a camera. It names only *how the surface responds*,
through a three-valued discriminant and the channel values feeding the existing
maths.

## Why a discriminant is the right extensibility point

The temptation is a programmable lighting hook — let a surface supply its own
shading function. Reject it, for three reasons drawn from the code:

1. **There is exactly one lit shader and an explicit anti-variant doctrine**
   (`post_chain.rs:462`, `surface_encode.rs:82`). Programmable lighting multiplies
   variants by the one axis that is currently free — the 12 capability bits are a
   *runtime uniform*, not a variant dimension, and a programmable BRDF would make
   them one.
2. **`docs/engine-datafication.md:234-240` prescribes the alternative in so many
   words**: *"Parameterize the fixed model by data and select from a small closed
   set of variants by discriminant."*
3. **The seam already exists and is already unused.**
   `modules/axiom-render/src/render_pipeline_kind.rs` declares
   `BASIC_LIT = 1` and `UNLIT = 2`, `RenderApi::build_commands` run-length-encodes
   `SetPipeline` per switch — and the value **dies at the `FramePacket`
   boundary**; no backend has ever seen it. The engine already decided what shape
   this extensibility takes and then never finished the wiring. Finish it.

## Architectural placement

**Layer `surface`** (the discriminant, already landed in `04`), **layer `host`**
(nothing changes), **module `gpu-backend`** (the emission), **module
`canvas2d-backend`** (the degrade), **module `axiom-render`** (the seam it
already emits).

## Existing code involved

| Path | Role |
|---|---|
| `modules/axiom-gpu-backend/src/scene_renderer.rs:296-412` | `fs` — the whole lighting body |
| `scene_renderer.rs:358` | hemisphere ambient |
| `scene_renderer.rs:400` | `let emitted = lit + in.emissive;` — emissive added post-lighting |
| `scene_renderer.rs:409` | fog applied last |
| `scene_renderer.rs:116` | `SPECULAR_POWER: f32 = 48.0` |
| `modules/axiom-render-pipeline/src/render_pipeline_api.rs:413` | `specular = 1.0 - roughness` |
| `modules/axiom-render/src/render_pipeline_kind.rs` | `BASIC_LIT`, `UNLIT` — emitted, unwired |
| `crates/axiom-host/src/frame_capability.rs` | `RenderCapability::Specular` |
| `modules/axiom-canvas2d-backend/src/canvas_depth_cue.rs:104-160` | `shade_triangle` — Lambert only, no view vector |

## Files owned

| Path | Action |
|---|---|
| `modules/axiom-gpu-backend/src/surface_program/emit_lighting.rs` | create |
| `modules/axiom-gpu-backend/src/surface_program/plan.rs` | modify |
| `modules/axiom-canvas2d-backend/src/surface_shading.rs` | modify |
| `modules/axiom-render/src/render_api.rs` | modify — carry the pipeline kind past the packet boundary |
| `crates/axiom-host/src/frame_packet.rs` | modify **only if** the kind must ride the packet — prefer deriving it from `surface_program` in the backend |

## Dependencies on earlier manifests

**`08`.** Parallel with `10` if both stay inside `surface_program/`.

## Public API / data contracts

### The three models, and exactly what each emits

| `LightingModel` | Emission |
|---|---|
| `Unlit = 0` | `out = base_color.rgb + emission`. No lights, no ambient, no shadow, no specular. Fog still applies (it is a depth effect, not a lighting one) — decide this explicitly and write it down. |
| `Lambert = 1` | ambient + Σ N·L, no specular term. The matte path. |
| `LambertSpecular = 2` | today's behaviour: ambient + Σ N·L + Blinn-Phong gated on `CAP_SPECULAR`. **The default, so nothing changes for existing content.** |

`Metallic` from `04` **feeds no BRDF in this manifest.** It is carried, packed,
and available — and it changes no pixel. That is deliberate: `SPEC-11` says
*"Resist PBR scope creep"*, and adding a metallic term means adding a Fresnel
term and an environment term, which is a different project. Document `metallic`
as reserved-and-inert, and **do not** ship it as if it worked. (Note the
cautionary precedent: `roughness` sat inert long enough that three separate
app-side docs still claim it is dead when it has been live since it became
`1.0 - roughness` specular strength.)

### Emission stays a `select`, not a branch and not a variant

The three models compile into the **same** program, selected by a `select()` on a
value in the surface parameter buffer — exactly how the 12 capability bits
already work (`scene_renderer.rs:73-78`, every gate a `select` with both arms
evaluated to keep control flow uniform for derivative-dependent texture ops).

**This is the important design decision in the manifest.** It means adding
`LightingModel` costs **zero** additional pipelines. Three models × N surfaces
stays N programs, not 3N. If a future model is too expensive to evaluate
unconditionally, *that* is the moment to consider a variant — and to pay the
doctrine's cost knowingly.

### `RenderPipelineKind`, finally connected

`Surface::lighting == Unlit` maps to `RenderPipelineKind::UNLIT`. Carry the
mapping through so the render module's existing run-length-encoded `SetPipeline`
stream stops being a lie. **Prefer deriving it in the backend from the surface**
rather than widening `FrameDrawItem` again — `06` already added one lane and
`frame_packet.rs` exists to stay primitive-only.

### Canvas2D

`Unlit` and `Lambert` are both **exactly expressible** — `shade_triangle` is
already `ambient + brightness * light_color` with no view vector, so `Lambert` is
its native model and `Unlit` is skipping the shade. Implement both properly.

`LambertSpecular` degrades to `Lambert` and is reported. Note that
`RenderCapability::Specular` is already absent from
`BackendCapabilityProfile::canvas2d()`, so the reporting channel exists.

## Explicitly excluded

* **No PBR, no GGX, no Fresnel, no image-based lighting, no environment maps.**
* **No per-surface light lists, no light types, no shadow parameters.** Lights are
  frame data and stay frame data.
* **No programmable BRDF hook.** That is the raw escape hatch (`07`), and it
  carries all of the escape hatch's losses.
* **No change to `MAX_LIGHTS = 16`, the 608-byte lights UBO, the PCF kernel, the
  ambient model, the fog, or the tonemap.**
* **No fourth lighting model.** Three is the closed set. A fourth needs a written
  justification amending `04`.

## Determinism requirements

Lighting selection is a value in a uniform, so it is deterministic by
construction. `Unlit` output must be exactly `base_color.rgb + emission` — assert
bit-exactly on the CPU path and within `08`'s tolerance on the GPU path.

## Serialization requirements

`LightingModel` is already in `Surface`'s canonical bytes from `04`. Changing it
changes the digest — correctly, because it changes the emitted program.

## Testing requirements (100%)

* Each of the three models renders its documented result for a known light rig —
  asserted numerically, not visually.
* `Unlit` is unaffected by moving or removing every light.
* `LambertSpecular` reproduces today's output **pixel-identically** for an
  existing app (this is the compatibility test that lets the default be safe).
* Canvas2D renders `Unlit` and `Lambert` correctly and reports
  `LambertSpecular` degraded.
* `RenderPipelineKind::UNLIT` is emitted for an unlit surface and reaches the
  backend — the first test in the repo's history to assert that.
* Cache size is **unchanged** across the three models on the same graph (the
  no-new-variants test).
* `metallic` set to any value changes no pixel (the reserved-and-inert test).

## Architecture tests

`cargo xtask check-architecture`; `capability_bits_are_the_gpu_shader_contract`
still passes; `tools/axiom-shot/tests/capability_parity.rs` still passes.

## Performance risks

* **Three models in one shader means all three arms are evaluated.** That is the
  same trade the 12 capability bits already make and the reason there is no
  stutter today. Measure the fragment cost on the Canvas2D-tier device profile
  before accepting it; if `Unlit` surfaces are common and the wasted Σ N·L is
  measurable, the *correct* next step is a second pipeline for `Unlit` only —
  compiled at the barrier like everything else, and a deliberate, documented
  variant rather than an accidental one.
* No new uniform buffer: the model selector is one lane in the existing surface
  parameter buffer.

## Migration considerations

`LambertSpecular` is the default, so every existing surface and every
`surface_program == 0` draw is unchanged. The pixel-identity test is the proof.

## Completion criteria

1. Three lighting models emit correctly on the GPU arm, selected by a uniform.
2. `Unlit` and `Lambert` render correctly on Canvas2D; `LambertSpecular` degrades
   and reports.
3. `RenderPipelineKind::UNLIT` reaches a backend for the first time.
4. Program cache size is independent of lighting model.
5. Existing content is pixel-identical.
6. `metallic` is documented as reserved-and-inert and proven to change nothing.
7. Coverage 100/100/100; `cargo xtask check-architecture` exits 0; no dylint count
   rises.

## Validation commands

```sh
cargo test -p axiom-gpu-backend --features offscreen
cargo test -p axiom-canvas2d-backend -p axiom-render
cargo test --workspace
cargo xtask check-architecture
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```

## Parallel safety

**Wave 8.** Parallel with `08`/`10` only if confined to
`surface_program/emit_lighting.rs`.
