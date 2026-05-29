# Axiom Host — Testing Discipline

Layer 03 is the boundary between deterministic engine code and the
outside world. Its tests have to prove that the boundary itself is
deterministic — same data in, same plan and same report out, every run,
every platform.

## Every public host concept needs direct tests

A "public concept" is any item that can be reached from outside the
host crate. In practice that is:

- every method on `HostApi`,
- every constructor and method on the curated lib.rs exports
  (`HostViewport`, `HostFrameInput`, `HostLifecycleSignal`,
  `HostLifecycleState`, `HostBoundaryConfig`, `HostStepPlan`,
  `HostSkipReason`, `HostStepDriver`, `HostFrameReport`, `HostError`,
  `HostErrorCode`, `HostResult`).

The rule:

> **If a public item is not directly unit-tested, it is removed.**

A test that only proves "this method does not panic" is not enough.
Every test must assert a deterministic outcome — a value, a specific
error code, an equality, a lifecycle state, a step count.

## Deterministic host frame tests

Every test that drives the boundary uses **fixed integer inputs**:

- the host sequence is an explicit `u64`,
- the elapsed time is an explicit `u64` of nanoseconds,
- the viewport is rebuilt from explicit dimensions and an explicit scale
  factor.

`HostStepDriver::drive` is exercised with multiple frames whose inputs
are byte-identical between two runs in the same test
(`driver_is_value_for_value_deterministic_across_identical_input_sequences`).
The assertion is on a tuple of `(accumulator_nanos, last_tick)` — two
values that change with every state mutation, so any nondeterminism
shows up as a test failure rather than being silently masked.

## Viewport validation tests

`HostViewport::new` and `HostViewport::from_physical` are tested for:

- **happy path** — a valid logical/physical size and a finite positive
  scale factor produce a viewport whose accessors return the supplied
  values.
- **zero logical width** — rejected with
  `HostErrorCode::InvalidViewportDimensions`.
- **zero logical height** — rejected with
  `HostErrorCode::InvalidViewportDimensions`.
- **zero physical width / height** — rejected on `from_physical`.
- **negative scale factor** — rejected with
  `HostErrorCode::InvalidScaleFactor`.
- **zero scale factor** — rejected with
  `HostErrorCode::InvalidScaleFactor`.
- **NaN scale factor** — rejected with
  `HostErrorCode::InvalidScaleFactor`.
- **infinity scale factor** — rejected with
  `HostErrorCode::InvalidScaleFactor`.
- **aspect ratio stability** — two viewports with identical inputs
  produce identical aspect ratios.
- **deterministic conversions** — `logical_to_physical` /
  `physical_to_logical` are byte-identical across calls.

## Lifecycle transition tests

`HostLifecycleState` tests prove:

- the initial state is quiescent,
- each `HostLifecycleSignal` transitions the right field,
- the `ShutdownRequested` signal is sticky (cannot be undone by later
  signals — preserves replay determinism),
- `allows_stepping` blocks hidden state unless the policy opts in,
  blocks suspended state unconditionally, and blocks shutdown
  unconditionally.

Lifecycle signal ordering is preserved when queued
(`HostLifecycleSignal::ordering_is_preserved_when_queued`).

## Step planning tests

`HostStepPlan::build` covers every deterministic branch:

- exact one-step frame,
- multi-step catch-up frame,
- max-step clamp (`max_steps_per_frame` ceiling),
- `retain_accumulator = true` carries unspent time forward,
- `retain_accumulator = false` discards leftover slack,
- hidden frame skipped with `HostSkipReason::LifecycleHidden`,
- hidden frame stepped when `step_while_hidden = true`,
- suspended frame skipped unconditionally,
- shutdown frame skipped and accumulator dropped,
- identical inputs produce identical plans.

## Runtime driver tests

`HostStepDriver::drive` tests:

- exact step count matches the plan,
- step records arrive in tick order (`1, 2, 3` for a three-step
  catch-up),
- out-of-order frame sequence rejected with
  `HostErrorCode::InvalidFrameSequence`,
