//! # Axiom Perception — Engine Module
//!
//! A deterministic, **game-agnostic** perception model for an embodied agent. It
//! turns raw spatial sense-data into the agent's neutral observation facts:
//!
//! ```text
//! ray-fan directions  ->  (app casts them against its world)  ->  obstacle facts
//! candidate entities  ->  view-cone cull  ->  visible facts
//! prior + current pos ->  relative motion  ->  tracked facts
//! ```
//!
//! ## What this module is
//! - An *isolated* engine module depending only on the kernel (the dimensioned
//!   [`axiom_kernel::Radians`] / [`axiom_kernel::Meters`] its API speaks in) and
//!   [`axiom_math`] (the `Vec3`/`Quat` geometry of fans and cones).
//! - The single owner of the **sensor geometry** (a horizontal ray fan, a forward
//!   view cone), the **subject-tracking** math, and the neutral **fact
//!   vocabulary** ([`PerceptionApi::FACT_OBSTACLE`] / `FACT_VISIBLE` /
//!   `FACT_TRACKED`) — facts emitted in the agent's `(kind, subject, x, y, z,
//!   value)` tuple shape, in fixed-point micro-units.
//!
//! ## What this module is not
//! It is **not** a renderer, a scene, or the agent. It never casts a ray itself —
//! casting a probe against the world is the app's job, because only the app knows
//! whether its world is scene entities (DOOM) or a procedural heightfield
//! (growth). It reads no clock and uses no randomness. By speaking the agent's
//! fact shape as plain tuples rather than importing `axiom-agent`, it stays
//! decoupled from both the scene it senses and the agent it feeds — the app wires
//! the two ends together.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** thing: the [`PerceptionApi`] facade.

mod perception_api;

pub use perception_api::PerceptionApi;
