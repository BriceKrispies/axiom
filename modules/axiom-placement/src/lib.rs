//! # Axiom Placement — deterministic object scatter (engine module)
//!
//! The first **domain generator** built on the procedural-generation substrate
//! (roadmap Phase 9). [`PlacementApi::scatter`] evaluates a draw-recipe at a
//! content address and reduces each artifact word into an integer grid position,
//! producing a reproducible placement — the same `(seed, address, count, bounds)`
//! always yields the same scatter, on every run and platform.
//!
//! ## What it is, and is not
//! - A reusable **engine module** (not app-local): a domain capability apps and
//!   feature modules compose. It depends on the `space` + `proc` (+ `kernel`)
//!   layers and on **no other module**.
//! - **Domain-light and integer-only.** A placement is a list of `(x, y)` cells;
//!   *what* is placed there is a caller's concern. No naked floats, no geometry,
//!   no browser/platform APIs. Branchless and 100%-covered like every module.
//!
//! ## Public surface
//! One facade: [`PlacementApi`]. The `Placement` it returns is read through its
//! own methods (positions / digest / canonical bytes).

mod placement;
mod placement_api;

pub use placement_api::PlacementApi;
