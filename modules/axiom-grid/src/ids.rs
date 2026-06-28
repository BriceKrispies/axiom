//! The module's identity vocabulary — the pure value types [`GridApi`] traffics
//! in. Re-exported from `lib.rs` via a single `pub use ids::{…}` so they sit
//! alongside the one behavioral facade without counting as a second facade
//! (Module Law #8). They carry data, not engine state.
//!
//! [`GridApi`]: crate::GridApi

pub use crate::cell::Cell;
pub use crate::dist::Dist;
pub use crate::grid::Grid;
pub use crate::tile_space::TileSpace;
