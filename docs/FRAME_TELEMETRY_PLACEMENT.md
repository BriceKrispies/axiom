# Frame Telemetry Placement

> Investigation + placement report. **No code was written for this pass.** It
> identifies where frame-production and frame-consumption measurement belongs in
> the current engine, with exact files, types, and boundary functions. The
> implementation is a later pass (see the checklist).

The central distinction the design must preserve:

- **Deterministic event counting** (how many sim ticks / engine frames / render
  artifacts the engine *produced*, and how many frames the frontend *consumed* /
  the backend *attempted to present*) — plain integer counters, replayable, no
  wall clock. These live in the engine spine (layers/modules) and may use the
  kernel telemetry primitives.
- **Wall-clock rate calculation** (rolling FPS over a time window) — derived from
  counter deltas against `performance.now()` / `Instant`. This is a host/app/
  dev-harness concern and must never enter the deterministic core.

Engine-produced frames are **not** the same as frontend-consumed frames, which
are **not** the same as presentation completions. Today they happen to be 1:1
(see "Current frame/data flow"), but the telemetry must be shaped so divergence
becomes measurable the moment the produce loop decouples from `requestAnimation‑
Frame`.

---

## Current frame/data flow

The path from a deterministic tick to a visible pixel, with exact files and
functions:

```
requestAnimationFrame (browser)
  └─ axiom-windowing run loop closure         modules/axiom-windowing/src/windowing_api.rs
        WindowingApi::step() -> u64               (advances next_tick + frames_driven)   [~line 128]
        frame_fn(tick)  ── app closure ──▶ RunningApp::tick(tick)                         modules/axiom/src/app.rs:375
            Runtime::step()                       crates/axiom-runtime/src/runtime.rs:144 (advances SimulationClock by one FixedStep)
            HostStepDriver / FrameBuilder::build()  crates/axiom-frame/src/frame_builder.rs:~70  → EngineFrame (engine_frame_index++)
            SceneApi::advance(tick, frame) -> SceneSnapshot   modules/axiom-scene/src/scene_api.rs:265
            RenderPipelineApi::submit(frame, scene, webgpu) -> RenderReport   modules/axiom-render-pipeline/src/render_pipeline_api.rs:201
                RenderApi::build_command_list(input) -> RenderCommandList   modules/axiom-render/src/render_api.rs:160
                WebGpuApi::submit(GpuSubmission) -> GpuSubmissionReport     modules/axiom-webgpu/src/webgpu_api.rs:172
            └─ returns FrameOutcome               modules/axiom/src/frame_outcome.rs (tick, draws, clear, lights, presented, recorded)
        be.present(tick, w, h, clear, lights, light_vp, batches)   windowing_api.rs LiveBackend::present [~line 396]
            LiveBackend::Gpu  → GpuBackendApi::present_frame(...) -> bool          modules/axiom-gpu-backend/src/gpu_backend_api.rs:68
                LiveGpuBinding::render_frame(...)  → frame.present()               modules/axiom-gpu-backend/src/live_gpu_binding.rs:185  (real GPU present)
            LiveBackend::Canvas → Canvas2dBackendApi::present_packet(&FramePacket) -> FrameSubmissionReport  modules/axiom-canvas2d-backend/src/canvas2d_backend_api.rs:95
                LiveCanvasBinding::blit(...) → ctx.put_image_data(...)             modules/axiom-canvas2d-backend/src/live_canvas_binding.rs:74  (real Canvas2D present)
        request_animation_frame(cb)  → schedule next frame                        windowing_api.rs:558
```

**Key property of today's loop:** `axiom-windowing`'s `run_web_multi` calls
`step()` **once**, then the app closure (which calls `RunningApp::tick` **once**),
then `present()` **once**, per `requestAnimationFrame` callback. So in the current
synchronous design: **sim ticks advanced == engine frames produced == render
artifacts emitted == frontend frames consumed == present attempts == 1 per rAF
callback.** "Produced but not consumed" is structurally zero *today*; it becomes
non-zero only if a future loop runs the sim/produce step on its own cadence (e.g.
a fixed-tick accumulator producing multiple frames, or producing while a tab is
hidden) and the frontend consumes only the latest. The telemetry must be built so
that divergence is observable, even though the counts coincide now.

