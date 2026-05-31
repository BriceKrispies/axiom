# Axiom — Headless Deterministic Rotating-Cube Vertical Slice (First Pass)

This app is Axiom's first end-to-end vertical slice and the **only** place
in the workspace where the four engine modules (`scene`, `resources`,
`render`, `webgpu`) are composed. This first pass is **headless**: it
proves the layers and modules can produce deterministic per-frame
artifacts end-to-end. It does **not** open a canvas, create a WebGPU
surface or swapchain, run `wasm-bindgen`, or present pixels.

## What this first pass proves

1. Tick 0 produces deterministic artifacts.
2. Tick 60 produces a **different** cube world transform than tick 0.
3. Tick 0 replayed from a fresh app is **byte-equal** to the first tick 0.
4. `SceneSnapshot`, `ResolvedResources`, `RenderInput`,
   `RenderCommandList`, `GpuSubmission`, and `GpuSubmissionReport` are all
   captured as inspectable plain-data artifacts.
5. Module boundaries stay isolated — no module imports another module.
6. All cross-module composition glue lives in this app crate.
7. The whole pipeline runs through the real module facades (the render
   command list is built by `axiom-render`; the GPU submission is recorded
   by `axiom-webgpu`), not re-implemented in the app.

These are exercised by `tests/vertical_slice.rs` and the per-module unit
tests in `src/*.rs`.

## Public facade

The app exposes a single **behavioral** facade from `lib.rs`:
[`DemoRotatingCubeApi`]. Its one method,
`run_tick(&mut self, tick) -> VerticalSliceArtifact`, runs the full
headless pipeline for one deterministic tick and returns a single
inspectable artifact.

Everything else re-exported from `lib.rs` is **inert plain data**: the
artifact tree that `run_tick` returns. Those types carry no behavior; they
are exported only so every boundary value is inspectable by callers and
tests (a public method returning a private type would not compile under the
workspace lint settings).

## Exact data flow

```text
                                              OWNING CRATE
──────────────────────────────────────────────────────────────────────
frame tick                                    app (DemoRotatingCubeApi)
  → HostFrameInput                            app builds → axiom-host
  → HostFrameReport  (runtime steps once)     axiom-host
  → EngineFrame / FrameContext                axiom-frame
  → scene transform update                    axiom-scene
  → SceneSnapshot                             axiom-scene
  → ResolvedResources                         axiom-resources
  ───── app glue: scene_to_render_input ─────────────────────────────
  → RenderInput          (app plan → builder) axiom-render
  → RenderCommandList                         axiom-render
  ───── app glue: render_command_list_to_gpu_submission ─────────────
  → GpuSubmission        (app plan → builder) axiom-webgpu
  → GpuSubmissionReport                       axiom-webgpu
```

### Which crate owns each artifact

| Boundary value         | Owning crate                | Mirrored as (app artifact)     |
|------------------------|-----------------------------|--------------------------------|
| `HostFrameInput`       | `axiom-host`                | (frame bookkeeping fields)     |
| `HostFrameReport`      | `axiom-host`                | (frame bookkeeping fields)     |
| `EngineFrame`          | `axiom-frame`               | `engine_frame_index`, …        |
| `SceneSnapshot`        | `axiom-scene`               | `SceneSnapshotArtifact`        |
| `ResolvedResources`    | `axiom-resources`           | `ResolvedResourcesArtifact`    |
| `RenderInput`          | `axiom-render`              | `RenderInputArtifact`          |
| `RenderCommandList`    | `axiom-render`              | `RenderCommandListArtifact`    |
| `GpuSubmission`        | `axiom-webgpu`              | `GpuSubmissionArtifact`        |
| `GpuSubmissionReport`  | `axiom-webgpu`              | `GpuSubmissionReportArtifact`  |
| translation glue       | `axiom-demo-rotating-cube`  | —                              |

## Source layout

