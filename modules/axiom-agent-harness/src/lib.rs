//! # Axiom Agent Harness — Feature Module
//!
//! The reusable, **game-agnostic** glue for driving any first-person Axiom game
//! with the [`axiom_agent`] substrate. It composes the `agent` engine module into
//! one neutral pipeline that every first-person game needs:
//!
//! ```text
//! observe (self pose + height + goal) -> decide (axiom-agent) -> lower to a
//! held-control bitmask the game applies
//! ```
//!
//! ## Why this exists
//!
//! `axiom-agent` deliberately knows no game noun: the app must translate its game
//! state into a neutral [`axiom_agent::AgentApi`] observation and translate the
//! emitted intents back into its own input. The DOOM app hand-rolled that
//! translation (a control bitmask + an `observe → step → lower` cycle). This
//! module owns that *exact* pattern once, so any game reuses it through a single
//! facade instead of re-implementing it.
//!
//! ## What it owns
//! - The **first-person held-control vocabulary** (a `u32` bitmask:
//!   forward / back / turn / strafe / two action buttons) — the shared meaning of
//!   a neutral `control_code`.
//! - [`AgentHarnessApi::decide_hold`] — hold a fixed control this tick (the
//!   substrate echoes it, producing a real decision report). A one-shot
//!   "hold forward" is `decide_hold(.., FORWARD)`.
//! - [`AgentHarnessApi::decide_seek`] — a deterministic navigation policy that
//!   turns toward a goal point and walks to it, expressed as a held-control
//!   bitmask and routed through the substrate. Generic: the game passes its own
//!   forward vector, so no yaw convention is baked in.
//!
//! Every observation the harness builds carries the **self pose including the
//! player's height** (in the fact's `y`), so the agent genuinely perceives — and
//! can report — how high the player is.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** thing: the [`AgentHarnessApi`] facade. All the
//! agent contract types it composes stay sealed inside [`axiom_agent`]; the
//! harness traffics only in primitives and tuples across its boundary, so an app
//! can drive an agent without ever naming an `axiom-agent` type.

mod agent_harness_api;

pub use agent_harness_api::AgentHarnessApi;