Native/headless apps (`apps/axiom-demo-rotating-cube`, `apps/axiom-sim-crucible`,
etc.) and the `tools/axiom-profile-runner` CPU profiler drive `RunningApp::tick`
(or lower) directly with no presentation at all — so the produce side and the
present side are already separable code paths.

---

## Existing relevant types

### Deterministic time / identity primitives — `crates/axiom-kernel/`
- `Tick` (`src/tick.rs`) — `u64` newtype; `new`, `raw`, `next` (saturating).
- `FrameIndex` (`src/frame_index.rs`) — `u64` newtype; `new`, `raw`, `next`.
- `SimulationClock` (`src/simulation_clock.rs`) — fields `step/tick/frame/
  elapsed_nanos`; `advance()`, `advance_by(n)`, `tick()`, `frame()`,
  `elapsed_nanos()`. **Advanced explicitly; never reads a wall clock.**
- `FixedStep` (`src/fixed_step.rs`) — `nanos` (non-zero).

### Deterministic telemetry primitives — `crates/axiom-kernel/` (re-exported in `lib.rs`)
- `TelemetryMetric` (`src/telemetry_metric.rs`) — `{name, kind, value, tick}`;
  `counter(name, i64, Option<Tick>)`, `gauge(...)`.
- `MetricKind` (`src/metric_kind.rs`) — `Counter = 0`, `Gauge = 1`.
- `MetricValue` (`src/metric_value.rs`) — `integer(i64)` / `float(f32)`.
- `TelemetrySink` (trait, `src/telemetry_sink.rs`) — `record(&mut self, TelemetryMetric)`.
- `InMemoryTelemetrySink` (`src/in_memory_telemetry_sink.rs`) — `record`,
  `metrics()`, `counter_total(name) -> i64`, `clear()`.
- (Parallel logging set: `LogRecord`, `LogField`, `LogLevel`, `LogSink`,
  `InMemoryLogSink` — `LogRecord` already carries optional `tick`/`frame`.)

### Runtime stepping — `crates/axiom-runtime/`
- `Runtime::step() -> RuntimeResult<RuntimeStepRecord>` (`src/runtime.rs:144`,
  `#[axiom_zones::sim]`).
- `RuntimeTimeline::advance() -> RuntimeStep` (`src/runtime_timeline.rs:45`);
  field `sequence: u64` (monotonic, +1 per advance).
- `RuntimeStep` (`src/runtime_step.rs`) — `{frame, tick, fixed_delta_nanos, sequence}`.
- `RuntimeStepRecord`, `RuntimeDiagnostics` — per-step summaries + metrics.

### The deterministic frame boundary — `crates/axiom-frame/` (layer; `depends_on = ["kernel","runtime","host"]`)
- `EngineFrame` (`src/engine_frame.rs`) — **`engine_frame_index: u64`** (the
  layer-04 monotonic produced-frame counter), `host_frame_sequence: u64`,
  `runtime_step_summaries: Vec<FrameStepSummary>`, `timing: FrameTiming`, … .
  Accessor `engine_frame_index()`.
- `FrameBuilder` (`src/frame_builder.rs`) — `build(&mut self, &HostFrameReport,
  Vec<FrameCommand>) -> FrameResult<EngineFrame>` (`~line 70`, `#[axiom_zones::sim]`);
  field `next_engine_frame_index: u64`. **This is where "an engine frame was
  produced" happens.**
- `FrameTiming` (`src/frame_timing.rs`) — **`runtime_steps_executed: u32`**,
  `consumed_nanos`, `retained_nanos`, `fixed_step_nanos`, **`skipped: bool`**.
  Doc: "Every value is an explicit integer count … nothing here is derived from a
  wall clock." `from_host_report(report, fixed_step_nanos)`.
- `FrameStepSummary` (`src/frame_step_summary.rs`) — `{runtime_frame_index,
  runtime_tick, runtime_sequence, succeeded, systems, metrics}`;
  `from_record(&RuntimeStepRecord)`.

