//! # Axiom GPU Backend — platform-facing engine module (the real wgpu executor)
//!
//! The impure half of presentation: the part that actually owns the browser's
//! `wgpu` device, pipeline, and buffers and draws real pixels. It is constructed
//! from a `host`-layer [`axiom_host::HostPresentationRequest`] (so it composes no
//! other module — it consumes nameable host data, not a module contract type) and
//! presents instanced draws. The deterministic *what/when* of presentation stays
//! in `axiom-windowing`, which drives the run loop and delegates each frame's draw
//! to this backend.
//!
//! ## What this module is
//! - The single owner of the real GPU binding (surface/device/pipeline/buffers)
//!   and the per-frame present, plus mid-loop geometry replacement.
//! - The native-testable surface size + readiness + no-op present, with the real
//!   browser-only `wgpu` work compiled in behind the `wasm32` arm.
//!
//! ## What this module is not
//! Not the run loop, not a scene/world, not a renderer that knows about meshes or
//! materials by name. It takes plain engine data (vertex/instance float streams +
//! a clear colour) and a host presentation request, and issues GPU calls.
//!
//! This is the second sanctioned platform-facing module (Module Law #9): its real
//! `wgpu`/`web-sys` arm is compiled only for `wasm32`, behind the native-clean
//! facade, and never enters the native build or the coverage gate.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`GpuBackendApi`].

mod gpu_backend_api;

// Pure, native-testable adapter from the backend-neutral host::FramePacket to
// the live path's instance-batch + light shape. No GPU/browser code, so it
// builds and is covered on native exactly as on wasm.
mod frame_packet_adapter;

// The deterministic surface-recovery decision (what to do when the GPU surface
// is lost/outdated — as a backgrounded mobile browser does). Pure data → action,
// but its only non-test consumer is the wasm-only live binding, so it is compiled
// for `wasm32` (where the binding uses it) and for native `test` (where its own
// unit tests exercise it) — and is absent from the default/offscreen builds,
// where it would be dead.
#[cfg(any(target_arch = "wasm32", test))]
mod surface_recovery;

// The shared, target-agnostic renderer (pipeline + caches + draw). Compiled only
// where a real GPU is in play — wasm32 (the live arm) or the native `offscreen`
// feature (the screenshot tool) — so the default native build, coverage gate, and
// branchless lint never see this wgpu code.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod scene_renderer;

// The upscale-blit pipeline that presents a reduced-resolution render target to
// the swapchain (the mobile-first render-scale path). Its only consumer is the
// live binding (wasm32); the off-screen screenshot path renders at a fixed size
// and never upscales, so this is wasm32-only.
#[cfg(target_arch = "wasm32")]
mod upscale;

// The real wgpu swap-chain binding — compiled only for wasm32, behind the facade.
#[cfg(target_arch = "wasm32")]
mod live_gpu_binding;

// The native off-screen renderer — compiled only behind the `offscreen` feature
// (non-wasm). Drives the same `scene_renderer` as the live arm.
#[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
mod offscreen;

pub use gpu_backend_api::GpuBackendApi;
