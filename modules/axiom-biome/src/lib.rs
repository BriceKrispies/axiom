//! # Axiom Biome — deterministic biome classification (engine module)
//!
//! A **domain generator** on the procedural-generation substrate (roadmap Phase
//! 9): [`BiomeApi::classify`] buckets an `(elevation, moisture)` pair into a biome
//! with a branchless band lookup, and [`BiomeApi::map`] generates a whole field by
//! drawing per-cell elevation/moisture from an entropy stream keyed by a content
//! address.
//!
//! ## Where the domain rules live
//! The thresholds that decide ocean vs forest vs peak are **biome's** concern,
//! not the generic `proc-validate` layer's (which only knows neutral words). This
//! is the Phase 9 split: generic validation/scoring is a *layer*; "what makes a
//! biome a biome" is a *domain module*.
//!
//! ## What it is, and is not
//! - A reusable **engine module** depending on `space` + `entropy` (+ `kernel`)
//!   and on **no other module** — notably **not `terrain`**: a caller composes a
//!   heightfield with a biome map; biome classifies the values it is given or
//!   draws, it never imports terrain.
//! - **Integer-only and branchless.** Biome codes are small `u8`s with named
//!   constants ([`BiomeApi::OCEAN`] … [`BiomeApi::PEAK`]), so the facade stays
//!   single (Module Law #8) without a separate category type. No browser/platform
//!   APIs.
//!
//! ## Public surface
//! One facade: [`BiomeApi`] (with its biome-code constants). The `BiomeMap` it
//! returns is read through its own methods.

mod biome_api;
mod biome_map;

pub use biome_api::BiomeApi;
