//! # Axiom visibility — Engine Module
//!
//! Per-frame **view-visibility**: the reusable "what does the camera actually
//! see, and how finely" decision every rendered world needs, lifted out of any
//! single app so a streamed world, a scene renderer, and a demo all compose the
//! same culling + level-of-detail logic instead of re-deriving it.
//!
//! - **[`VisibilityApi`]** — the facade. Two pure queries:
//!   - [`VisibilityApi::visible_mask`] — frustum-cull a batch of world-space
//!     bounding boxes against the camera's clip-from-world matrix, returning one
//!     keep/cull flag per box. A degenerate matrix is treated conservatively
//!     (everything visible), so culling never wrongly hides geometry.
//!   - [`VisibilityApi::lod_levels`] — the level of detail for each box: how many
//!     ascending distance bands (metres) the camera→box-centre distance exceeds.
//!     `0` is nearest / highest detail.
//!
//! ## Determinism
//! Both queries are pure functions of their inputs (camera matrix / position,
//! boxes, bands). No tick, RNG, or wall-clock reaches them, so the same frame
//! produces byte-identical visibility run-to-run and machine-to-machine.
//!
//! ## Payload-agnostic by design
//! The module owns *visibility*, never *geometry*. It takes bounding boxes and
//! emits flags + levels; the caller decides what a "visible" box draws and what a
//! LOD level selects (which mesh, how many instances). The module never sees a
//! vertex — which is exactly why it is reusable across unrelated worlds.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one behavioral facade** — [`VisibilityApi`]. It
//! traffics only in lower-layer value types (`Mat4`, `Aabb`, `Vec3`, `Meters`)
//! and plain `bool` / `u8` outcomes, so it publishes no vocabulary of its own.

mod visibility_api;

pub use visibility_api::VisibilityApi;
