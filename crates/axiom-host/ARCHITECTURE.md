# Axiom Host — Layer 03 Architecture

`axiom-host` is the deterministic platform/host boundary of the Axiom
engine. It sits fourth in the chain:

```
Layer 00  axiom-kernel    (time, identity, errors, binary, logging, telemetry)
Layer 01  axiom-runtime   (lifecycle, fixed-step scheduling, queues, context)
Layer 02  axiom-math      (scalars, vectors, matrices, transforms, geometry)
Layer 03  axiom-host      ← this crate
```

## What Layer 03 is

The boundary between deterministic engine code and the world outside it.

- It **acknowledges** that an external host exists (a browser tab, a
  desktop window, a native shell, a headless harness) and defines the
  deterministic contracts that host has to supply data through.
- It **validates** every piece of host-supplied data and surfaces a
  machine-stable `HostError` on rejection.
- It **adapts** validated host data into deterministic runtime stepping
  plans (`HostStepPlan`), executes those plans against a borrowed
  [`axiom_runtime::Runtime`] via [`HostStepDriver`], and emits
  per-frame [`HostFrameReport`]s.

The crate ships exactly thirteen public items, all curated through
`lib.rs`:

- `HostApi` — the facade.
- `HostViewport` — validated viewport / surface metadata.
- `HostFrameInput` — one externally-supplied host frame pulse.
- `HostLifecycleSignal` / `HostLifecycleState` — coarse lifecycle facts and
  their deterministic projection.
- `HostBoundaryConfig` — fixed step + catch-up policy + accumulator policy.
- `HostStepPlan` / `HostSkipReason` — the deterministic per-frame plan.
- `HostStepDriver` — the only thing in this layer that calls
  `Runtime::step`.
- `HostFrameReport` — the post-frame summary.
- `HostError` / `HostErrorCode` / `HostResult` — the error model.

## Why it exists

Every layer below this one is fully deterministic and self-contained. The
moment a future browser adapter wires up `requestAnimationFrame`, a future
native adapter calls `winit`, or a future headless harness reads JSON
input, the engine has to accept time, lifecycle, and surface facts from
*outside*. Putting a Rust type system between that data and the runtime
gives us:

- **One choke point.** Every external timing value flows through one
  `HostFrameInput` constructor, every external surface fact through one
  `HostViewport` constructor. Validation, error codes, and replay all
  live in one place.
- **A typed, replay-friendly substrate for future adapters.** A browser
  adapter, a native adapter, and a headless test all build *the same*
  `HostFrameInput`s. The engine cannot tell them apart, and the test
  harness can drive identical sequences.
- **A clear stop sign for higher layers.** Renderer / scene / picking /
  input layers built on top of this one do not get to invent their own
  lifecycle policies or their own clock model — they inherit the
  deterministic ones that already exist here.

## Why it is the host boundary but not the browser adapter

This layer never compiles in a browser binding. It uses no `web_sys`, no
`js_sys`, no `wasm_bindgen`, no `wgpu`, no `winit`. A future
`axiom-browser` adapter crate (Layer 04+ — not yet built) will translate
browser events into the deterministic data types this layer defines and
hand them to a `HostStepDriver`. The opposite arrangement — making the
host layer itself depend on the browser — would force every replay test,
every native build, every headless harness, and every console use to drag
in browser machinery they do not need, and would make determinism a
runtime accident rather than a structural guarantee.

The same reasoning applies to `winit`, `sdl2`, GLFW, and friends. None of
them appear here.

## What nondeterminism is allowed to enter, and how

Layer 03 is the *only* layer that accepts inputs an engine outside of it
generated:

- **Wall-clock time** enters only as `HostFrameInput::elapsed_nanos`
  (`u64`) and the optional `HostFrameInput::presentation_nanos`. The host
  adapter is responsible for measuring time; this layer never does. The
  type system rules out negative elapsed times.
- **Surface size and scale** enter only through `HostViewport::new` /
  `HostViewport::from_physical`. Logical and physical dimensions are
  non-zero `u32`s; the scale factor is validated as a finite positive
  `f32` via [`axiom_math::MathApi::validate_finite`].
