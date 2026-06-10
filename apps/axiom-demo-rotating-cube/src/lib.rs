//! # Axiom — Headless Deterministic Rotating-Cube Vertical Slice
//!
//! The first end-to-end vertical slice, and the **only** place in the
//! workspace that composes all four engine modules. This first pass is
//! **headless**: it proves that the layers and modules produce
//! deterministic per-frame artifacts end-to-end. It does **not** open a
//! canvas, create a WebGPU surface/swapchain, or present pixels — see
//! `VERTICAL_SLICE.md` for the data flow and the WebGPU presentation
//! blocker.
//!
//! ```text
//! frame tick
//!   → runtime step                 (axiom-runtime, via axiom-host/axiom-frame)
//!   → scene transform update       (axiom-scene)
//!   → SceneSnapshot                (axiom-scene)
//!   → ResolvedResources            (axiom-resources)
//!   → RenderInput                  (app glue → axiom-render)
//!   → RenderCommandList            (axiom-render)
//!   → GpuSubmission                (app glue → axiom-webgpu)
//!   → GpuSubmissionReport          (axiom-webgpu)
//! ```
//!
//! ## Public surface
//! The single behavioral facade is [`DemoRotatingCubeApi`]. Everything
//! else exported here is **inert plain data**: the artifact tree that
//! [`DemoRotatingCubeApi::run_tick`] returns, exposed so every boundary
//! value is inspectable by callers and tests.

mod demo_api;
mod render_to_gpu_submission;
mod scene_to_render_input;
mod vertical_slice;

// The single behavioral facade of the app.
pub use demo_api::DemoRotatingCubeApi;

// --- Inspectable artifact tree returned by `DemoRotatingCubeApi::run_tick`. ---

pub use vertical_slice::{
    CubeIdentityArtifact, CubeTransformArtifact, GpuSubmissionReportArtifact, VerticalSliceArtifact,
};

pub use scene_to_render_input::{
    RenderCameraArtifact, RenderInputArtifact, RenderLightArtifact, RenderMaterialArtifact,
    RenderMeshArtifact, RenderObjectArtifact, ResolvedMaterialArtifact, ResolvedMeshArtifact,
    ResolvedResourcesArtifact, SceneCameraArtifact, SceneLightArtifact, SceneNodeArtifact,
    SceneRenderableArtifact, SceneSnapshotArtifact,
};

pub use render_to_gpu_submission::{
    GpuCommandArtifact, GpuSubmissionArtifact, RenderCommandArtifact, RenderCommandListArtifact,
};
