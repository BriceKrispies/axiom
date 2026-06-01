//! # Axiom Render Pipeline — Feature Module
//!
//! The deterministic per-frame render pipeline, as a **feature module**: the
//! one place that composes the isolated engine modules (`axiom-scene`,
//! `axiom-render`, `axiom-webgpu`) into a single capability, so apps don't each
//! reimplement the translation.
//!
//! ```text
//! SceneApi snapshot + caller-supplied mesh/material assets
//!   → RenderInput            (axiom-render builder)
//!   → RenderCommandList      (axiom-render)
//!   → GpuSubmission          (axiom-webgpu)
//!   → submit → report + per-draw data
//! ```
//!
//! ## What this module is
//! - A *feature* (composition) module — it depends on the engine modules it
//!   lists in `module.toml`'s `allowed_modules` (`scene`, `render`, `webgpu`)
//!   plus the math layer. The Module Law permits this only for feature modules.
//! - The single owner of the scene→render→GPU translation. Apps build a scene,
//!   advance it, hand it here with their resource assets and a WebGPU backend,
//!   and read back a deterministic report — they no longer thread every
//!   boundary by hand.
//!
//! ## What this module is not
//! Not a scene model, not a renderer, not a GPU backend — it *orchestrates*
//! those. It owns no component types, no command model, and no device state. It
//! is browser-free: the caller supplies the `WebGpuApi` (recording or live).
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`RenderPipelineApi`]. The frame
//! input and the frame report are reached only through it.

mod render_pipeline_api;

pub use render_pipeline_api::RenderPipelineApi;
