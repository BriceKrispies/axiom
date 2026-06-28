# Axiom Host — Architecture

`axiom-host` is the deterministic platform/host boundary of the Axiom
engine. It depends on kernel and runtime:

```
axiom-host depends on:
  axiom-kernel   (time, identity, errors, binary, logging, telemetry)
  axiom-runtime  (lifecycle, fixed-step scheduling, queues, context)
```

## What axiom-host is

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

The crate's public surface is curated through `lib.rs`. The stepping
boundary is:

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

The **presentation boundary** (added for the second-pass browser-visible
slice) adds:

- `HostPresentationTarget` — the abstract thing the engine presents into
  (kernel `HandleId` identity + deterministic label).
- `HostSurfaceHandle` — an opaque kernel-`HandleId` handle to a future live
  surface.
- `HostSurfaceDescriptor` — the requested surface shape (a validated
  `HostViewport` + abstract present/alpha/colour enums).
- `HostPresentMode` / `HostAlphaMode` / `HostColorFormat` — abstract surface
  enums.
- `HostAdapterRequest` / `HostPowerPreference` — the abstract adapter request.
- `HostDeviceRequest` / `HostDeviceProfile` — the abstract device request.
- `HostPresentationRequest` — a validated binding of target + surface +
  descriptor + adapter/device requests.
- `HostPresentationReport` / `HostPresentationStatus` — the deterministic
  evaluation result.

The curated set (every `pub use` in `lib.rs`) is locked by
`tests/architecture.rs::lib_exports_are_curated_set`; widening it requires
editing both `lib.rs` and that test together.

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
`axiom-browser` adapter crate (not yet built) will translate
browser events into the deterministic data types this layer defines and
hand them to a `HostStepDriver`. The opposite arrangement — making the
host layer itself depend on the browser — would force every replay test,
every native build, every headless harness, and every console use to drag
in browser machinery they do not need, and would make determinism a
runtime accident rather than a structural guarantee.

The same reasoning applies to `winit`, `sdl2`, GLFW, and friends. None of
them appear here.

## What nondeterminism is allowed to enter, and how

Host is the *only* layer that accepts inputs an engine outside of it
generated:

- **Wall-clock time** enters only as `HostFrameInput::elapsed_nanos`
  (`u64`) and the optional `HostFrameInput::presentation_nanos`. The host
  adapter is responsible for measuring time; this layer never does. The
  type system rules out negative elapsed times.
- **Surface size and scale** enter only through `HostViewport::new` /
  `HostViewport::from_physical`. Logical and physical dimensions are
  non-zero `u32`s; the scale factor arrives as the kernel
  [`axiom_kernel::Ratio`] quantity type, which guarantees finiteness at
  construction, and the viewport additionally enforces positivity.
- **Lifecycle and visibility** enter only as `HostLifecycleSignal`
  values. The signal alphabet is closed: any future signal requires a
  host-layer change. Keyboard, mouse, touch, gamepad, and any other input
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
input layer will accept its own input events as data, in the
same deterministic style, and route them through the runtime context.

## How it consumes `axiom-runtime` and `axiom-kernel`

The host no longer depends on `axiom-math`. Viewport finiteness is now a
property of the kernel `Ratio` quantity type:

- `HostViewport::new` / `from_physical` take a `scale_factor: Ratio`.
  `Ratio::new` is the only way to build one and it rejects NaN / ±inf, so
  a non-finite scale can no longer reach the host at all. The viewport
  enforces only positivity and the dimension invariants on top.
- `HostViewport::aspect_ratio` returns a `Ratio` (`physical_width /
  physical_height`); both dimensions are validated non-zero, so the
  quotient is provably finite and the `Ratio::new` invariant holds by
  construction.

Runtime is the layer the host *drives*, and is a depended layer the
host adapts:

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

## The host presentation boundary

The first-pass vertical slice (`apps/axiom-demo-rotating-cube`) runs the
whole `frame → … → GpuSubmission → GpuSubmissionReport` pipeline
deterministically, but `axiom-webgpu` stays in **Recording** mode: it
captures a deterministic submission report instead of presenting pixels.
The blocker that pass found was structural — there was no host-owned way to
*describe* a presentation target, a surface, or an adapter/device request.
This boundary fills exactly that gap and nothing more.

### What it is

A set of deterministic, host-owned **data** types and **opaque stable
handles** that describe *what the engine wants to present into and with*:

