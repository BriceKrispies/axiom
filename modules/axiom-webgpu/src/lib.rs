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
//! - **Live**: the structural seam for real presentation, built from the
//!   deterministic host presentation boundary
//!   (`axiom_host::HostPresentationRequest`). It accepts the same
//!   `GpuSubmission`, and — behind the off-by-default `offscreen` feature —
//!   **realizes it on a real native GPU off-screen** and reads the pixels back
//!   (`WebGpuApi::present_submission_offscreen_rgba`), proving the same
//!   `GpuSubmission` the recording backend records can render actual pixels.
//!   The deterministic `submit()` receipt is unchanged and never claims a
//!   swap-chain present occurred.
//!
//! ## What the real-GPU arm needs, and what it does not
//! The off-screen live arm needs only a native GPU adapter — no surface, no
//! swap-chain, no browser objects — so it runs headlessly in CI-like
//! environments with a GPU and stays isolated behind `offscreen`. A *browser
//! swap-chain* present still needs a bound surface: the `axiom-host` layer
//! exposes the abstract presentation boundary (`HostPresentationRequest` /
//! `HostSurfaceHandle` / adapter + device requests), and binding a real surface
//! to a `HostSurfaceHandle` belongs to a browser/native adapter app, not this
//! module. See `ARCHITECTURE.md`.
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

// The live presentation arm: real `wgpu`, off-screen, native. Compiled only
// behind the off-by-default `offscreen` feature, so the default build (and the
// coverage / branchless / hygiene gates that run it) never see the real-GPU
// arm; the recording backend stays the deterministic default. See ARCHITECTURE.md.
#[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
mod live_present;

pub use webgpu_api::WebGpuApi;