### Host boundary (deterministic) — `crates/axiom-host/` (layer; `depends_on = ["kernel","runtime"]`)
- `HostApi`, `HostStepDriver`, `HostFrameReport` (`steps_executed()`, `plan()`,
  `sequence()`), `HostStepPlan` (`consumed_nanos`, `retained_nanos`,
  `is_skipped`), `HostViewport`, `HostPresentationRequest`, `Pixels`,
  `FramePacket` (backend-neutral frame artifact; carries `frame_index`, `tick`).
  **No browser/GPU/wall-clock here.**

### Render artifacts (deterministic) — `modules/`
- `SceneSnapshot` (`axiom-scene`, `SceneApi::advance/snapshot`).
- `ResolvedResources` (`axiom-resources`, `ResourcesApi::resolve`).
- `RenderInput`, `RenderCommandList`, `RenderCommand` (`axiom-render`,
  `RenderApi::build_command_list:160`); `command_count(&list)`. `FramePacket` built
  via `RenderApi::build_frame_packet(input, frame_index, tick, …):286`.
  `RenderReceipt` (`capture_receipt(FrameIndex, Tick, &RenderCommandList)`) —
  carries `frame_index`, `tick`, `command_count`.
- `GpuSubmission`, `GpuSubmissionReport` (`axiom-webgpu`, `WebGpuApi::submit:172`)
  — **already has `clear_count`, `draw_count`, `present_count`** and
  `GpuSubmissionStatus` (`Recorded` / `LiveNotBound` / `LiveNotInitialized`;
  `presented()` is always `false` for the recording backend).
- `RenderReport` (`axiom-render-pipeline`, `RenderPipelineApi::submit:201`) —
  `command_count`, `draws`, `lights`, `presented`, `recorded`.

### App-facade per-frame output — `modules/axiom/` (feature module; composes scene, resources, render-pipeline, webgpu, windowing)
- `App` / `RunningApp` (`src/app.rs`); `RunningApp::tick(u64) -> FrameOutcome`
  (`:375`), `tick_with`, `tick_with_controls`; browser entry `App::run()` (`:113`,
  wasm32) wires `WindowingApi::run_web_multi` to `RunningApp::tick`.
- `FrameOutcome` (`src/frame_outcome.rs`) — `{tick, command_count, clear_color,
  draws, lights, light_view_proj, presented, recorded}`; `instance_floats()`,
  `mesh_batches()`.

### Presentation driver + live backends (platform-facing) — `modules/`
- `WindowingApi` (`axiom-windowing/src/windowing_api.rs`, feature module,
  `allowed_layers = ["kernel","host"]`, `allowed_modules =
  ["gpu-backend","canvas2d-backend"]`) — fields **`next_tick: u64`,
  `frames_driven: u64`**; `step() -> u64` (`:128`, increments both);
  `frames_driven()` (`:141`); wasm32 `run_web` / `run_web_multi` (`:158`) /
  `run_web_streaming`; `LiveBackend::{Gpu,Canvas}::present(...)`;
  `request_animation_frame` (`:558`). **No wall clock; `frames_driven` is a plain
  monotonic count, not a rate.**
- `GpuBackendApi` (`axiom-gpu-backend`, wasm32 platform module) —
  `present_frame(...) -> bool` (`:68`), `binding_is_ready()`; `LiveGpuBinding::
  render_frame` → `frame.present()` (`live_gpu_binding.rs:185`).
- `Canvas2dBackendApi` (`axiom-canvas2d-backend`, wasm32 platform module) —
  `present_packet(&FramePacket) -> FrameSubmissionReport` (`:95`);
  `LiveCanvasBinding::blit` → `ctx.put_image_data` (`live_canvas_binding.rs:74`).

