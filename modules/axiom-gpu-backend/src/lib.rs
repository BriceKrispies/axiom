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
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`GpuBackendApi`].

mod gpu_backend_api;

// Pure, native-testable adapter from host::FramePacket to the live path's
// instance-batch + light shape.
mod frame_packet_adapter;

// Walks a layer-sorted host::Draw2dList into backend-neutral quad geometry
// (positions, UVs, alpha-folded colours, per-quad texture). Pure and
// branchless — the 2D peer of `frame_packet_adapter`.
mod draw2d_geometry;

// The real wgpu pipeline that draws `draw2d_geometry`'s output, alpha-blended,
// to a wgpu colour target — the 2D peer of `scene_renderer`.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod draw2d_renderer;

// Native off-screen 2D capture entry: renders a Draw2dList's geometry into a
// linear RGBA8 texture and reads it back; drives `axiom-shot` and the SPEC-04
// alpha-blend parity proof.
#[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
mod draw2d_offscreen;

// The deterministic surface-recovery decision (what to do when the GPU surface
// is lost/outdated, as a backgrounded mobile browser does).
#[cfg(any(target_arch = "wasm32", test))]
mod surface_recovery;

// The shared, target-agnostic renderer (pipeline + caches + draw).
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod scene_renderer;

// Upscale-blit pipeline presenting a reduced-resolution render target: the live
// binding's mobile-first render-scale path, and the offscreen retro 32-bit low-res +
// nearest upscale. Available wherever a real GPU renders (wasm32 / offscreen).
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
mod upscale;

// The real wgpu swap-chain binding.
#[cfg(target_arch = "wasm32")]
mod live_gpu_binding;

// The native off-screen renderer. Drives the same `scene_renderer` as the live arm.
#[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
mod offscreen;

pub use gpu_backend_api::GpuBackendApi;
