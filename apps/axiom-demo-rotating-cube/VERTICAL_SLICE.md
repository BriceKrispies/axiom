# Axiom — Deterministic Rotating Cube Vertical Slice

This app is the engine's first end-to-end vertical slice. It is the
**only** place in the workspace where the four engine modules are
composed together. Every step in the pipeline is owned by exactly
one crate; the app does the translation between module contracts.

## Boundary-by-boundary pipeline

```text
                        OWNING CRATE
─────────────────────────────────────────────────────────────────
                                            apps/axiom-demo-
                                            rotating-cube
   ┌── frame tick                          (app)
   │
   ▼
host frame input                            axiom-host
   │
   ▼
host frame report                           axiom-host
   │
   ▼
engine frame context                        axiom-frame
   │
   ▼
scene transform update                      axiom-scene
   │
   ▼
SceneSnapshot                               axiom-scene
   │
   │     ResolvedResources                  axiom-resources
   │           ▼
   └─────────►  ◄────────────────────────  (app translation)
                   ▼
                 RenderInput                axiom-render
                   ▼
                 RenderCommandList          axiom-render
                   ▼
              (app translation)
                   ▼
                 GpuSubmission              axiom-webgpu
                   ▼
                 GpuSubmissionReport        axiom-webgpu
```

### Step-by-step

| #  | Boundary value         | Producer crate      | Consumer crate(s)          |
|----|------------------------|---------------------|----------------------------|
| 1  | `HostFrameInput`       | app                 | `axiom-host`               |
| 2  | `HostFrameReport`      | `axiom-host`        | `axiom-frame`              |
| 3  | `EngineFrame`          | `axiom-frame`       | `axiom-frame`              |
| 4  | `FrameContext`         | `axiom-frame`       | `axiom-scene` (via `advance`) |
| 5  | `SceneSnapshot`        | `axiom-scene`       | app                        |
| 6  | `ResourceTable`        | `axiom-resources`   | `axiom-resources`          |
| 7  | `ResolvedResources`    | `axiom-resources`   | app                        |
| 8  | `RenderInput`          | `axiom-render` (app builds) | `axiom-render`     |
| 9  | `RenderCommandList`    | `axiom-render`      | app                        |
| 10 | `GpuSubmission`        | `axiom-webgpu` (app builds) | `axiom-webgpu`     |
| 11 | `GpuSubmissionReport`  | `axiom-webgpu`      | app                        |

### Translation glue (app-owned)

- `SceneSnapshot + ResolvedResources → RenderInput`
  - Read `snapshot.cameras()`, compute `view = inverse(world)` and
    `projection = MathApi::mat4_perspective(...)`.
  - Read `snapshot.lights()`, translate each to a `RenderApi`
    directional / point light.
  - Walk `ResolvedResources` meshes and materials, add each to
    `RenderInput` via `RenderApi::add_input_mesh` /
    `add_input_basic_lit_material`. Remember the resulting render-
    side indices keyed by resource id.
  - For each `snapshot.renderables()`, look up its mesh/material
    indices and add a `RenderObject` with the renderable's node's
    world matrix.
- `RenderCommandList → GpuSubmission`
  - Walk the list one command at a time via `RenderApi`'s indexed
    accessors, switching on the `KIND_*` `u32` codes. For each
    command, call the matching `WebGpuApi::submission_*` method.
  - Append `submission_present` at the end.

## What is deterministic today

- The scene is rebuilt each tick from the rotation derived from the
  tick number (`cube_rotation_for_tick`).
- The resource table is rebuilt each tick with the built-in cube
  mesh and basic-lit material.
- The runtime steps exactly once per tick (fixed-step config).
- The host driver, the frame builder, and the runtime are persistent
  across ticks so engine frame indices, host frame sequences, and
  runtime tick counts increase monotonically.
- The per-tick `CubeFrame` is plain-data (`Vec<u32>` + `Mat4` + scalar
  counts) and byte-equal across two replays of the same tick
  sequence.

## What is NOT working today (and why)

**Actual WebGPU presentation does not run.** The `axiom-webgpu`
module is in `Recording` backend mode: every submission is captured
into a deterministic `GpuSubmissionReport` instead of being submitted
to a real device. The blockers are entirely in `axiom-host`:

1. **No surface handle.** `HostViewport` describes the viewport
   dimensions and scale factor but does not yet hand out a
   `wasm-bindgen` canvas reference, a `RawWindowHandle`, or a
   `wgpu::Surface` constructor. Without a surface there is no
   swap-chain to present to.
2. **No async device init path.** `wgpu`'s
   `Instance ⇒ Adapter ⇒ Device ⇒ Queue` initialisation is async.
   `axiom-host` is synchronous today and `axiom-runtime`/`axiom-frame`
   assume synchronous stepping.
3. **No adapter-request entry point.** `axiom-host` does not expose
   a "request adapter" entry matching the future browser
   `navigator.gpu.requestAdapter()` shape.

The vertical slice is otherwise complete: the headless test
`deterministic_rotating_cube_tick_60_produces_stable_render_commands`
proves that every boundary up to (and including) the GPU submission
shape is deterministic. When the host layer exposes the missing
surface / adapter / async-init capability, `axiom-webgpu`'s
`submit()` can be extended with a `BackendKind::Live` arm that runs
real `wgpu` calls without changing the `GpuSubmission` /
`GpuSubmissionReport` shape.

## How to run

```sh
cargo test -p axiom-demo-rotating-cube
```

The integration test
`deterministic_rotating_cube_tick_60_produces_stable_render_commands`
runs the full pipeline for 61 ticks and asserts the shape, content,
and byte-equality of every artifact.

## How to extend

The cleanest module-isolated extensions:

- Add new mesh kinds — extend `ResourcesApi` with new built-in mesh
  builders. The app's translation step does not change.
- Add new render pipelines — extend `RenderApi` with new `KIND_*`
  codes and `add_input_*` builder methods. The app's translation
  step grows a new `match` arm.
- Add a live GPU backend — implement `BackendKind::Live` inside
  `axiom-webgpu` once the host layer exposes a surface. The boundary
  shape (`GpuSubmission`/`GpuSubmissionReport`) stays unchanged.
