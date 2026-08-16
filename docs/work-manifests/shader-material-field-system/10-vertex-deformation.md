# 10 — Vertex deformation

## Objective

Make `SurfaceChannel::Displacement` real on the GPU vertex stage, and make the
same field drive bake-time deformation, so wind, ripple, bend, twist and
squash/stretch are **authored graphs, not engine features**.

## The architectural answer, stated first

The brief asks whether vertex deformation is a general field consumer, shader
functionality, material functionality, mesh functionality, or a separate
capability.

**It is a general field consumer.** A displacement is a `Vec3` field of position
and time — it has no more to do with materials than a heightfield does. The
engine already proves this: `MeshOp::Displace` in `crates/axiom-proc-mesh` is
bake-time deformation with no material anywhere near it, and
`crates/axiom-mesh-ops` transforms geometry with no material either.

`Surface` carries a `Displacement` channel **only because that is the binding
site for the vertex stage of the program the fragment channels already compile
into.** It is a wiring convenience, not a claim that deformation is an appearance
concept. Say so in `crates/axiom-surface/ARCHITECTURE.md` so a future agent does
not conclude that GPUs executing something makes it a material.

## Architectural placement

* Bake-time: **Layer `mesh-ops`** and **Layer `proc-mesh`** — already done in `05`.
* Runtime: **Engine module `gpu-backend`** (the vertex stage) and
  **`canvas2d-backend`** (which cannot do it — see below).
* The channel itself: **Layer `surface`** — already declared in `04`.

## Existing code involved

| Path | Role |
|---|---|
| `modules/axiom-gpu-backend/src/scene_renderer.rs:180` | `vs` — the rigid vertex entry |
| `scene_renderer.rs:212` | `vs_skinned` — linear-blend skinning from a joint-palette **texture** (chosen over a storage buffer for WebGL2) |
| `scene_renderer.rs:245-252` | *"16 vertex attributes … exactly the WebGL2 downlevel guarantee … a 17th would fail pipeline creation"* |
| `scene_renderer.rs:253-256` | skinned draws set emissive and specular to zero — **because the pipeline is already at the attribute ceiling** |
| `crates/axiom-proc-mesh/src/transforms.rs` | `MeshOp::{Displace, Bend}` — bake-time, hardcoded noise |
| `crates/axiom-host/src/frame_retro_32bit.rs` | the only other vertex-stage effect (vertex snap) |
| `modules/axiom-canvas2d-backend/src/mesh_skinning.rs:186-228` | CPU skinning — allocates a fresh `MeshGeometry` **per skinned draw per frame** |

## Files owned

| Path | Action |
|---|---|
| `modules/axiom-gpu-backend/src/surface_program/emit_vertex.rs` | create |
| `modules/axiom-gpu-backend/src/surface_program/plan.rs` | modify — `StageSplit` |
| `modules/axiom-gpu-backend/src/scene_renderer.rs` | modify **minimally** — vertex splice marker only |
| `crates/axiom-surface/src/{surface_builder.rs, requirements.rs}` | modify — displacement helpers |
| `modules/axiom-canvas2d-backend/src/surface_shading.rs` | modify — report the drop |

## Dependencies on earlier manifests

**`08`** (the emitter). Parallel with `11` if both stay inside
`surface_program/`; otherwise sequential after it.

## Public API / data contracts

### Vertex-stage emission

The `Displacement` channel's graph is emitted into the **vertex** stage, with an
`EvalContext` whose `point` is the object-space vertex position, `normal` the
vertex normal, `uv` the vertex uv, and `time` the frame's deterministic time
uniform. Output is added to the object-space position before the MVP multiply.

```wgsl
fn axiom_displace(pos: vec3<f32>, nrm: vec3<f32>, uv: vec2<f32>, t: f32,
                  params: SurfaceParams) -> vec3<f32> { ... }
```

**The hard constraints, and they are hard:**

1. **No new vertex attributes.** 16 of 16 are bound on the rigid pipeline; a 17th
   fails pipeline creation on the browser fallback path. Everything the
   displacement graph reads must already be an attribute (position, normal, uv)
   or come from the surface parameter uniform.
2. **The skinned pipeline gets no displacement in this manifest.** It is already
   at the ceiling and already silently drops emissive and specular for that
   reason. `SurfaceRequirements::has_displacement` on a skinned draw is a
   validation failure reported as a degraded feature, not a silent no-op. Say so
   in the error message.
