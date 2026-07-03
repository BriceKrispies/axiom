//! # Axiom scatter — Engine Module
//!
//! Chunked deterministic point scatter: the reusable worldgen primitive that
//! turns "which trees / rocks / plants stand where" from a one-off patch draw
//! into a **seamless, tileable, infinite field**. Given a world seed and an
//! integer cell coordinate, it emits that cell's scattered ground positions —
//! and because a site depends only on its own cell, adjacent cells stitch
//! together with no seam and no cross-cell bookkeeping.
//!
//! - **[`ScatterApi`]** — the facade. [`ScatterApi::chunk_sites`] places one
//!   cell's sites as a **jittered sub-grid**: the cell is divided into
//!   `sites_per_side²` sub-cells, each of which may spawn one site (kept with
//!   probability `fill`, wiggled up to `jitter` of a sub-cell from its centre).
//!   The sub-grid gives an implicit minimum spacing that holds *across* the cell
//!   boundary, so the field never clumps or gaps at a seam.
//! - **[`CellCoord`]** — the integer ground-plane cell the field addresses.
//! - **[`ScatterRule`]** — the sub-grid rule (`sites_per_side`, `jitter`, `fill`).
//! - **[`ScatterSite`]** — a placed site: a ground position + a stable seed the
//!   caller expands into per-instance attributes (yaw, scale, species).
//!
//! ## Determinism & seamlessness
//! Each site is derived from an independent, order-stable sub-stream of the
//! cell's entropy address (`axiom_entropy` keyed by an `axiom_space` address of
//! `domain / cell.x / cell.z`). So the same `(seed, cell)` yields byte-identical
//! sites run-to-run and machine-to-machine, and a cell can be (re)generated in
//! isolation — the property that lets a world stream cells in and out around a
//! moving camera without the field ever shifting under the player.
//!
//! ## Placement-only by design
//! The module owns *where*, never *what*. It emits positions + seeds; the caller
//! seats each site on its terrain and turns the seed into whatever it grows. The
//! module never sees a heightfield or a mesh — which is why it is reusable across
//! a forest, a boulder field, a village, or a star field.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one behavioral facade** — [`ScatterApi`] — plus the
//! pure value-type vocabulary it traffics in ([`CellCoord`], [`ScatterRule`],
//! [`ScatterSite`]).

mod ids;
mod scatter_api;

pub use ids::{CellCoord, ScatterRule, ScatterSite};
pub use scatter_api::ScatterApi;