### Pre-existing wall-clock FPS (the ONLY one in the repo)
- `apps/axiom-stress-cubes-browser/web/index.html` (~lines 73–88) — a **JS**
  `requestAnimationFrame` loop using `performance.now()`, a 500 ms window,
  computing FPS (`frames*1000/acc`) and frame-ms (`acc/frames`), writing
  `#fps` / `#ms` / `#count` DOM spans. This is page-level, outside all Rust.
  **No Rust layer/module/app currently computes wall-clock FPS.** This is the
  precedent for where rate calculation lives.

---

## Recommended ownership

Legend — **Storage**: `counter` (monotonic u64), `metric` (kernel
`TelemetryMetric`), `derived` (computed from other counters), `value` (a field on
an existing record).

| Measurement | Kind | Owning crate/module/app | File it should live in | Boundary function where recorded | Storage |
|---|---|---|---|---|---|
| `sim_ticks_advanced` | deterministic | layer `axiom-frame` | new `crates/axiom-frame/src/frame_telemetry.rs` | `FrameTelemetry::record(&EngineFrame)` called right after `FrameBuilder::build` (adds `frame.timing().runtime_steps_executed()`) | counter |
| `engine_frames_produced` | deterministic | layer `axiom-frame` | `crates/axiom-frame/src/frame_telemetry.rs` | same `FrameTelemetry::record(&EngineFrame)` (+1 per built non-… frame; equals `engine_frame_index + 1`) | counter |
| `render_artifacts_emitted` | deterministic | module `axiom` (App facade, composition tier) | `modules/axiom/src/app.rs` (+ a small `frame_telemetry.rs` if it grows) | `RunningApp::tick*` right after `RenderPipelineApi::submit` returns a `RenderReport` (+1 per report) | counter |
| `frontend_frames_consumed` | deterministic count (rate is wall-clock) | module `axiom-windowing` | `modules/axiom-windowing/src/windowing_api.rs` | **already** `WindowingApi::step()` (today's `frames_driven`); expose as `frames_consumed()` | counter |
| `present_attempts` | deterministic count, backend-abstract | module `axiom-windowing` (drives present) | `modules/axiom-windowing/src/windowing_api.rs` | increment in the run-loop closure immediately **before** `LiveBackend::present(...)` | counter |
| `present_completions` | deterministic count, backend-abstract | module `axiom-windowing`, reading the backend result | `modules/axiom-windowing/src/windowing_api.rs` | increment when `GpuBackendApi::present_frame -> true` / `Canvas2dBackendApi::present_packet` reports presented | counter |
| `produced_but_not_consumed` | derived (no wall clock) | module `axiom` facade (sees both sides) OR app | computed, not stored: `engine_frames_produced - frontend_frames_consumed` | read at snapshot assembly | derived |
| `consumed_but_not_presented` | derived (no wall clock) | module `axiom` facade OR app | `frontend_frames_consumed - present_completions` | read at snapshot assembly | derived |
| `rolling_engine_output_fps` | **wall-clock** | **app / dev-harness only** | per browser app `src/web.rs` (or a dev-harness tool), e.g. `FramePacingWindow` | app's rAF closure / dev loop, from `engine_frames_produced` delta ÷ wall-time delta | derived (wall-clock) |
| `rolling_frontend_consume_fps` | **wall-clock** | **app / dev-harness only** | same `FramePacingWindow` | from `frontend_frames_consumed` delta ÷ wall-time delta (this is what `stress` index.html measures today) | derived (wall-clock) |
| `rolling_present_fps` | **wall-clock** | **app / dev-harness only** | same `FramePacingWindow` | from `present_completions` delta ÷ wall-time delta | derived (wall-clock) |

Notes:
- Several deterministic counters **already exist** and should be *surfaced*, not
  re-invented: `EngineFrame::engine_frame_index` (== `engine_frames_produced - 1`),
  `FrameTiming::runtime_steps_executed` (the per-frame term of `sim_ticks_advanced`),
  `FrameTiming::skipped` (drives a `frames_skipped` counter), `WindowingApi::
  frames_driven` (== `frontend_frames_consumed`), and `GpuSubmissionReport::
  {draw_count, present_count}` (abstract backend event counts for the recording
  path).
- **Renderer draw attempts** (the task's item 5) map to `GpuSubmissionReport::
  draw_count` (recording backend) and, for the live path, a backend-abstract
  `draw_attempts` counter alongside `present_attempts` in the backend modules.
  Keep these **abstract** (a "draw attempt" / "present attempt" event), never a
  Canvas2D-specific or wgpu-specific field in render-core.

### Which boundary exposes the data (Q9)

`modules/axiom` (the `axiom` **feature module**) is the single legal aggregation
point, because it is the only crate that already composes **both** the engine
produce side (`axiom-frame` via `RunningApp`) **and** the frontend/present side
(`axiom-windowing`). It exposes a deterministic `FrameTelemetrySnapshot`
(`RunningApp::frame_telemetry()` + windowing counters). **Apps** read that
snapshot each frame and pair it with a wall-clock `FramePacingWindow` to produce
on-screen rates. A module cannot import another module, so the *combining* must
stay in `axiom` (which lists both in `allowed_modules`) or in the app; it must not
be smeared across the isolated engine modules.

---

## Proposed minimal design

Smallest structurally correct shape. Deterministic counting is one set of types
in the spine; wall-clock rate is a separate type in app/dev-harness. No god
object, no "PerformanceManager".

### 1. Deterministic engine-produce counters — `crates/axiom-frame/src/frame_telemetry.rs`

```rust
/// Cumulative, deterministic frame-production counters. No wall clock.
/// One per RunningApp; folded forward as each EngineFrame is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameTelemetry {
    engine_frames_produced: u64,
    sim_ticks_advanced: u64,
    frames_skipped: u64,
}

impl FrameTelemetry {
    pub fn new() -> Self;
    /// Record one produced engine frame (called after FrameBuilder::build).
    pub fn record(&mut self, frame: &EngineFrame);   // +1 produced; += runtime_steps_executed; += skipped as 0/1
    pub fn engine_frames_produced(&self) -> u64;
    pub fn sim_ticks_advanced(&self) -> u64;
    pub fn frames_skipped(&self) -> u64;
    pub fn snapshot(&self) -> FrameTelemetrySnapshot;
    /// Optional: emit as kernel metrics into a TelemetrySink.
    pub fn emit(&self, sink: &mut impl axiom_kernel::TelemetrySink, tick: axiom_kernel::Tick);
}

/// Plain, Copy read-model of the deterministic counters at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTelemetrySnapshot {
    pub engine_frames_produced: u64,
    pub sim_ticks_advanced: u64,
    pub render_artifacts_emitted: u64,   // filled by the axiom facade
    pub frames_skipped: u64,
    pub frontend_frames_consumed: u64,   // filled by the axiom facade from windowing
    pub present_attempts: u64,           // filled by the axiom facade from windowing
    pub present_completions: u64,        // filled by the axiom facade from windowing
}

