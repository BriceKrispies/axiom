# axiom-windowing — architecture

`axiom-windowing` is the **deterministic presentation driver**: the part of
presentation that owns *what* a window shows and *when*, with no browser or GPU
object in its native build. It is an **engine module** (`kind =
"engine-module"`, `allowed_modules = []`) over the kernel and the `host` layer's
presentation boundary.

## Why an engine module, not a feature module over `axiom-webgpu`

The obvious instinct is "compose `axiom-webgpu` so windowing can consume a
`GpuSubmission`." That does not work, and the reason is structural, not
incidental: **module contract types are not nameable across modules.**
`axiom-webgpu`'s `lib.rs` exposes exactly one facade (`WebGpuApi`);
`GpuSubmission` is `pub` only inside a private module, so another crate can hold
it as an inferred local but can never *name* it in a signature. A
`PresentationBackend` trait therefore cannot take `&GpuSubmission`.

What actually reaches the GPU is **plain, nameable data** — per-draw
`mvp: [f32; 16]` + `color: [f32; 4]` + clear colour, extracted from the render
pipeline's report (the umbrella's `App` already does exactly this extraction).
So the presentation backend operates on plain data + the host layer's
presentation types, and windowing needs **no** module dependency. That is why it
is an isolated engine module.

This also sharpens the "one path" goal: the unifying artifact for live vs.
deterministic presentation is **not** `GpuSubmission`; it is the single
`RenderPipelineApi.submit` → extracted plain draws, fed to either the recording
backend (today) or the live backend (the future platform arm).

## The platform boundary (the compiled-out arm)

The real `wgpu` / `web-sys` / `requestAnimationFrame` / canvas binding is a
**later, platform-gated addition** that will live behind this deterministic
core, compiled only for `wasm32`. Coverage posture, per the Coverage Law's
"platform arm compiled out" clause:

- **All logic** — presentation-request assembly, the fixed-step loop, and (when
  added) the surface-binding lifecycle and the submission→draw marshalling —
  lives on the deterministic side and is at **100%** coverage on native.
- The `wasm32` arm is a **thin, logic-free** translation from plain draw data
  into real GPU calls. It carries no decisions, is never in the native coverage
  report (compiled out), and is the one place this module references platform
  APIs — added to the source-hygiene platform allowlist
  (`crates/xtask/src/hygiene.rs`) when it lands.

If a real surface/device cannot be acquired (no browser host), the deterministic
state still validates and the blocker is documented here — the same boundary
`axiom-webgpu`'s `ARCHITECTURE.md` already draws.

## Today's surface (`WindowingApi`)

- `configure_surface(width, height)` — assemble + validate one
  `HostPresentationRequest` (the relocated, browser-free
  `build_presentation_request`); the one fallible step is the host's viewport
  validation.
- `step() -> tick`, `next_tick()`, `frames_driven()` — the fixed-step run-loop
  driver `App::run` pumps (one tick per animation frame on the web; a
  finite/headless drive on native).

## Roadmap (north-star App)

The browser demo app still holds its own copies of this deterministic core
(`render_loop.rs`, `live_gpu_binding.rs`'s state machine,
`browser_bootstrap.rs`'s request assembly). A later phase rewires that app to
consume this module and deletes the duplicates; the live `wgpu` arm and the App
run-loop ownership follow. See the north-star roadmap.
