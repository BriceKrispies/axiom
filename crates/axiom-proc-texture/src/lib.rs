//! # Axiom Proc-Texture — texture operators (layer)
//!
//! A tiny, orthogonal set of texture operators that a recipe composes into an
//! RGBA8 [`TextureBuffer`]. Six **sources** (Solid, Gradient, Noise, Bricks,
//! Checker, Text) and four **transforms** (Blur, Blend, ColorRamp,
//! HeightToNormal), dispatched branchlessly by a `const` table over the operator
//! code and baked through the shared [`axiom_proc_core::ProcCore`] executor.
//!
//! ## What it is, and is not
//! - **Neutral output.** A [`TextureBuffer`] is plain row-major RGBA8 — the shape
//!   an app hands to `RunningApp::add_texture_data`. It names no GPU resource.
//! - **Deterministic.** The same recipe and seed produce byte-identical pixels;
//!   the Noise operator draws its seed from the node's `axiom-entropy` stream.
//! - **Branchless + bounded.** Dispatch is a table index; dimensions clamp into
//!   `1..=MAX_DIM` and blur radius into a fixed cap, so a recipe can never ask for
//!   an unbounded texture.

mod color_math;
mod dispatch;
mod filters;
mod generators;
mod proc_texture_api;
mod text;
mod texture_buffer;
mod texture_op;

pub use proc_texture_api::ProcTextureApi;
pub use texture_buffer::{TextureBuffer, MAX_DIM};
pub use texture_op::TextureOp;