- **Lifecycle and visibility** enter only as `HostLifecycleSignal`
  values. The signal alphabet is closed: any future signal requires a
  layer-03 change. Keyboard, mouse, touch, gamepad, and any other input
  mapping are deliberately out of scope.

Once data is inside the layer, it is plain values — `Copy`/`Clone` where
possible, equality by value — and every downstream operation is
deterministic.

## Why this layer does not call browser APIs

The layer compiles to a pure Rust `rlib`. It has no `wasm-bindgen`
dependency, no JS interop, no DOM/canvas interop. The architecture tests
(`tests/architecture.rs`) scan the source tree for `web_sys`, `js_sys`,
`wasm_bindgen`, `wgpu`, `webgl`, `requestAnimationFrame`,
`performance.now`, `std::time`, `Instant`, `SystemTime`, `chrono`, and
randomness, and fail the build if any of them appear.

## Why this layer does not render

Rendering needs a backend (`wgpu` or `webgl2`), a swapchain, a
descriptor model, a shader compiler, and a frame-graph. None of that
belongs at the host boundary — it belongs in a dedicated renderer layer
built on top of this one (and on top of math). Letting renderer code
leak in here would:

- pull WebGPU/WebGL into every replay test, native build, and headless
  harness;
- force the host boundary to know what a "draw" means before any scene
  graph or material system exists; and
- couple lifecycle policy to renderer state, which is exactly the
  coupling we are paying for this layer to prevent.

## Why this layer does not own input mapping

Input is its own concern: keymaps, modifier semantics, edge/level
detection, repeat handling, IME composition, gamepad axis filtering,
touch gesture recognition. None of that is host-boundary work. The
lifecycle alphabet in `HostLifecycleSignal` is intentionally coarse —
visibility, focus, suspension, shutdown — because those are the facts
the engine boundary needs to decide whether to step at all. A future
input layer (Layer N) will accept its own input events as data, in the
same deterministic style, and route them through the runtime context.

## How it consumes `axiom-math` and `axiom-runtime`

Math is the immediate previous layer, and the host layer is a
**semantic adapter over math**:

- `HostViewport::new` calls `MathApi::validate_finite` on the scale
  factor. The kernel does not have a finite-`f32` policy; math does.
  Calling math here is what makes `HostViewport` enforce the engine-wide
  scalar discipline.
- `HostStepDriver` carries a `MathApi` and re-checks the viewport scale
  factor on every `drive` call as defence in depth. The architecture
  checker's proof exports for `HostApi` and `HostStepDriver` lock this
  usage in.

Runtime is the layer the host *drives*:

- `HostBoundaryConfig::validate` calls `KernelApi::fixed_step` (via the
  same path `RuntimeConfig::validate` uses) so the host and runtime
  cannot disagree on what a valid fixed step is.
- `HostStepDriver::drive` calls `Runtime::step` exactly as many times as
  the plan asks for. A `RuntimeError` is preserved verbatim inside the
  resulting `HostError::RuntimeStepFailed`.
- The driver collects `RuntimeStepRecord`s in order and embeds them in
  `HostFrameReport::step_records`.

The host layer does not own a runtime: the driver borrows one. That
keeps replay tests, multi-runtime harnesses, and future
host-adapter-per-window topologies straightforward.

## How future browser/native adapters should consume `HostApi`

A future adapter (e.g. a `axiom-browser` crate) will:

1. Construct a `HostApi` once and a `HostBoundaryConfig` from values it
   reads at startup, validating the latter via
   `HostApi::validate_boundary_config(&config, &kernel)`.
2. Build a `HostStepDriver` from that config.
3. On every host frame:
   - apply any new `HostLifecycleSignal`s to the driver,
   - construct a `HostFrameInput` from the explicit timing values it
     measured (or received from the host) and the current `HostViewport`,
   - call `driver.drive(&mut runtime, input)` to get a `HostFrameReport`.
4. Hand the report (and its embedded `RuntimeStepRecord`s) to whatever
   higher-layer system needs them — a renderer, a debug overlay, a
   replay sink, a test harness.

The adapter never reaches around the host layer to call `Runtime::step`
itself, and it never invents its own viewport or lifecycle types. If a
future adapter needs a fact the host layer does not yet expose, the
correct response is to add it to this layer, not to bypass it.
