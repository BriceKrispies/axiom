//! # Axiom world — Feature Module
//!
//! The per-frame **streaming-world plan**: the reusable policy every streamed,
//! culled, level-of-detail'd world runs each frame, composed once so an app
//! doesn't re-derive it. Given the camera and a [`WorldConfig`], one
//! [`WorldApi::frame_plan`] call decides:
//! - which chunks to **load** and **unload** — the residency ring
//!   ([`axiom_streaming`]) around the camera's focus chunk; and
//! - which loaded chunks are **visible** this frame and at what **level of
//!   detail** — frustum culling + distance-banded LOD ([`axiom_visibility`]).
//!
//! - **[`WorldApi`]** — the facade. Stateful only in its residency ring; call
//!   [`WorldApi::frame_plan`] each frame with the camera position, its
//!   clip-from-world matrix, and a closure giving each chunk's world AABB (the
//!   caller owns terrain, so the module never needs to know it).
//! - **[`WorldConfig`]** — chunk size, load radius + hysteresis margin, LOD bands.
//! - **[`WorldFramePlan`]** — `{ load, unload, visible }` for the frame.
//! - **[`VisibleChunk`]** — a loaded, in-frustum chunk + its LOD.
//!
//! ## Payload-agnostic by design
//! The module owns the *plan*, never the *payload*. `frame_plan` emits chunk
//! coordinates to load / unload / draw; the caller turns a loaded coordinate into
//! geometry (terrain + scattered content) and draws the visible ones at their
//! LOD. This is why the same policy drives a forest, a city, or an ocean.
//!
//! ## Determinism
//! The plan is a pure function of the camera, the config, and the residency
//! state, so a replayed camera path produces byte-identical plans.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one behavioral facade** — [`WorldApi`] — plus the
//! pure value-type vocabulary it traffics in ([`WorldConfig`], [`WorldFramePlan`],
//! [`VisibleChunk`]).

mod ids;
mod world_api;

pub use ids::{VisibleChunk, WorldConfig, WorldFramePlan};
pub use world_api::WorldApi;
