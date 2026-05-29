# Axiom Frame — Testing Discipline

Layer 04 publishes the engine's canonical per-frame contract. Once
renderer / scene / picking / animation layers depend on it, any
nondeterminism here is a multiplier on every consumer. The tests have
to prove the contract is deterministic — same inputs in, same
`EngineFrame` and same `FrameContext` out, every run, every platform.

## Every public frame concept needs direct tests

A "public concept" is any item reachable from outside the frame crate.
That is:

- every method on `FrameApi`,
- every constructor and method on the curated lib.rs exports
  (`EngineFrame`, `FrameBuilder`, `FrameCommand`, `FrameCommandQueue`,
  `FrameContext`, `FrameDiagnostics`, `FrameError`, `FrameErrorCode`,
  `FrameLifecycleState`, `FrameResult`, `FrameStepSummary`,
  `FrameTiming`, `FrameViewport`).

The rule:

> **If a public item is not directly unit-tested, it is removed.**

Tests that only prove "this method does not panic" are not enough.
Every test asserts a deterministic outcome — a value, a specific error
code, an equality between two engine frames, a lifecycle state, a step
count.

## Deterministic frame construction tests

`FrameBuilder` is tested with **fixed integer inputs**: a synthesized
[`HostFrameReport`] built from explicit `HostFrameInput` (sequence,
elapsed nanos, viewport) and a fixed [`HostBoundaryConfig`]. The same
sequence of `build` calls in two separate `make()` closures is asserted
equal (`repeated_builder_use_with_identical_input_is_deterministic`).

## Host report adaptation tests

`EngineFrame::new` and `FrameBuilder::build` are tested to prove:

- the engine frame is built from a host frame report (every input on
  the constructor flows through);
- the host frame sequence is preserved;
- the runtime step count is preserved;
- the ordered runtime step summaries are preserved;
- the viewport snapshot is preserved;
- the lifecycle state is preserved;
- a skipped host frame produces a skipped engine frame
  (`is_skipped == true`, `runtime_step_count == 0`, `lifecycle ==
  Hidden`/`Suspended`/`ShutdownRequested` as appropriate);
- identical inputs produce equal frames.

## Viewport snapshot tests

`FrameViewport::from_host` is tested for:

- values copied verbatim from the host viewport;
- aspect ratio matching `physical_width / physical_height`;
- aspect ratio stable across constructions;
- finite aspect passing `MathApi::is_finite_value`;
- identical inputs producing equal snapshots;
- different host viewports producing different snapshots;
- the `InvalidViewport` error code being distinct (its shape is pinned
  via the shorthand constructor since fabricating a non-finite host
  viewport is impossible — Layer 03 rejects it first).

## Timing summary tests

`FrameTiming::from_host_report` covers every deterministic branch:

- exact one-step timing;
- multi-step timing;
- max-step clamped timing (with retained-nanos carryover);
- retain-accumulator timing (carryover for under-budget frames);
- skipped frame timing (`skipped == true`, zero steps, zero consumed);
- identical inputs producing equal timings;
- a `steps_executed`/`plan.steps` mismatch failing with
  `InvalidFrameTiming`.

## Runtime step summary tests

`FrameStepSummary::from_record` and `list_from_records` are tested
through real `HostStepDriver` runs so the summaries flow through the
genuine runtime/host integration:

- summaries preserve runtime step order (ticks `1, 2, 3`);
- summaries preserve frame index / tick / sequence identity;
- empty record slices produce empty summary lists;
- identical runtime sequences produce identical summary lists;
- a system failure (with `fail_on_system_error = false`) propagates
  `succeeded() == false` into the summary.

## Lifecycle state tests

`FrameLifecycleState::from_host` is tested for:

- `Active` from a visible host;
- `Hidden` from an invisible host;
- `Suspended` from a suspended host;
- `ShutdownRequested` from a shutting-down host;
- the precedence rule (`ShutdownRequested` > `Suspended` > `Hidden` >
  `Active`);
- deterministic mapping for equal host states.

## Frame command queue tests

`FrameCommandQueue` and `FrameCommand` are tested for:

- a fresh queue starts empty with `next_sequence == 1`;
- `push` returns monotonic sequence numbers;
- `drain` returns commands in FIFO order;
- assigned sequence IDs are preserved through `drain`;
- `clear` empties the queue;
- identical insertion sequences produce identical `drain` output;
- the internal counter is not reset by `drain` or `clear`;
- `Default` matches `new`;
- `FrameCommand` equality requires all three fields;
- empty payloads are supported.

