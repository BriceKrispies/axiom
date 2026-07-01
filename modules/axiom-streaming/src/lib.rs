//! # Axiom streaming — Engine Module
//!
//! A payload-agnostic **residency ring** over an integer 2-D lattice: the
//! reusable substrate every streamed, focus-radius world stands on. This is the
//! chunk-streaming ring the growth game grew — lifted out of the app so any app
//! (a survival overworld, an open-world camera, a tile stream) composes the same
//! deterministic load/unload logic instead of re-deriving it.
//!
//! - **[`Residency`]** — the facade: tracks *which* coordinates are resident and,
//!   as a focus point moves, computes the deterministic set of coordinates to
//!   load and to unload via [`Residency::apply`]. Dirty (author-edited)
//!   coordinates are never unloaded; a hysteresis `margin` keeps a jittering
//!   focus from thrashing the boundary.
//! - **[`ChunkCoord`]** — the integer ground-plane coordinate the ring addresses.
//! - **[`ResidencyDelta`]** — the `{ load, unload }` outcome of a move.
//!
//! ## Payload-agnostic by design
//! The ring owns *residency*, never *payload*. It emits coordinates; the caller
//! turns a `load` coordinate into whatever it holds (a heightfield chunk, a mesh,
//! an entity batch) and a `unload` coordinate into teardown. The module never
//! sees heights, meshes, or bytes — which is exactly why it is reusable across
//! unrelated worlds.
//!
//! ## Determinism
//! The resident set is a `BTreeSet`, and both delta vectors are emitted in a
//! fixed order (`load` in row-major scan order, `unload` sorted), so the same
//! focus move produces byte-identical deltas run-to-run and machine-to-machine.
//! No tick, RNG, or wall-clock reaches any query.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one behavioral facade** — [`Residency`] — plus the
//! pure value-type vocabulary it traffics in ([`ChunkCoord`], [`ResidencyDelta`]):
//! the nouns the ring returns and takes in, carrying data, not engine state.

mod ids;
mod residency;

pub use ids::{ChunkCoord, ResidencyDelta};
pub use residency::Residency;
