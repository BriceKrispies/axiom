//! # Axiom — Forest Walk (browser/WASM app)
//!
//! A first-person walk through the `prologue_postcard_001` forest. The visual
//! target is a *static* diorama rendered from a fixed camera to a PNG; this app
//! makes it **playable**: the same forest geometry ([`visual_target::build::build`]
//! over the champion manifest, baked into the bundle) is uploaded once, and each
//! frame a first-person camera — driven by WASD + mouse-look via
//! `axiom-fp-controller`, seated on the terrain surface (ground-follow) —
//! re-projects every instance and presents it live through the engine's
//! `axiom-windowing` WebGPU → WebGL2 → Canvas 2D cascade.
//!
//! Extracted from the merged `apps/axiom-gallery` crate back into its own
//! composition-leaf app (the gallery de-merge). The wasm entry keeps its
//! namespaced `forest_walk_start` name. Because an app may not depend on another
//! app, this crate carries its own copy of the diorama builder the demo consumes
//! ([`visual_target`]: manifest → deterministic render batches, plus the
//! [`curves`] interpolation helpers) — the comparator/review half of the gallery's
//! visual-target machinery stays behind in the gallery's growth demo.
//!
//! The walk itself is wasm32-only (it is the browser presentation arm); the
//! diorama builder is pure and compiles natively so its unit tests run in
//! `cargo test`.

pub mod curves;
pub mod visual_target;

#[cfg(target_arch = "wasm32")]
mod walk;

#[cfg(target_arch = "wasm32")]
pub use walk::forest_walk_start;

#[cfg(target_arch = "wasm32")]
pub mod overlay;