- equal frame sequence rejected with the same code,
- accumulator carries across frames (half a step + half a step ⇒ one
  runtime step),
- catch-up clamps to `max_steps_per_frame`,
- value-for-value determinism across two identical drive sequences,
- hidden/suspended frames produce a skipped report (zero
  `Runtime::step` calls),
- runtime step failures propagate as
  `HostErrorCode::RuntimeStepFailed` with the runtime cause preserved.

## Error path tests

Every host error code has a direct test:

- `InvalidViewportDimensions` — zero logical/physical dimensions,
  derived dimensions overflowing `u32`, or rounding to zero.
- `InvalidScaleFactor` — non-finite or non-positive scale factors,
  including on the driver's defence-in-depth re-check.
- `InvalidFrameSequence` — out-of-order or equal sequence numbers.
- `InvalidBoundaryConfig` — zero `max_steps_per_frame`, or a fixed
  step rejected by the kernel.
- `RuntimeStepFailed` — a runtime that has never been started rejects
  `step()`; the host error preserves the wrapped `RuntimeError`.
- `InvalidLifecycleTransition` — reserved for forward use; the error
  code itself is tested for stability and machine identity, and
  shorthand constructors are tested for round-tripping.

## Logging / telemetry determinism

Layer 03 deliberately ships **zero** ambient logging or telemetry today.
Diagnostics are emitted by the runtime through the kernel sinks; the
host layer surfaces them via `HostFrameReport::step_records`. If a
future iteration adds host-level telemetry it must follow the same rule
as math: counts only, tick-stamped, routed through `KernelApi` and
`TelemetrySink`, and proven deterministic with a counter-equality test.

## Architecture / boundary tests

`tests/architecture.rs` mechanically enforces the boundary rules by
scanning the source tree (comments and string literals stripped). It
asserts:

- no `web_sys`, `js_sys`, `wasm_bindgen`, `wasm-bindgen` references;
- no DOM / canvas / browser globals (`HtmlCanvas`, `document.`,
  `window.`, `navigator.`, `OffscreenCanvas`);
- no `wgpu`, `webgpu`, `WebGL`, `webgl`, `GPUDevice`;
- no `requestAnimationFrame`, no `performance.now`;
- no `std::time`, `SystemTime`, `Instant::now`, `chrono`;
- no randomness (`rand::`, `thread_rng`, `getrandom`, `fastrand`);
- no console output or placeholder macros (`println!`, `eprintln!`,
  `print!`, `eprint!`, `dbg!`, `todo!`, `unimplemented!`);
- no global mutable state (`static mut`, `lazy_static`);
- no renderer / shader / material / mesh / texture / swapchain symbols;
- no higher-engine-layer symbols (`World`, `Scene`, `Asset`, `Physics`,
  `Animator`, `Audio`, `KeyCode`, `MouseButton`, `Gamepad`, `Plugin`,
  `EditorPanel`, `GameLoop`, `rapier`, `winit`, `egui`, `bevy`);
- no `utils`, `helpers`, `common`, or `misc` modules;
- `lib.rs` exports exactly the curated thirteen-item set;
- `axiom-kernel`, `axiom-runtime`, and `axiom-math` do not import
  `axiom_host`;
- `axiom-host` imports only `axiom_kernel`, `axiom_runtime`, and
  `axiom_math`.

`tests/manifest.rs` proves the layer-manifest contract:

- the host manifest validates with index 3, the three legal
  dependencies (kernel, runtime, math), and the eight documented
  capabilities,
- math (the immediate previous layer) is accepted as a dependency
  on its own,
- depending on itself is rejected as `SelfImport`,
- depending on a future layer (index 4+) is rejected as
  `ForwardImport`,
- duplicate dependencies and duplicate capabilities are both
  rejected.

In addition, `cargo xtask check-architecture` (and the workspace's
`real_repo_layers_pass` test that wraps it) validates the real
`crates/axiom-host/layer.toml` against the Axiom Layer Law on every
workspace test run, so the manifest on disk cannot drift from the code.
