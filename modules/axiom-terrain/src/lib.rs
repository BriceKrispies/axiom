//! # Axiom Terrain — coherent value-noise heightfields (engine module)
//!
//! A **domain generator** on the procedural-generation substrate (roadmap Phase
//! 9): this is where coherent **noise** graduates from `apps/axiom-growth` into a
//! reusable, spine-tested engine module. [`TerrainApi::heightfield`] produces a
//! grid of integer heights by bilinearly interpolating lattice values, each drawn
//! from an entropy stream keyed by its lattice site.
//!
//! ## What it is, and is not
//! - A reusable **engine module** depending on the `space` + `entropy` (+ `kernel`)
//!   layers and on **no other module**. It does not use `proc`: noise is a spatial
//!   field, not a sequential recipe.
//! - **Integer-only** (heights are `i32` — no naked floats), **branchless**, and
//!   100%-covered. No geometry, no meshing, no browser/platform APIs — a
//!   heightfield is neutral data a caller turns into a mesh.
//!
//! ## The seam-coherence invariant
//! Lattice values are a pure function of **world** lattice coordinates, so any two
//! heightfields that overlap in world space agree on the overlap. Adjacent tiles
//! therefore share a **seamless** edge — the discipline `axiom-growth` had to get
//! right by hand, now an engine guarantee with a test to prove it.
//!
//! ## Public surface
//! One facade: [`TerrainApi`]. The `HeightField` it returns is read through its
//! own methods (`at` / `heights` / `digest` / canonical bytes).

mod height_field;
mod terrain_api;

pub use terrain_api::TerrainApi;