## Diagnostics tests

`FrameDiagnostics::new` is tested for:

- accessors round-tripping constructed values;
- skipped diagnostics being explicit (`skipped == true`,
  `skip_reason == Some(LifecycleHidden)`);
- identical inputs producing identical diagnostics;
- `validation_failure_count` being independent;
- `command_count` being preserved.

`FrameBuilder` also asserts the built diagnostics reflect the command
count it was given.

## Error path tests

Every frame error code has a direct test:

- `InvalidEngineFrameSequence` — pinned via the shorthand constructor
  (the builder's monotonic counter prevents the builder itself from
  emitting one, but the code shape and identity are tested).
- `InvalidHostFrameSequence` — the builder emits this when two
  consecutive reports have non-increasing sequence numbers, and
  `FrameApi::validate_host_frame_transition` emits the same code for
  equal or decreasing values.
- `InvalidFrameTiming` — a `steps_executed`/`plan.steps` mismatch
  triggers this both directly (in `FrameTiming`) and through the
  builder.
- `InvalidViewport` — the error shape is pinned; the constructor's
  only real failure path is the math finite check, which is exercised
  indirectly through the viewport tests.
- `HostFrameAdaptationFailed` — wraps a `HostError`; the round-trip is
  tested via `FrameError::with_host` and `host_frame_adaptation_failed`.

## Logging / telemetry determinism

Layer 04 deliberately ships **zero** ambient logging or telemetry today.
The runtime emits diagnostics through its own kernel sinks; the host
boundary records explicit metrics through `MathApi`. The frame layer's
own diagnostic surface is the typed `FrameDiagnostics` struct, which is
plain data with no IO. If a future iteration adds frame-level
telemetry it must follow the same rule as math/host: counts only,
tick-stamped, routed through `KernelApi` and `TelemetrySink`, and
proven deterministic with a counter-equality test.

## Architecture / boundary tests

`tests/architecture.rs` mechanically enforces the boundary rules by
scanning the source tree (comments and string literals stripped). It
asserts:

- no `web_sys`, `js_sys`, `wasm_bindgen`, `wasm-bindgen` references;
- no DOM / canvas / browser globals;
- no `wgpu`, `webgpu`, `WebGL`, `webgl`, `GPUDevice`;
- no `requestAnimationFrame`, no `performance.now`;
- no `std::time`, `SystemTime`, `Instant::now`, `chrono`;
- no randomness (`rand::`, `thread_rng`, `getrandom`, `fastrand`);
- no console output or placeholder macros (`println!`, `eprintln!`,
  `print!`, `eprint!`, `dbg!`, `todo!`, `unimplemented!`);
- no global mutable state (`static mut`, `lazy_static`);
- no renderer / shader / material / mesh / texture / swapchain /
  render-graph symbols;
- no higher-engine-layer symbols (`World`, `Scene`, `Asset`,
  `Physics`, `Animator`, `Audio`, `KeyCode`, `MouseButton`, `Gamepad`,
  `Plugin`, `EditorPanel`, `GameLoop`, `rapier`, `winit`, `egui`,
  `bevy`);
- no `utils`, `helpers`, `common`, or `misc` modules;
- `lib.rs` exports exactly the curated fourteen-item set;
- `axiom-kernel`, `axiom-runtime`, `axiom-math`, and `axiom-host` do
  not import `axiom_frame`;
- `axiom-frame` imports only `axiom_kernel`, `axiom_runtime`,
  `axiom_math`, and `axiom_host`.

`tests/manifest.rs` proves the layer-manifest contract:

- the frame manifest validates with index 4, the four legal
  dependencies (kernel, runtime, math, host), and the nine documented
  capabilities;
- host (the immediate previous layer) is accepted as a dependency on
  its own;
- depending on itself is rejected as `SelfImport`;
- depending on a future layer (index 5+) is rejected as
  `ForwardImport`;
- duplicate dependencies and duplicate capabilities are both
  rejected.

In addition, `cargo xtask check-architecture` (and the workspace's
`real_repo_layers_pass` test that wraps it) validates the real
`crates/axiom-frame/layer.toml` against the Axiom Layer Law on every
workspace test run, so the manifest on disk cannot drift from the code.