3. **Normals are not recomputed.** Displacing a vertex invalidates its normal, and
   recomputing requires neighbour access the vertex stage does not have. The
   honest options are: leave the normal (correct for small displacement), or bind
   a `Normal` channel that the author derives analytically from the same field.
   **Choose the second and document it** — that is exactly what
   `normal_from_height` in `04` exists for. Do not pretend the geometric normal is
   still right; `apps/burnt-rubber/src/render/rock_mesh.rs:40` already records
   this limitation for the bake-time case (*"re-deriving normals is not something
   the operator vocabulary can express today"*), and the field algebra now can.

### Time, and determinism

`EvalContext::time` is a kernel `Seconds` supplied by the frame, never a wall
clock. Wind and ripple are therefore replayable: the same tick produces the same
displacement. This is the brief's "time-varying procedural fields where
deterministic engine time is explicitly supplied", and it is the only sanctioned
way time enters a field.

**A surface whose `SurfaceRequirements::inputs` includes `Time` writes its time
uniform per frame; one whose does not, does not.** That distinction already
exists in `04` and is what keeps static surfaces free.

### Bend, twist, squash, wind, ripple — all library graphs

None of these is an engine feature.

| Effect | The graph |
|---|---|
| wind | `Mul(Const(dir), Mul(Fbm(seed, Add(Point, Mul(Time, speed))), strength))`, masked by height |
| ripple | `Mul(Normal, Mul(Smoothstep(…, Length(Sub(Point, centre))), amplitude))` — a radial falloff, no trigonometry needed |
| bend | `Transform` with a `Mat4` parameter, masked by `Component(Point, 1)` |
| twist | a `Transform` whose parameter the app updates per frame from the sim |
| squash/stretch | component-wise `Mul` against a `Vec3` parameter |

**Note the honest gap:** a true twist needs `sin`/`cos`, which the 23-op algebra
excludes (`01`) because transcendentals break the CPU/GPU parity budget. A twist
is therefore expressed as a per-frame `Mat4` **parameter** computed by the app
and uploaded — which costs a uniform write, not a recompile. If a real consumer
proves that insufficient, that is the moment to consider adding `Sin`, with a
parity-tolerance decision attached, and not before.

### Canvas2D

Cannot do vertex displacement usefully. It CPU-skins already and allocates a
fresh `MeshGeometry` per skinned draw per frame (`mesh_skinning.rs:186-228`);
adding per-vertex field evaluation on top would multiply that cost on the backend
least able to pay it.

**Report `Displacement` as a dropped feature on Canvas2D.** The silhouette will
differ from the GPU arm. That is acceptable and is consistent with the existing
policy — burnt-rubber's own convergence campaign sets
`guard_rule = "legibility, not parity"` for the software arm.

## Explicitly excluded

* No skinned-pipeline displacement (attribute ceiling).
* No tessellation, no geometry shaders, no compute — none exist on this path and
  WebGL2 has none.
* No automatic normal recomputation.
* No Canvas2D displacement.
* No new vertex attributes, under any justification.

## Determinism requirements

Same tick + same parameters → same displaced positions. The vertex stage mirrors
`FieldGraph::evaluate`, so `08`'s parity test extends to cover displacement
graphs sampled at vertex positions.

## Serialization requirements

None new — displacement is a channel on `Surface` and rides its bytes.

## Testing requirements (100%)

* CPU↔GPU parity for a displacement graph over a sampled vertex set, at `08`'s
  tolerance.
* A displaced mesh's rendered silhouette differs from the undisplaced one —
  assert on a captured image via `axiom-shot`, not merely on the shader string.
* Time-varying displacement at tick N and tick N+60 differ; tick N replayed twice
  is identical.
* A skinned draw with a displacement channel reports the degraded feature.
* Canvas2D reports `Displacement` dropped.
* `has_displacement == false` surfaces emit **no** vertex-stage code (assert on
  the generated string) and write no time uniform.

## Architecture tests

`cargo xtask check-architecture`; `engine_no_large_files` — keep the vertex
emitter in its own file, do not grow `scene_renderer.rs`.

## Performance risks

* The vertex stage runs per vertex per frame, and burnt-rubber's course is
  vertex-heavy. A displacement graph is real per-vertex cost; record node count in
  telemetry as `08` does for the fragment stage.
* A surface with `Time` forces a per-frame uniform write. Keep it to one small
  write; do not repack the whole parameter buffer.
* **Do not let displacement force a second pipeline for the same surface.** The
  vertex and fragment stages compile into one program keyed by one digest.

## Migration considerations

None. Additive; existing content has no displacement channel.

## Completion criteria

1. A displacement-bound surface visibly deforms geometry on the GPU arm.
2. Parity holds against the CPU evaluator.
3. Skinned and Canvas2D cases report degraded features rather than failing
   silently.
4. Static surfaces emit no vertex code and cost nothing.
5. Wind and ripple are demonstrated as **authored graphs** in a test, with no new
   Rust operator.
6. Coverage 100/100/100; `cargo xtask check-architecture` exits 0; no dylint count
   rises.

## Validation commands

```sh
cargo test -p axiom-gpu-backend --features offscreen
cargo test -p axiom-surface
cargo test --workspace
cargo xtask check-architecture
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
```

## Parallel safety

**Wave 8.** Parallel with `08`/`11` only if confined to
`surface_program/emit_vertex.rs` and `plan.rs`.
