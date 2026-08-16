# 07 — Backend lowering

## Objective

Teach the two backends what a `Surface` means — **without generating any WGSL
yet**. This manifest lands: the backend-shaped program plan (stages, varyings,
bindings, parameter packing) in `modules/axiom-gpu-backend`; the capability
validation gate; and the **complete, shipping Canvas2D path**, which needs no
shader at all because it CPU-evaluates the surface through `axiom-field`.

By the end of this manifest, a field-authored surface **renders correctly on
Canvas2D** and is cleanly, reportedly unsupported on the GPU arm. `08` then makes
the GPU arm work.

## Architectural placement

Two existing **engine modules**: `axiom-gpu-backend` and `axiom-canvas2d-backend`.
Both are on `PLATFORM_FACING_MODULES` (`crates/xtask/src/hygiene.rs:64`), which is
where backend-specific knowledge is allowed to exist. No new package, no new
layer.

This manifest also absorbs the backend half of the rejected shader-IR stratum —
see `00-architecture-findings.md` §3C. The program plan is *inherently*
backend-shaped: it is defined by the WebGL2 16-attribute ceiling and the 40-float
instance stride, both of which live in this module.

## Existing code involved

| Path | Role |
|---|---|
| `modules/axiom-gpu-backend/src/scene_renderer.rs` | 2650 lines; `SCENE_WGSL` at `:29-413`; ≤10 pipelines, all built in constructors |
| `modules/axiom-gpu-backend/src/scene_renderer.rs:245-252` | *"16 vertex attributes … exactly the WebGL2 downlevel guarantee … a 17th would fail pipeline creation"* |
| `modules/axiom-gpu-backend/src/frame_packet_adapter.rs:23,32-49` | `INSTANCE_FLOATS = 40`; the sort + `chunk_by` batcher and why the `HashMap` was removed |
| `modules/axiom-gpu-backend/src/post_chain.rs:245-251` | `queue.write_buffer` orders against **submission**, not passes — the two-buffer fix |
| `modules/axiom-gpu-backend/src/post_chain.rs:462-466`, `surface_encode.rs:82-84` | the anti-variant doctrine, twice |
| `crates/axiom-host/src/frame_capability.rs` | `RenderCapability` (12 bits), `BackendCapabilityProfile`, `CapabilityDegradation::{Substitute, Drop}` |
| `crates/axiom-host/src/frame_submission_report.rs:31-57` | `degraded_features: Vec<FrameFeature>` — how a drop is reported |
| `modules/axiom-canvas2d-backend/src/raster_triangle.rs:14-19` | `RasterTriangle { vertices, object_id, color: [f32; 4] }` — **one flat colour per triangle** |
| `modules/axiom-canvas2d-backend/src/mesh_cache.rs:27-31` | keeps positions + colour, **discards normals and uv at upload** |
| `modules/axiom-canvas2d-backend/src/canvas_depth_cue.rs:35-43,104-160` | `face_normal_world`, `shade_triangle` |
| `tools/axiom-shot/tests/capability_parity.rs` | enforces that a capability is substituted or reported dropped |

## Files owned

| Path | Action |
|---|---|
| `modules/axiom-gpu-backend/src/surface_program/mod.rs` | **create** — and create the submodule directory first, so `08`/`10`/`11` can run in parallel without contending on `scene_renderer.rs` |
| `modules/axiom-gpu-backend/src/surface_program/plan.rs` | create — `SurfaceProgramPlan` |
| `modules/axiom-gpu-backend/src/surface_program/params.rs` | create — the uniform packing |
| `modules/axiom-gpu-backend/src/surface_program/capability.rs` | create — the validation gate |
| `modules/axiom-canvas2d-backend/src/surface_shading.rs` | create — the CPU path |
| `modules/axiom-canvas2d-backend/src/{frame_packet_raster.rs, raster_triangle.rs}` | modify |
| `crates/axiom-host/src/frame_capability.rs` | modify — add one capability bit |
| both `module.toml` + `Cargo.toml` | modify — `allowed_layers` gains `surface`, `field` |

