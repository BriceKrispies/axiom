//! # Axiom WebGPU — Engine Module (backend boundary)
//!
//! Owns the deterministic WebGPU/wgpu backend boundary.
//!
//! ## Two backend modes, one input contract
//! `WebGpuApi` operates in one of two backend modes, both routed through the
//! **same** [`crate::gpu_submission::GpuSubmission`] input shape:
//!
//! - **Recording** (the default, the proof backend): every command is
//!   captured into a deterministic
//!   [`crate::gpu_submission_report::GpuSubmissionReport`] and **no GPU calls
//!   are made**. This is what the headless vertical slice relies on.
//! - **Live**: the structural seam for real presentation. A live backend is
//!   built from the deterministic host presentation boundary
//!   (`axiom_host::HostPresentationRequest`) and accepts the same
//!   `GpuSubmission`, but performs **no real GPU work** in this pass — its
//!   report carries a deterministic not-bound / not-initialized status and
//!   never claims pixels were presented.
//!
//! ## What blocks real WebGPU submission
//! Real `wgpu` (or `web-sys`) integration needs a *bound* surface/device. The
//! `axiom-host` layer now exposes the abstract presentation boundary
//! (`HostPresentationRequest` / `HostSurfaceHandle` / adapter + device
//! requests), so live mode can consume host-owned data — but binding a real
//! surface to a `HostSurfaceHandle` and driving real GPU calls belongs to a
//! future browser/native adapter app, not this module. See `ARCHITECTURE.md`.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`WebGpuApi`]. Backend modes,
//! states, and submission status are reached only through it.

mod backend_kind;
mod gpu_command;
mod gpu_submission;
mod gpu_submission_report;
mod gpu_submission_status;
mod webgpu_api;
mod webgpu_backend_state;

pub use webgpu_api::WebGpuApi;