impl FrameTelemetrySnapshot {
    pub fn produced_but_not_consumed(&self) -> u64;   // saturating_sub
    pub fn consumed_but_not_presented(&self) -> u64;  // saturating_sub
}
```

> An optional `FrameTelemetryEvent` enum (`FrameProduced`, `TicksAdvanced(u32)`,
> `RenderArtifactEmitted`, `FrameConsumed`, `PresentAttempt`, `PresentCompletion`,
> `FrameSkipped`) can be introduced if a single event stream is later preferred,
> but the boring path is the direct counters above — each boundary owns the
> counters it can legally see, and the facade reads them. Do not add the event
> enum unless it removes duplication.

### 2. Frontend/present counters — `modules/axiom-windowing/src/windowing_api.rs`

Extend the existing `WindowingApi` (it already owns `frames_driven`):

```rust
impl WindowingApi {
    pub fn frames_consumed(&self) -> u64;     // surface existing frames_driven
    pub fn present_attempts(&self) -> u64;     // new field, ++ before LiveBackend::present
    pub fn present_completions(&self) -> u64;  // new field, ++ on backend present success
}
```

Backend-abstract counts stay backend-neutral: `present_attempts` /
`present_completions` are incremented by the windowing run loop from the
`bool`/`FrameSubmissionReport` the backend already returns. `draw_attempts` for
the live path can be a backend counter mirroring `GpuSubmissionReport::draw_count`.

### 3. The aggregation boundary — `modules/axiom/src/app.rs`

```rust
impl RunningApp {
    /// Deterministic engine-produce side telemetry (frame + render counts).
    pub fn frame_telemetry(&self) -> FrameTelemetrySnapshot;
}
```

`RunningApp` owns a `FrameTelemetry` + a `render_artifacts_emitted: u64`, updates
both in `tick*` (after `FrameBuilder::build` and after `RenderPipelineApi::
submit`). `App::run`'s wasm closure merges `running.frame_telemetry()` with the
`WindowingApi` consume/present counters into the full `FrameTelemetrySnapshot`.

### 4. Wall-clock rates — app / dev-harness ONLY (e.g. `apps/<app>/src/web.rs`, or a dev-harness `tool`)

```rust
/// Rolling rates over a wall-clock window. Reads platform time (performance.now /
/// Instant). NEVER compiled into a layer or engine module.
#[derive(Debug, Clone, Copy)]
pub struct FramePacingWindow {
    window_nanos: u64,
    // last sampled counters + last wall-clock timestamp + accumulators
}