| File                            | Responsibility                                             |
|---------------------------------|------------------------------------------------------------|
| `src/lib.rs`                    | Single facade + the inert artifact re-exports.             |
| `src/demo_api.rs`               | `DemoRotatingCubeApi` + persistent engine state.           |
| `src/vertical_slice.rs`         | Per-tick orchestrator + `VerticalSliceArtifact`.           |
| `src/scene_to_render_input.rs`  | Glue: `SceneSnapshot + ResolvedResources → RenderInput`.   |
| `src/render_to_gpu_submission.rs` | Glue: `RenderCommandList → GpuSubmission`.               |
| `tests/vertical_slice.rs`       | Determinism, boundary-completeness, isolation proofs.      |

## Why composition glue lives in the app

Two modules can never name each other's types: each module re-exports
**exactly one** facade, and its contract types (`SceneSnapshot`,
`RenderInput`, `GpuSubmission`, …) live behind private modules. So the only
way to bridge two modules is for an **app** to read the producer's facade
and feed the consumer's facade. That keeps each module a black box with a
stable shape and keeps every two-module pairing re-composable by future
apps (a native app, a WASM app, a different backend) without rewriting the
modules.

Concretely, the app's two glue steps are pure functions over **app-owned,
nameable** plain-data types:

- `scene_to_render_input(math, scene, resources) -> RenderInputArtifact`
- `render_command_list_to_gpu_submission(commands, w, h) -> GpuSubmissionArtifact`

The orchestrator in `vertical_slice.rs` does only the mechanical plumbing
that *must* touch the un-nameable module values: it reads each producer
value (through its facade's accessors) into a plain-data artifact, runs the
nameable glue, and replays the resulting plan back into the next module's
builder. Because a helper function cannot name an un-nameable contract type
in its signature, that plumbing lives in one function by necessity — the
*decisions* it makes are delegated to the two glue modules, which are
independently unit-tested.

## Why this pass is headless

`axiom-webgpu` ships only its `Recording` backend today: each command
pushed into a `GpuSubmission` is captured into a deterministic
`GpuSubmissionReport` rather than issued to a real GPU device. No surface,
no swapchain, no device, no draw calls. That is exactly what makes the
slice deterministic and CI-friendly: every boundary — up to and including
the GPU submission **shape** — is a comparable value.

## Missing host / WebGPU presentation boundary (what blocks actual pixels)

Real WebGPU presentation is blocked entirely in the **host layer**, not in
this app:

1. **No surface handle.** `axiom-host`'s `HostViewport` describes the
   viewport dimensions and scale factor but does not hand out a
   `wasm-bindgen` canvas reference, a `RawWindowHandle`, or a
   `wgpu::Surface` constructor. Without a surface there is no swapchain to
   present to.
2. **No async device-init path.** `wgpu`'s
   `Instance → Adapter → Device → Queue` initialisation is async;
   `axiom-host` is synchronous today and `axiom-runtime`/`axiom-frame`
   assume synchronous stepping.
3. **No adapter-request entry point.** `axiom-host` exposes nothing shaped
   like the browser `navigator.gpu.requestAdapter()` path.

(See `modules/axiom-webgpu/ARCHITECTURE.md` for the backend-side view.)

## What remains for the browser-visible second pass

- Extend `axiom-host` with a surface/adapter/async-device capability
  (a new platform-facing boundary on the host layer's allowlist).
- Add a `BackendKind::Live` arm to `axiom-webgpu::WebGpuApi::submit` that
  drives real `wgpu` calls. The `GpuSubmission` / `GpuSubmissionReport`
  **shape stays unchanged**, so this app's glue does not change.
- Add a thin WASM/`wasm-bindgen` entrypoint **in a new app** (not in any
  module) that owns the canvas and the render loop.

None of that is in this first pass. **This pass does not render pixels in a
browser.**

## How to run

```sh
cargo test -p axiom-demo-rotating-cube     # app unit + integration tests
cargo test --workspace                     # whole workspace, incl. arch check
cargo xtask check-architecture             # the Axiom Layer/Module/App laws
```
