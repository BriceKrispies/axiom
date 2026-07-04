//! # Generated Micro-FPS — a recipe project
//!
//! A complete, small, playable industrial sci-fi *training facility*, generated
//! from recipes by the existing Axiom procedural pipeline. Nothing here is a new
//! engine crate or a new procedural operator: the project only *composes* the
//! existing texture/mesh operators (`axiom-proc-texture`, `axiom-proc-mesh`) into
//! a hierarchy of reusable recipe macros, then expands them into ordinary Axiom
//! runtime resources and a seeded scene with a gameplay ruleset.
//!
//! Read the modules bottom-up:
//! - [`style`] — the one [`style::Style`]: seed, palette, art-direction knobs.
//! - [`textures`] / [`meshes`] — reusable *recipe macros* (wall, floor, door,
//!   crate, pipe, light, enemy bodies, weapon, exit) built from the operators.
//! - [`materials`] — the shared-palette material bindings.
//! - [`prefabs`] — mesh + material + local transform + gameplay tag bundles.
//! - [`grammar`] — seeded composition of prefabs into room shells, corridors, and
//!   combat rooms (nothing hand-placed).
//! - [`scenes`] — the title/menu scene and the three-area level.
//! - [`gameplay`] — the deterministic ruleset (spawn, health, pickup, hitscan,
//!   damage, death, gate unlock, win).
//! - [`pack`] — the packed-recipe export and the size report.

pub mod gameplay;
pub mod grammar;
pub mod materials;
pub mod meshes;
pub mod pack;
pub mod prefabs;
pub mod scenes;
pub mod style;
pub mod textures;
pub mod validation;

/// The browser (wasm32) entry — the live windowing present loop.
#[cfg(target_arch = "wasm32")]
pub mod web;

pub use pack::{PackedProject, SizeReport};
pub use scenes::{expand_level, expand_menu, ExpandedLevel};
pub use style::Style;
