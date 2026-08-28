//! Ported from Claude-of-Duty `src/weapons/geometry.js` (447 lines) — the
//! procedural hard-surface geometry kit for the weapons. See
//! `docs/work-manifests/shmup-port/03-weapon-geometry-api.md` for
//! the fixed Rust API contract every primitive builder and part/model builder
//! writes against.
//!
//! This module owns the geometry buffer ([`Geo`]), the merge layer
//! ([`merge_all`], ported along with the two Three.js utilities it depends
//! on — see `merge`'s doc), and the [`Assembly`] builder. Primitive builders
//! (`box_geo`, `blob`, `lathe_z`, …) live in sibling modules landing from a
//! concurrent port pass against the same contract.

mod assembly;
mod geo;
mod merge;
pub mod primitives;

pub use assembly::{Assembly, Node, Xform};
pub use geo::Geo;
pub use merge::merge_all;
