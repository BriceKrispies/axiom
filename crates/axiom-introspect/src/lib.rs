//! # Axiom Introspect — Layer 06
//!
//! The engine's agent-facing introspection surface. Axiom already *records*
//! (kernel structured logs and telemetry); this layer makes the engine
//! *remember* and *answer*. It adapts the canonical per-frame contract
//! ([`axiom_frame::EngineFrame`]) into a retained, deterministic,
//! byte-serializable model — [`FrameReport`] per frame, a bounded
//! [`FrameHistory`] of recent frames, a [`FrameDiff`] between any two — and
//! observes the ECS world ([`WorldReport`]) it sits on, and carries the world's
//! semantic nouns ([`WorldTag`]) an agent resolves a command against, behind one
//! query facade, [`IntrospectApi`], that an external agent uses to interrogate a
//! running engine: "describe frame N", "what systems ran and which failed",
//! "what changed between tick N and M", "how big is the world", "what is *in*
//! the world and what is it called", "hand me a serialized snapshot".
//!
//! ## What this layer is
//! - The single place the frame contract is projected into an owned,
//!   serializable, queryable report keyed by engine frame index.
//! - A bounded, deterministic, replay-friendly history of those reports.
//!
//! ## What this layer is not
//! Not a transport. It produces inert data and a read facade; *how* an agent
//! receives it (a browser query bridge, a socket, a Rust call) is an app
//! concern. It is also not a renderer, scene, asset, physics, input, or
//! browser adapter, and it never reads a wall clock or randomness — every
//! value enters as the explicit frame data handed to [`IntrospectApi::observe`].
//!
//! ## Public surface
//! `lib.rs` exposes the [`IntrospectApi`] facade plus the curated report
//! types. The curated set is locked down by
//! `tests/architecture.rs::lib_exports_are_curated_set`.

mod frame_diff;
mod frame_history;
mod frame_report;
mod introspect_api;
mod metric_report;
mod system_report;
mod world_report;
mod world_tag;

#[cfg(test)]
mod fixtures;

pub use introspect_api::IntrospectApi;

pub use frame_diff::FrameDiff;
pub use frame_history::FrameHistory;
pub use frame_report::FrameReport;
pub use metric_report::MetricReport;
pub use system_report::SystemReport;
pub use world_report::WorldReport;
pub use world_tag::WorldTag;
