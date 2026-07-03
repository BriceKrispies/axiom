//! # Axiom forest — Feature Module
//!
//! A chunk's worth of trees: the reusable vegetation a streamed world grows on
//! each loaded chunk, composed once so an app doesn't re-derive "scatter → seated
//! tree instances" by hand.
//!
//! - **[`ForestApi`]** — the facade:
//!   - [`ForestApi::chunk_trees`] composes the scatter field ([`axiom_scatter`])
//!     into a cell's tree transforms — each tree at a scattered site, seated on
//!     the ground via a caller-supplied height function, with a stable per-tree
//!     yaw and size derived from the site's seed. Deterministic and seamless
//!     across cells (it inherits the scatter's tiling).
//!   - [`ForestApi::tree_mesh`] is a simple unit tree (a crossed billboard, brown
//!     at the base fading to canopy green) to instance the transforms with.
//! - **[`ForestConfig`]** — the scatter rule + the tree size range.
//!
//! ## Payload-agnostic edges
//! The module owns *vegetation placement*, not terrain or rendering: the caller
//! supplies the ground height (so terrain stays the caller's) and draws the
//! returned transforms with the tree mesh (so the renderer stays the caller's).
//!
//! ## Deliberately thin
//! This is the first cut — simple trees that prove the streamed-world loop end to
//! end. The diorama's full foliage/branch/leaf art graduates into this facade
//! incrementally, behind the same `chunk_trees` / `tree_mesh` surface.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one behavioral facade** — [`ForestApi`] — plus the
//! pure value-type vocabulary it takes in ([`ForestConfig`]).

mod forest_api;
mod ids;

pub use forest_api::ForestApi;
pub use ids::ForestConfig;
