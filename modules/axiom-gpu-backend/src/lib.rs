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

// The real wgpu binding — compiled only for wasm32, behind the facade. Never
// enters the native build or the coverage gate.
#[cfg(target_arch = "wasm32")]
mod live_gpu_binding;

pub use gpu_backend_api::GpuBackendApi;
