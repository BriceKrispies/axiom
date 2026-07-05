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
- **Live** (`BackendKind::Live`). The real-presentation arm, built from the
  deterministic `axiom-host` presentation boundary (`HostPresentationRequest`)
  and accepting the **same** `GpuSubmission` shape. Behind the off-by-default
  `offscreen` feature it **executes that submission on a real native GPU
  off-screen** and reads the pixels back
  (`WebGpuApi::present_submission_offscreen_rgba`) — the headless proof that the
  deterministic command chain the engine records is the chain that renders real
  pixels. The deterministic `submit()` receipt is unchanged; only the new
  off-screen method touches a GPU.

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

## What live mode does

- Is constructible from a validated `HostPresentationRequest`
  (`WebGpuApi::new_live`), or as an explicitly unbound live backend
  (`WebGpuApi::new_live_unbound`).
- Accepts the same `GpuSubmission` as recording mode.
- The `submit()` receipt stays deterministic: its **status** explicitly states
  the recording outcome (`LiveNotBound` / `LiveNotInitialized`) and its
  `presented()` is **always `false`** — the receipt never claims a swap-chain
  present occurred.
- **Renders real pixels off-screen** (behind the `offscreen` feature):
  `present_submission_offscreen_rgba` interprets the submission's command stream
  (clear + camera + per-draw mesh/material/world), executes it on a throwaway
  native `wgpu` device into an off-screen colour+depth target, and reads the
  frame back to RGBA8. The wgpu `[0,1]` depth remap is applied **here, in the
  wgpu consumer**, so the upstream contracts stay backend-neutral.
- Rejects a structurally unusable setup — a request whose adapter does not
  require a presentation surface, or whose surface handle is invalid — through
  the kernel error model (`KernelResult` / `KernelError`, scope `Id`, code
  `InvalidId`).

## The off-screen boundary (why the real GPU arm is isolated)

- The real `wgpu` work is **compiled only behind the off-by-default `offscreen`
  feature on native**. The engine's default build — and therefore the coverage
  gate, the branchless dylint, and the source-hygiene scan — never compile it,
  exactly as `axiom-gpu-backend` isolates its own off-screen renderer. The
  recording backend stays the deterministic, fully-covered, branchless default.
- The off-screen arm needs only a native GPU adapter — **no surface, no
  swap-chain, no browser objects** — so it proves the chain headlessly. A
  browser swap-chain present still needs a bound surface and belongs to a host
  adapter app, not this module (see below). This module stores **no** `web_sys`
  / `js_sys` / `wasm_bindgen` binding.

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
- Real **browser** bindings (`web_sys::`, `js_sys::`, `wasm_bindgen::`) — absent,
  enforced by `tests/architecture.rs::no_browser_bindings`. Native `wgpu::` is
  permitted, but only inside the `offscreen`-gated `live_present` module.

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

The off-screen live arm proves the `GpuSubmission -> pixels` chain headlessly. A
*browser swap-chain* present is the remaining work, and it lives **outside** this
module:

1. **A browser/native adapter app** (a new app, not a layer or module) that
   owns the real presentation surface, creates the real `wgpu::Surface`, and
   stores it in a `HandleId → real surface` table keyed by the
   `HostSurfaceHandle`. This is where `wasm-bindgen` / `web_sys` glue lives.
   (The `offscreen` arm here needs no surface, so it renders headlessly today.)
2. **The async device-init bridge for the *surface* path.** The off-screen arm
   drives `wgpu`'s async `Instance → Adapter → Device` synchronously with
   `pollster::block_on`; a browser surface path must instead bridge async
   acquisition into the run loop where a `HostSurfaceHandle` becomes backed.
3. **A swap-chain `submit()` successor.** Once a surface/device is bound, the
   `LivePresentationRequested` state can gain a `LiveReady` successor that
   presents to the swap-chain — consuming the *same* `GpuSubmission` and
   returning the *same* `GpuSubmissionReport` shape, with a future
   `Presented`-style status. Nothing else in the engine changes.
