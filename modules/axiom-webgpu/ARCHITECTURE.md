# Axiom WebGPU — Module Architecture

`axiom-webgpu` is **an isolated engine module** that owns the WebGPU/wgpu
backend boundary. It routes the **one** `GpuSubmission` input contract through
one of two backend modes.

## Two backend modes, one input contract

`WebGpuApi` carries a tiny, deterministic backend state and exposes both modes
behind the same facade:

- **Recording** (`BackendKind::Recording`, the default and the proof backend).
  Every command pushed into a `GpuSubmission` is captured into a deterministic
  `GpuSubmissionReport`. **No real GPU calls are made.** This is what the
  headless rotating-cube vertical slice depends on, and its behaviour is
  byte-for-byte unchanged from before backend modes existed.
- **Live** (`BackendKind::Live`). The structural seam for real presentation. A
  live backend is built from the deterministic `axiom-host` presentation
  boundary (`HostPresentationRequest`) and accepts the **same** `GpuSubmission`
  shape, but performs **no real GPU work** in this pass.

### Why `GpuSubmission` remains the stable input contract

There is exactly **one** command model. Both backends consume the identical
`GpuSubmission` (the same `GpuCommand` stream + target dimensions). Render
output is never forked into "recording commands" and "live commands". This is
the whole point of the seam: the app builds a submission once, and swapping
`WebGpuApi::new_recording()` for `WebGpuApi::new_live(...)` changes only the
*body* of `submit()`, never the input shape, never the report shape, and never
any other crate. A future live backend that truly presents will still consume
exactly this `GpuSubmission`.

### Deterministic backend state

The module-internal state machine is intentionally tiny — three states, no
adapter/device/swapchain management:

```
Recording                     → submissions reported as Recorded
LiveUnbound                   → submissions reported as LiveNotBound
LivePresentationRequested(req)→ submissions reported as LiveNotInitialized
```

`req` is a validated `axiom_host::HostPresentationRequest`, carried so a
future live pass can build a real surface/device from host-owned data without
re-plumbing it. There is deliberately **no `LiveReady` state**, because no real
backend binding exists yet.

## What live mode does in this pass

- Is constructible from a validated `HostPresentationRequest`
  (`WebGpuApi::new_live`), or as an explicitly unbound live backend
  (`WebGpuApi::new_live_unbound`).
- Accepts the same `GpuSubmission` as recording mode.
- Produces a deterministic `GpuSubmissionReport` whose **status** explicitly
  states the outcome (`LiveNotBound` / `LiveNotInitialized`) and whose
  `presented()` is **always `false`**.
- Rejects a structurally unusable setup — a request whose adapter does not
  require a presentation surface, or whose surface handle is invalid — through
  the kernel error model (`KernelResult` / `KernelError`, scope `Id`, code
  `InvalidId`).

## What live mode intentionally does not do yet

- It does **not** touch a GPU, create an adapter/device/queue, configure a
  swapchain, or present a single pixel. No `GpuSubmissionReport` ever claims
  presentation happened.
- It does **not** manage adapter/device/surface lifetimes. The purpose of this
  pass is the *seam*, not the renderer backend.

## Why live mode consumes host presentation data, not browser objects

Live mode is built from `axiom_host::HostPresentationRequest` —
host-owned, deterministic, `Copy` data describing *what* to present into
(`HostPresentationTarget` / `HostSurfaceHandle` identities) and *with what*
(`HostSurfaceDescriptor` + abstract adapter/device requests). It stores **no**
`wgpu`, `web_sys`, `js_sys`, `wasm_bindgen`, window, or canvas objects. The
`HostSurfaceHandle`'s kernel `HandleId` is the stable join key: a future
adapter binds the real surface to that id in its *own* table, out-of-band,
without leaking browser/GPU types into any engine module. This keeps
`axiom-webgpu` deterministic, replayable, and buildable in every native and
headless context.

## What this module does not import

- `axiom-scene`, `axiom-resources`, `axiom-render` — no module imports another
  module (enforced by `tests/architecture.rs` and the architecture checker).
- Any app or tool crate.
- Real GPU/JS bindings (`wgpu::`, `web_sys::`, `js_sys::`, `wasm_bindgen::`) —
  still absent, enforced by `tests/architecture.rs::no_real_gpu_backend_today`.

### Why webgpu does not import render

Per the workspace's module law, modules may not depend on other modules. The
**app** translates `RenderCommandList → GpuSubmission`, so the GPU backend
stays usable by a future test/native/browser app that picks a different
producer. `axiom-webgpu` depends only on the layers its `module.toml` allows
(`kernel`, `runtime`, `math`, `host`, `frame`).

## Public surface

`lib.rs` exposes **exactly one** facade: `WebGpuApi`. Backend kinds, the
internal state machine, and the submission status are reached only through it
(or, externally, through `WebGpuApi` constructors and report accessor methods
and bool predicates such as `is_live()`, `report.presented()`).

## What remains for the browser-visible app pass

This module is intentionally **not** the browser app and not the live renderer.
The remaining work lives *outside* this module:

1. **A browser/native adapter app** (a new app, not a layer or module) that
   owns the real `<canvas>`/window, creates the real `wgpu::Surface`, and
   stores it in a `HandleId → real surface` table keyed by the
   `HostSurfaceHandle`. This is where `wasm-bindgen` / `web_sys` glue lives.
2. **An async device-init path.** `wgpu`'s `Instance → Adapter → Device →
   Queue` is async; runtime/host stepping is synchronous. The adapter must
   bridge async acquisition to the point where a `HostSurfaceHandle` becomes
   backed.
3. **A real `submit()` live arm.** Once a surface/device is bound, the
   `LivePresentationRequested` state gains a `LiveReady` successor and
   `submit()` performs real submission — consuming the *same* `GpuSubmission`
   and returning the *same* `GpuSubmissionReport` shape, with a future
   `Presented`-style status. Nothing else in the engine changes.
