//! # Axiom grid — Engine Module (SPEC-06)
//!
//! A first-class integer **grid** plus the deterministic **pathfinding** and
//! **steering** that read it. This is the substrate every tile, board, and
//! procedural-level game stands on:
//!
//! - **`Grid<T>`** — the board: a row-major integer container with an
//!   out-of-bounds-safe `get` (returns the grid's default cell, never panics),
//!   `set`, `fill`, `idx`, `in_bounds`, and `for_each`.
//! - **`TileSpace`** — the tile↔world mapping: cell-center `tile_to_world`,
//!   `world_to_tile`, and idempotent `snap_to_cell`, all pure functions of an
//!   origin and a dimensioned cell size.
//! - **The path queries** — `distance_field`, `path`, `reachable`, and
//!   `step_toward`, all `sim`-class: pure functions of `(grid, cells, passable)`
//!   with a fixed canonical neighbor order (N, E, S, W) and a lexicographic
//!   `(y, x)` tie-break, so paths are *byte*-identical run-to-run and machine-to-
//!   machine, not merely *a* shortest path.
//!
//! ## Determinism
//! No tick, no RNG, no wall-clock reaches any query. The distance field is a
//! **bounded wavefront relaxation** — a fixed number of branchless relaxation
//! passes over the flat cell array — not a queue-based BFS, so the spine stays
//! branchless. `GridApi::state_hash` pins a board into the kernel's `StableHash`
//! sequence for the §17.4 replay obligation.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one behavioral facade** — [`GridApi`] — plus the
//! module's pure value-type vocabulary ([`Grid`], [`Cell`], [`TileSpace`],
//! [`Dist`]): the nouns the facade traffics in. They carry data, not engine
//! state, and an author cannot name a board, a coordinate, or a distance without
//! them (the same reason the `ecs` layer exports `EntityHandle`).

mod cell;
mod dist;
mod grid;
mod grid_api;
mod ids;
mod tile_space;

pub use grid_api::GridApi;
pub use ids::{Cell, Dist, Grid, TileSpace};
