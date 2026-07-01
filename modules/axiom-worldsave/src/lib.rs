//! # Axiom WorldSave — the save/delta model (feature module)
//!
//! The procedural-generation payoff (roadmap Phase 12): a world reproduces from a
//! tiny save, not a stored copy. [`WorldSaveApi::save`] captures only the
//! regeneration inputs — `seed`, generator version, the world's address +
//! dimensions — plus the player's **deltas** (per-cell biome overrides);
//! [`WorldSaveApi::restore`] regenerates the levelgen world from the seed and
//! replays the deltas on top, byte-for-byte.
//!
//! ## Why a feature module
//! It composes the `levelgen` world recipe (regenerating a [`axiom_levelgen`]
//! world), which an engine module may not do (`allowed_modules` must be empty). A
//! **feature module** (`kind = "feature-module"`) is the sanctioned exception. It
//! depends on `kernel` + `space` and the one module it lists.
//!
//! ## The save/delta and multiplayer payoff
//! A naive save stores the whole generated world; this stores only what cannot be
//! regenerated. `Save::to_bytes` is far smaller than the world it rebuilds (a test
//! pins this). The **same shape** powers lockstep multiplayer — a peer ships
//! `{seed, versions, command/delta stream}`, never full state — and because the
//! whole generation stack is integer-only and deterministic (the
//! `axiom-proc-fuzz` gate proves it across 2000 seeds), the regenerated world is
//! byte-identical on every platform, so server and browser agree.
//!
//! ## Public surface
//! One facade: [`WorldSaveApi`]. The `Save` and `SavedWorld` it returns are read
//! through their own methods.

mod save;
mod saved_world;
mod worldsave_api;

pub use worldsave_api::WorldSaveApi;
