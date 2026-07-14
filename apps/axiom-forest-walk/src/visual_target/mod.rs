//! # The `prologue_postcard_001` diorama builder — the walk's forest geometry.
//!
//! A deliberately boring pipeline: a **fixed, versioned scene manifest**
//! ([`scene::Manifest`]) describing a camera, a sun, fog, a terrain patch, ground
//! materials, and vegetation instances → **neutral render data**
//! ([`build::RenderData`]): meshes, materials, per-instance batches, lights, and
//! frame settings. There is **no** procedural world generation, survival, weather,
//! inventory, AI, or gameplay here — the whole diorama is a pure function of one
//! TOML file (the `axiom-terrain-mesh` heightfield mesher supplies the terrain
//! geometry; `axiom-entropy`/`axiom-space` drive the deterministic scatter).
//!
//! This is the forest-walk app's private copy of the gallery growth demo's
//! visual-target *diorama* half (an app may not depend on another app). The
//! convergence *comparator* half — axes, scorecards, ledger, review, pixel
//! compare — stays behind in `apps/axiom-gallery/src/growth/visual_target/`;
//! this app only needs manifest → render batches for the live first-person arm.

pub mod build;
pub mod scatter;
pub mod scene;

pub use build::{all_trees, build, RenderData};
pub use scene::Manifest;
