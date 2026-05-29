//! # Axiom WebGPU — Engine Module (backend boundary)
//!
//! Owns the deterministic WebGPU/wgpu backend boundary.
//!
//! ## Today: a deterministic recorder
//! The vertical slice ships only the `Recording` backend. Every
//! command the app pushes into a [`crate::webgpu_api::WebGpuApi`]
//! submission is captured in a deterministic
//! [`crate::gpu_submission_report::GpuSubmissionReport`] and returned
//! to the caller. **No GPU calls are made today.**
//!
//! ## What blocks real WebGPU submission
//! Real `wgpu` (or `web-sys`) integration needs a surface the
//! `axiom-host` layer does not yet expose. The host layer's
//! `HostViewport` describes the viewport but does not yet hand out a
//! surface handle, a `wasm-bindgen` canvas, or a `RawWindowHandle`.
//! Wiring those up belongs in a future layer-03 (host) fix; the
//! module boundary here is already the shape the live backend will
//! present.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`WebGpuApi`].

mod backend_kind;
mod gpu_command;
mod gpu_submission;
mod gpu_submission_report;
mod webgpu_api;

pub use webgpu_api::WebGpuApi;