## Dependencies on earlier manifests

**`06`** (`surface_program` must reach `FrameDrawItem`). Also **`04`**
(`SurfaceRequirements`) and **`03`** (the evaluator, which the Canvas2D path
executes).

Blocks `08`, `09`, `10`, `11`.

## Public API / data contracts

### The new capability bit

```rust
// crates/axiom-host/src/frame_capability.rs
ProceduralSurface = 1 << 12,
```

**Append only.** `capability_bits_are_the_gpu_shader_contract`
(`frame_capability.rs:324-344`) asserts every existing numeric value because the
WGSL hardcodes the same masks; renumbering breaks the cross-language contract.
There are 20 free bits.

Degradation policy: **`Drop`**, not `Substitute` — but see the Canvas2D section,
because Canvas2D does *not* drop it.

### `SurfaceProgramPlan` — the backend-shaped plan

```rust
pub(crate) struct SurfaceProgramPlan {
    program_id: u64,          // = Surface::digest().raw()
    stage_split: StageSplit,  // which channels are vertex-stage vs fragment-stage
    varyings: VaryingSet,     // what the vertex stage must pass down
    param_layout: ParamLayout,// slot -> byte offset in the surface UBO
    inputs: SurfaceInput,     // from SurfaceRequirements
}
```

**Stage assignment is a two-valued fact, not an IR.** `SurfaceChannel::Displacement`
is vertex-stage; the other six are fragment-stage. That is the entire "stage
scheduling" problem, which is precisely why a separate shader IR is not warranted.

### Parameter packing — a new uniform channel, and why it must be new

Surface parameters cannot ride the instance stream (`INSTANCE_FLOATS = 40`, zero
free lanes) and cannot become vertex attributes (16 of 16 used; the skinned
pipeline is already at the ceiling and silently drops emissive and specular
because of it). They therefore need a **per-surface uniform buffer**.

Design constraints, each with the code that imposes it:

* **One buffer per surface program, written at the preparation barrier**, plus a
  small per-frame write only for parameters an app actually animates. This
  preserves the property from `04` that a parameter change never touches the
  program identity.
* **Never reuse one buffer across draws in a single pass.** `post_chain.rs:245-251`
  documents that `queue.write_buffer` is ordered against *submission*, not against
  passes inside the encoder — so N writes to one buffer means every draw in that
  pass reads the *last* write. The engine already paid for this once and fixed it
  with two separate buffers. Use per-program buffers or dynamic offsets into one
  large buffer; do not use a single rewritten buffer.
* **Bind group 0 is already the material group.** A surface program's parameter
  buffer joins it, so `set_bind_group(0, …)` per batch is unchanged in count.
  Groups 1 (`lights`) and 2 (`shadow_sample`) must stay hoisted outside the batch
  loop (`scene_renderer.rs:1826-1827`); a per-surface *layout* would un-hoist them.
  **Therefore every surface program shares one `BindGroupLayout`** with a
  fixed-size parameter buffer (cap the parameter count, e.g. 32 slots = 512 B) —
  variable layouts are the thing to avoid.

### Capability validation — before lowering, not during

```rust
pub(crate) fn validate(reqs: &SurfaceRequirements, profile: BackendCapabilityProfile)
    -> Result<(), FrameFeature>;
```

Checked once per surface at bind/preparation time. A surface the backend cannot
support is reported through the existing
`FrameSubmissionReport::degraded_features` channel, never silently skipped —
`tools/axiom-shot/tests/capability_parity.rs` enforces exactly that.

Concrete checks: parameter count within the cap; node count within the shader
budget; `Displacement` bound only if the pipeline has a vertex stage that can
carry it (`10`).

### The Canvas2D path — a correct degrade, and a real render path

This is the part that justifies the whole CPU-evaluator decision.