impl FramePacingWindow {
    pub fn new(window_nanos: u64) -> Self;
    /// Feed the latest deterministic snapshot + the current wall-clock nanos.
    pub fn sample(&mut self, snapshot: FrameTelemetrySnapshot, now_nanos: u64) -> HostFrameStats;
}

#[derive(Debug, Clone, Copy)]
pub struct HostFrameStats {
    pub rolling_engine_output_fps: f32,
    pub rolling_frontend_consume_fps: f32,
    pub rolling_present_fps: f32,
    pub produced_but_not_consumed: u64,
    pub consumed_but_not_presented: u64,
}
```

`now_nanos` is the **only** wall-clock input and it is supplied by the caller
(`web_sys::window().performance().now()` in a browser app; `std::time::Instant`
in a native dev-harness) — `FramePacingWindow` itself takes time as a parameter,
so even it contains no ambient clock read and could live in a `tool` and be
unit-tested deterministically.

### Optional clarity newtypes

`ProducedFrameIndex(u64)` and `ConsumedFrameIndex(u64)` may wrap the produce/
consume cursors for type safety, but `EngineFrame::engine_frame_index` and
`WindowingApi::frames_consumed` already serve; add the newtypes only if call
sites confuse the two.

---

## Forbidden placements

- **`axiom-kernel`** — no wall-clock FPS, no rate computation, no `performance`/
  `Instant`/`SystemTime`. The kernel may *define* `TelemetryMetric` / `MetricKind::
  Counter` (it already does) and those are fair game for deterministic counts, but
  the kernel must not compute or store a rate, and must not gain a frame-pacing
  type. (`tests/architecture.rs::lib_exports_are_curated_set` guards its surface.)
- **`axiom-runtime`, `axiom-math`, and any other non-host, non-app layer** — no
  browser timing APIs, no FPS, no `requestAnimationFrame`/`performance.now`. They
  may carry deterministic counters only (runtime already has `RuntimeDiagnostics`).
- **`axiom-scene` / `axiom-resources` / `axiom-render` / `axiom-webgpu`
  (isolated engine modules)** — no wall-clock, and **no Canvas2D- or wgpu-specific
  counters**. Backend draw/present counts must be *abstract* event counters
  (`GpuSubmissionReport::draw_count` is already abstract — keep it that way). These
  modules also cannot import each other, so cross-cutting telemetry cannot live
  here.
- **Deterministic render-core logic** must not gain Canvas2D-specific fields; a
  "draw attempt" is an abstract backend event so the same counter serves the
  WebGPU and Canvas2D backends later.
- **`FrameTelemetry` (deterministic) must not read a wall clock** — if it ever
  needs "time," that is the signal it has crossed into the host/app layer.
- **No `PerformanceManager`/`utils`/`helpers`/`metrics_common` junk drawer.**
  Counters live with the boundary that produces the event; the snapshot is a plain
  `Copy` struct; rates live in a named `FramePacingWindow` in app/dev-harness.
- **Wall-clock rate must not live in `axiom-windowing`** even though it is
  platform-facing — it is a *module*, and the constraint is "rates only in
  host/app/dev-harness." Windowing owns the deterministic consume/present
  **counts**; the app turns those into rates.
- **No module may aggregate another module's counters** (module-to-module imports
  are illegal). The unified snapshot is assembled only in the `axiom` feature
  module (which legally composes both) or in an app.

---

## Implementation checklist

1. **`axiom-frame`**: add `frame_telemetry.rs` with `FrameTelemetry` +
   `FrameTelemetrySnapshot`; re-export through `FrameApi`'s facade surface (respect
   the layer's single-facade + curated-export rules). Keep it branchless and 100%
   covered (it is spine).
2. **`axiom-frame`**: wire `FrameTelemetry::record(&EngineFrame)` to read
   `engine_frame_index`, `timing().runtime_steps_executed()`, `timing().skipped()`.
   Do **not** make `FrameBuilder` read a clock.
3. **`axiom-windowing`**: surface `frames_consumed()` (existing `frames_driven`);
   add `present_attempts` / `present_completions` fields incremented in the run
   loop around `LiveBackend::present` and from the backend return values. Keep the
   deterministic core fully covered; the wasm32 arm stays the platform arm.
4. **`axiom` facade** (`modules/axiom`): give `RunningApp` a `FrameTelemetry` +
   `render_artifacts_emitted`, update them in `tick*` after build/submit, and add
   `RunningApp::frame_telemetry() -> FrameTelemetrySnapshot`; merge windowing
   counts where `App::run` wires the loop.
5. **App / dev-harness**: add `FramePacingWindow` + `HostFrameStats` in a browser
   app's `src/web.rs` (start with `apps/axiom-stress-cubes-browser`, replacing the
   `index.html` JS FPS loop with a Rust one fed by `performance.now()`), or in a
   `tool` dev-harness for native. This is the only place wall-clock time is read.
6. Leave the isolated render modules untouched except to *read* existing abstract
   counts (`GpuSubmissionReport::draw_count`).

Tests to add:

- **Deterministic counter exactness** (`axiom-frame`): building N frames yields
  `engine_frames_produced == N` and `sim_ticks_advanced == Σ runtime_steps_executed`;
  exactly one increment per produced frame.
- **Consumed < produced** is representable: a test that drives `engine_frames_
  produced` ahead of `frontend_frames_consumed` and asserts `produced_but_not_
  consumed() > 0` (and `== 0` in the current 1:1 loop).
- **Dropped/skip derivation**: `produced_but_not_consumed` and `consumed_but_not_
  presented` equal the saturating counter differences; a skipped frame
  (`FrameTiming::skipped`) bumps `frames_skipped` and not `sim_ticks_advanced`.
- **No wall clock in deterministic telemetry**: an `axiom-frame` architecture/source
  test asserting `frame_telemetry.rs` contains no `Instant`/`SystemTime`/`std::time`/
  `performance`/`Date` (mirror the existing kernel/frame wall-clock scans).
- **Wall-clock isolation**: a test/assertion that `FramePacingWindow` takes time as
  a parameter (so it is deterministic in a unit test) and that no layer/module
  source references `performance.now`/`Performance` (the existing hygiene scans in
  `crates/xtask/src/hygiene.rs` already forbid browser APIs outside host/windowing;
  extend coverage to the new rate type's location).
- **Backend separation**: a `axiom-canvas2d-backend` test that draw attempts are
  reported separately from engine production (a Canvas2D `present_packet` reports
  its own draw/present counts independent of `engine_frames_produced`).
- **Backend parity**: assert the live GPU path and the Canvas2D path both feed the
  same abstract `present_attempts`/`present_completions` counters, so a future
  WebGPU/backend swap reuses them unchanged.
- **Architecture stays green**: `cargo xtask check-architecture` passes (no new
  cross-module imports, no app imported by engine, single-facade preserved).

---

## Validation commands

```sh
cargo test --workspace
cargo xtask check-architecture
```