```
HostPresentationTarget   abstract target the engine presents into (HandleId + label)
HostSurfaceHandle        opaque handle to a future live surface (HandleId)
HostSurfaceDescriptor    requested surface shape (HostViewport + present/alpha/colour)
HostAdapterRequest       requested adapter (power preference + needs-presentation)
HostDeviceRequest        requested device (needs-presentation + coarse profile)
HostPresentationRequest  validated binding of all of the above
HostPresentationReport   deterministic evaluation result (status PendingBackend)
```

Everything is built and validated through `HostApi`:
`presentation_target`, `surface_handle`, `surface_descriptor`,
`adapter_request`, `device_request`, `presentation_request`, and
`evaluate_presentation`. The two identity-bearing types
(`HostPresentationTarget`, `HostSurfaceHandle`) have crate-private
constructors, so their kernel `HandleId` identity is always minted by the
host boundary — never forged elsewhere.

### What it intentionally does not implement

- **No real adapter/device/surface acquisition.** Nothing here calls
  `navigator.gpu`, requests an adapter, awaits a device, or configures a
  swapchain. There is no async, no threads, no wall-clock, no randomness.
- **No live presentation.** `evaluate_presentation` always returns
  `HostPresentationStatus::PendingBackend`. The boundary records that
  presentation was *structurally requested and validated*; it never claims
  a GPU, adapter, device, or surface actually exists. `Ready` is
  unreachable until the live pass binds a real backend.
- **No renderer concepts.** No pipelines, shaders, meshes, materials, or
  swapchain objects — those belong to the render/webgpu modules, not the
  host boundary.

### Why it contains no browser/DOM/WebGPU objects

The same structural reason the rest of the layer avoids them: a presentation
*description* that embedded a `web_sys::HtmlCanvasElement`, a
`wgpu::Surface`, or a `GPUDevice` would drag browser/GPU machinery into
every replay test, native build, and headless harness, and would make the
boundary non-deterministic and non-`Copy`. Instead, identity is a kernel
`HandleId` and shape is validated host data. The architecture tests scan
the source for `web_sys`/`js_sys`/`wasm_bindgen`/`wgpu`/`webgpu`/`WebGL`/
`GPUDevice`/DOM globals and fail the build if any appear — the new
presentation files are covered by those same scans.

### How future wasm/browser code binds a real surface

A future `axiom-browser` (or native) adapter — built *on top of* this layer,
never inside it — will:

1. Mint a `HostPresentationTarget` and a `HostSurfaceHandle` from `HostApi`,
   choosing the raw ids (e.g. `1` for the primary canvas).
2. Out-of-band, create the real browser surface (a `<canvas>` +
   `wgpu::Surface`, or a native window surface) and store it in the
   adapter's own table, keyed by the handle's `HandleId`. The host layer
   never sees that object.
3. Build a `HostSurfaceDescriptor` from a validated `HostViewport` plus the
   abstract present/alpha/colour enums, and a `HostPresentationRequest`
   binding the target, surface handle, descriptor, and adapter/device
   requests.

The `HandleId` is the stable join key between the deterministic engine side
and the adapter's nondeterministic real-surface table.

### How a future `axiom-webgpu` live mode consumes the request

`axiom-webgpu` is untouched by this pass (it remains Recording-only). When
the live pass arrives, a `BackendKind::Live` arm will accept a
`HostPresentationRequest` and:

- look up the real surface bound to `request.surface()`'s `HandleId`,
- translate `request.adapter()` / `request.device()` into the backend's real
  adapter/device requests,
- configure the surface from `request.descriptor()` (viewport size, present
  mode, alpha mode, colour format),

…all **without changing** the existing `GpuSubmission` / `GpuSubmissionReport`
shapes. The recorder and the live backend will present the same module
surface; only `submit()`'s body differs.

### Why `axiom-webgpu` is still Recording-only after this pass

This pass only adds the host-side *description* of presentation. There is
still no live backend, no real surface, no device, and no async device-init
path in any layer. `axiom-webgpu` therefore continues to record submissions
deterministically. Binding a real surface to a `HostSurfaceHandle` and
driving real `wgpu` calls is the next pass's job.

### What remains for the live WebGPU pass

- A browser/native **adapter crate** (a new app/adapter, not a layer or
  module) that owns the real canvas/window and the `HandleId → real surface`
  table, plus the `wasm-bindgen`/`web_sys` glue. None of that lives here.
- An **async device-init path**. `wgpu`'s `Instance → Adapter → Device →
  Queue` is async; the runtime/host stepping is synchronous. The live pass
  must bridge the async acquisition to a point where a `HostSurfaceHandle`
  becomes backed, and only then can `HostPresentationStatus::Ready` be
  produced.
- A `BackendKind::Live` arm in `axiom-webgpu::WebGpuApi::submit` that
  consumes a `HostPresentationRequest` and presents to the bound surface.