`modules/axiom-canvas2d-backend` shades **per triangle**, not per pixel
(`raster_triangle.rs:14-19`), and discards normals and uv at upload
(`mesh_cache.rs:27-31`). It cannot execute a shader. But it *can* evaluate a
`Surface`'s channels on the CPU at each triangle's centroid, through
`FieldGraph::evaluate`:

```rust
// modules/axiom-canvas2d-backend/src/surface_shading.rs
pub(crate) fn shade_surface(
    surface: &Surface, centroid_object: Vec3, normal: Vec3, time: Seconds,
) -> ShadedChannels;  // base_color, emission, opacity, and the specular scalar
```

The result feeds the existing `RasterTriangle::color` and the existing
`shade_triangle` Lambert path. **`ProceduralSurface` is therefore `Substitute` on
Canvas2D, not `Drop`** — the substitution being "per-triangle instead of
per-pixel", which is the same fidelity relationship every other feature has on
that backend.

Two things this needs and does not have:
* **Object-space centroid.** `mesh_cache.rs` keeps world positions; the object
  transform is on the draw item. Compute the centroid in object space per triangle
  from the inverse world matrix, or pre-store object-space positions. Prefer the
  latter — it is one extra `Vec3` per vertex at upload, not per frame.

  > **CORRECTION — the premise is wrong.** `MeshGeometry::from_interleaved`
  > stores positions exactly as uploaded, i.e. already in **object space**; the
  > draw's `mvp` is what transforms them, and `world_y()` applies `world` to them
  > separately. So the object-space centroid is the mean of three numbers the
  > rasteriser already reads for projection, and **no matrix is inverted, per
  > frame or at all**. What was genuinely missing is the **uv** — `MeshGeometry`
  > now keeps interleaved floats 6..8 as one `[f32; 2]` per vertex at upload.
  > The normal comes from a new `face_normal_model`, kept separate from
  > `face_normal_world` whose normalize-after-rotate order must stay bit-exact.
* **The normal.** Already computed by `face_normal_world`; use it.

Roughness/metallic have no Canvas2D expression today (there is no view vector in
`shade_triangle`). Report them as degraded; do not fake them.

## Explicitly excluded

* **No WGSL generation.** That is `08`. Until then the GPU arm reports
  `ProceduralSurface` as dropped and renders the surface's *constant* base colour
  — which every `Surface` has, because an unbound channel is a constant.
* **No pipeline or program caching.** That is `09`.
* **No displacement.** That is `10`. `Displacement` bound ⇒ validation failure here.
* **No lighting-model wiring.** That is `11`.
* **No raw shader escape hatch yet** — its home is decided below, but it is
  implemented in `08`.

## The raw shader escape hatch — where it lives, and what it costs

Decided here, implemented in `08`, listed so no other manifest adopts it.

**Home: `modules/axiom-gpu-backend` only, as a backend-module type.** A `Surface`
never carries one, and `crates/axiom-surface` never mentions WGSL. That single
placement decision gives every guarantee the brief asks for:

| Question | Answer, and why it follows from the placement |
|---|---|
| Where does raw source belong? | `modules/axiom-gpu-backend`, beside the seven existing WGSL consts. Nowhere else in the engine may hold shader text. |
| How does it interact with capability validation? | It cannot be validated. It is admitted only against a `BackendCapabilityProfile` that includes a distinct `RawShaderProgram` bit, which **Canvas2D never has** — so a raw program is a hard `Drop` there, not a degrade. |
| What guarantees are lost? | CPU evaluation, Canvas2D rendering, canonicalisation, constant folding, structural diff, node-level diagnostics, and backend portability. All of them, by construction. |
| Does it participate in serialization/hashing? | **No.** It is not in `crates/axiom-surface`, so it cannot appear in a `Surface`'s canonical bytes. Its cache key is a hash of the source string, computed locally in the backend. |
| Does it bypass semantic optimisation? | Entirely. There is no semantic graph to optimise. |
| How is it labelled? | By its type name and its module. Any draw using one sets a `FrameFeature` in the submission report, so a frame that contains raw shader code says so in its own telemetry. |

