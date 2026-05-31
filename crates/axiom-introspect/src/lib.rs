//! # Axiom Introspect — Layer 05
//!
//! The engine's agent-facing introspection surface. Axiom already *records*
//! (kernel structured logs and telemetry); this layer makes the engine
//! *remember* and *answer*. It adapts the canonical per-frame contract
//! ([`axiom_frame::EngineFrame`]) into a retained, deterministic,
//! byte-serializable model — [`FrameReport`] per frame, a bounded
//! [`FrameHistory`] of recent frames — behind one query facade,
//! [`IntrospectApi`], that an external agent uses to interrogate a running
//! engine: "describe frame N", "what systems ran and which failed", "give me
//! the recent frames", "hand me a serialized snapshot".
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

mod frame_history;
mod frame_report;
mod introspect_api;
mod system_report;

#[cfg(test)]
mod fixtures;

// --- Curated public surface ---

// Primary facade.
pub use introspect_api::IntrospectApi;

// Inert, inspectable, serializable report types reachable through the facade.
pub use frame_history::FrameHistory;
pub use frame_report::FrameReport;
pub use system_report::SystemReport;
