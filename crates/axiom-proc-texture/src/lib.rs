//! # Axiom Proc-Texture — texture operators (layer)
//!
//! A tiny, orthogonal set of texture operators that a recipe composes into an
//! RGBA8 [`TextureBuffer`]. Seven **sources** (Solid, Gradient, Noise, Bricks,
//! Checker, Text, Spots) and four **transforms** (Blur, Blend, ColorRamp,
//! HeightToNormal), dispatched branchlessly by a `const` table over the operator
//! code and baked through the shared [`axiom_proc_core::ProcCore`] executor.
//!
//! Beside those eleven **fixed** operators sits one whose shape the recipe
//! author chooses: [`TextureOp::Field`] bakes an [`axiom_field::FieldGraph`] —
//! a pointwise expression carried as a value — into the same RGBA8 buffer, one
//! evaluation per texel. It does not replace the eleven: `Blur` is a
//! neighbourhood operator a pointwise field cannot express, and the fixed
//! generators are what the apps on this layer already bake. The graph travels
//! beside the recipe in the field table [`ProcTextureApi::bake_with_fields`]
//! takes, and the node carries only its index.
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
mod field_source;
mod filters;
mod generators;
mod proc_texture_api;
mod text;
mod texture_buffer;
mod texture_op;

pub use proc_texture_api::ProcTextureApi;
pub use texture_buffer::{TextureBuffer, MAX_DIM};
pub use texture_op::TextureOp;