**Do not implement it in this manifest.** It is documented here so that when it
is built it lands in the one place that cannot infect the semantic model.

## Determinism requirements

* The Canvas2D CPU path must be deterministic and replayable — it is
  `FieldGraph::evaluate`, which `03` guarantees bit-exactly.
* Capability validation is a pure function of `(requirements, profile)`.
* `program_id` is a content digest, stable across runs.

## Serialization requirements

None new. `SurfaceProgramPlan` is derived, never persisted.

## Testing requirements (100%)

* `validate` accepts a supportable surface and rejects each unsupportable case
  with the right `FrameFeature`.
* The new capability bit's numeric value is asserted, extending
  `capability_bits_are_the_gpu_shader_contract`.
* `BackendCapabilityProfile::canvas2d()` includes `ProceduralSurface`; the
  degradation policy for it is `Substitute`.
* Canvas2D: a surface whose base colour is a field of `Uv.x` renders a triangle
  whose colour equals the CPU evaluation at its centroid — asserted numerically,
  not visually.
* Canvas2D: a constant-only surface renders identically to today's `Material`
  path (the compatibility test).
* GPU arm: a field-bound surface reports `ProceduralSurface` degraded and renders
  the constant fallback.
* Parameter packing: layout offsets are stable and within the cap; over-cap is
  rejected.
* `tools/axiom-shot/tests/capability_parity.rs` still passes.

## Architecture tests

* `cargo xtask check-architecture` — both modules gain layer deps;
  `ModuleDependsOnLayerNotAllowed` guards the manifest/Cargo pairing.
* **Hygiene trap:** the substring `canvas` is banned outside the allowlist, and
  matching is on comment-stripped source *including string literals*. Both these
  modules **are** allowlisted (`PLATFORM_FACING_MODULES` contains `windowing`,
  `gpu-backend`, `canvas2d-backend`, `debug-overlay`, `audio`), so this is safe
  here — but any helper extracted into a non-allowlisted crate is not.

## Performance risks

* **Canvas2D CPU shading is per triangle, per frame.** That backend runs at
  240×135 by default with a tiered ladder, and its triangle counts are already
  the fill-rate concern. A 12-node graph evaluated per triangle is cheap relative
  to rasterising it, but **measure on burnt-rubber's Canvas2D arm before and
  after**, and cache per-`(surface, triangle)` results within a frame if the
  surface has no `Time` input (`SurfaceRequirements::inputs` says so exactly).
* **Bind group churn.** Every bind group in `axiom-gpu-backend` today is created
  in a `new()`. Keep it that way: one shared layout, buffers allocated at the
  preparation barrier.
* **Batch fragmentation** — see `06`. Verify the count did not move for
  surface-free content.

## Migration considerations

Adding a capability bit changes `FrameSubmissionReport` contents for frames that
use surfaces. Existing goldens are unaffected because existing draws carry
`surface_program = 0` and never trigger validation.

## Completion criteria

1. `SurfaceProgramPlan`, parameter layout, and capability validation exist in
   `modules/axiom-gpu-backend/src/surface_program/`.
2. `RenderCapability::ProceduralSurface = 1 << 12` exists, appended, asserted.
3. **Canvas2D renders a field-authored surface correctly**, per triangle.
4. The GPU arm reports it dropped and falls back to the constant colour.
5. Every existing app is pixel-identical.
6. Coverage 100/100/100; `cargo xtask check-architecture` exits 0; no dylint
   count rises; `capability_parity.rs` passes.

## Validation commands

```sh
cargo test -p axiom-gpu-backend -p axiom-canvas2d-backend -p axiom-host
cargo test --workspace
cargo xtask check-architecture
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
cargo run -p axiom-shot --features offscreen -- \
  --app burnt-rubber-straight --backend canvas2d --tick 0 --out screenshots/br-c2d.png
```

## Parallel safety

**Wave 7, width 1.** Must create the `surface_program/` submodule directory before
`08`, `10` and `11` can proceed in parallel.
