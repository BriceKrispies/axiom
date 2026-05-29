//! # Axiom — Deterministic Rotating Cube Demo
//!
//! The first end-to-end vertical slice. This app is the **only** place
//! that knows about all four engine modules at once:
//!
//! ```text
//! frame tick
//!     → scene transform update     (axiom-scene)
//!     → SceneSnapshot              (axiom-scene)
//!     → ResolvedResources          (axiom-resources)
//!     → RenderInput                (axiom-render)
//!     → RenderCommandList          (axiom-render)
//!     → GpuSubmission              (axiom-webgpu)
//!     → GpuSubmissionReport        (axiom-webgpu)
//! ```
//!
//! See `VERTICAL_SLICE.md` for the boundary-by-boundary description
//! and the current WebGPU blocker.

mod app_state;
mod cube_demo;
mod cube_frame;
mod translation;

pub use cube_demo::RotatingCubeDemo;
pub use cube_frame::CubeFrame;
