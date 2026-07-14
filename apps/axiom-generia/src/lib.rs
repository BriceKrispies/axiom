//! # `generia` — a first-person walk through an **endless** procedural Axiom forest.
//!
//! The port target for the WAT-engine fall-forest game, on Axiom's GPU forest.
//! Phase 2: the world **streams**. An `axiom_world::WorldApi` residency ring
//! loads/unloads/culls chunks around the camera; each loaded chunk's trees are
//! placed by `axiom_scatter::ScatterApi` and turned into the same rich
//! trunk/foliage/branch instances the hero render uses
//! ([`visual_target::build`]'s `*_instances`); the ground is a terrain mesh
//! regenerated around the moving camera and streamed into the backend via
//! `run_web_multi_streaming`. Walk forever — chunks appear ahead and unload
//! behind, and only the visible ones are drawn.
//!
//! Later phases layer on the fall-forest game systems (layered terrain + rail
//! path, rule-based props, discoveries, world modes, the horror layer, a console).
//!
//! Extracted from the merged `apps/axiom-gallery` crate into its own composition
//! leaf. The crate splits into a browser-free core and a wasm-only shell:
//!
//! - [`visual_target`] + [`curves`] — the manifest-driven diorama vocabulary
//!   ([`visual_target::scene::Manifest`]), the scatter placer, and the
//!   trunk/foliage/branch/terrain instance builders. Pure and deterministic;
//!   compiled (and unit-tested) on native.
//! - `web` (wasm32 only) — the browser presentation arm: the streaming world
//!   ring, the first-person controls, and the `generia_start` entry the page
//!   boots. Native builds compile it away.

pub mod curves;
pub mod visual_target;

#[cfg(target_arch = "wasm32")]
pub mod overlay;
#[cfg(target_arch = "wasm32")]
mod web;
