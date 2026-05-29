# Axiom WebGPU — Module Architecture

`axiom-webgpu` is **an isolated engine module** that owns the
WebGPU/wgpu backend boundary.

## Status: deterministic recorder

The vertical slice ships only the **`Recording`** backend. Every
command the app pushes into a `GpuSubmission` is captured in a
deterministic `GpuSubmissionReport`. **No real GPU calls are made.**

## What this module owns

- `BackendKind` — backend selector (`Recording` today).
- `GpuCommand` — backend-level commands:
  `ClearFrame`, `SetPipeline`, `SetCamera`, `SetMesh`, `SetMaterial`,
  `DrawIndexed`, `Present`.
- `GpuSubmission` — mutable ordered submission input.
- `GpuSubmissionReport` — deterministic record (commands + per-kind counts).
- `WebGpuApi` — the single public facade.

## What this module does not import

- `axiom-scene`, `axiom-resources`, `axiom-render` — no module imports
  another module.
- Any app crate.
- Real GPU bindings (`wgpu::`, `web_sys::`, `js_sys::`,
  `wasm_bindgen::`) — gated by missing host capability (see below).

## What blocks real WebGPU submission

Going from the `Recording` backend to a live `wgpu`/`web-sys`
backend needs three host capabilities the layer-03 module does not
yet expose:

1. **A surface handle.** `HostViewport` describes the viewport but
   does not yet hand out a `wasm-bindgen` canvas reference, a
   `RawWindowHandle`, or a `wgpu::Surface` constructor.
2. **An async device initialisation path.** `wgpu`'s `Instance ⇒
   Adapter ⇒ Device ⇒ Queue` chain is async; `axiom-host` is
   synchronous today and the runtime/frame layer assumes synchronous
   stepping.
3. **An adapter request mechanism.** `axiom-host` does not expose a
   "request adapter" entry point matching the future browser
   `navigator.gpu.requestAdapter()` shape.

When those land in `axiom-host`, this module's `submit()` can be
extended with a `BackendKind::Live` arm that performs real submission
behind the same `GpuSubmission` / `GpuSubmissionReport` boundary. The
shape the rest of the engine consumes does not change.

## Why webgpu does not import render today

Per the workspace's module law, modules may not depend on other
modules. The app translates `RenderCommandList → GpuSubmission` so
the GPU backend stays usable by a future test app that bypasses the
render module entirely.

## Public surface

`lib.rs` exposes **exactly one** facade: `WebGpuApi`.
