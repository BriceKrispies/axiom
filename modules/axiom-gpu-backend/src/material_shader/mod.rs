//! The runtime material shader: `materials/shader.js`, as hand-written WGSL.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js` (890 lines) — the
//! runtime half of that project's material system. The *bake* half is already
//! ported: nineteen procedural surface generators live in the app and produce
//! real albedo/roughness/metalness/normal data. This is the shader that samples
//! them.
//!
//! ## Why hand-written WGSL and not the field algebra
//!
//! The field algebra (`axiom-field` / `axiom-surface`) has no control flow, no
//! loops, no division, no derivatives and no texture sampling, with a budget of
//! 256 nodes per *whole surface*. Those absences are deliberate and the
//! branchlessness is the Branchless Law itself, so they are immovable.
//!
//! This shader needs exactly what the algebra refuses: parallax occlusion
//! mapping is a bounded loop with a linear refine, de-tiling needs `textureGrad`
//! with explicit derivatives, and triplanar is nine fetches. So the port splits
//! along the seam the source already has — a bake to a target, then a runtime
//! shader that reads it. The algebra keeps bake time, which is what it is good
//! at. The argument in full is in `docs/work-manifests/shmup-port/01-engine-gaps.md`.
//!
//! ## Where it plugs in
//!
//! [`crate::scene_wgsl`] is a prefix, a program-shaped hole, and a suffix:
//!
//! ```wgsl
//! fn axiom_surface(in: SurfaceIn, params: SurfaceParams) -> SurfaceOut
//! ```
//!
//! Today that hole takes either `DEFAULT_SURFACE_WGSL` or WGSL generated from an
//! authored `axiom_surface::Surface`. This module is a **third filling**:
//! hand-written WGSL honouring the same signature. Nothing about the lighting
//! maths, the PCF shadow lookup, the hemisphere ambient, the fog or the tonemap
//! changes — a surface program supplies channel values, never a way of being lit
//! — and there is no new pipeline mechanism, because the existing
//! content-addressed program identity already gives one pipeline per distinct
//! program.
//!
//! ## The shape of a layer
//!
//! One source section per submodule, and each submodule owns three things that
//! land together:
//!
//! 1. the **WGSL** for that layer, as a `&str` constant;
//! 2. a **CPU reference** in Rust computing the same maths;
//! 3. a **parity test** proving the two agree on a real adapter, in the shape
//!    `crate::surface_program::parity` already establishes.
//!
//! Layers are written as free functions taking explicit arguments — textures and
//! samplers included, which WGSL permits — rather than reaching for globals. That
//! is what lets them be composed here, tested in isolation, and written in
//! parallel without sharing a file.
//!
//! ## The laws
//!
//! This is the spine, not an app. The Rust that assembles the WGSL contains zero
//! control flow (Branchless Law) and is 100% covered (Coverage Law). The WGSL
//! *itself* has loops — parallax occlusion mapping is one — and that is fine:
//! `engine_no_branching` reads Rust HIR, and a loop inside a `&str` is shader
//! text, which is data.

pub(crate) mod cloth;
pub(crate) mod compose;
pub(crate) mod detail;
pub(crate) mod detile;
pub(crate) mod frames;
pub(crate) mod macro_variation;
pub(crate) mod masks;
pub(crate) mod params;
pub(crate) mod patches;
pub(crate) mod pom;
pub(crate) mod tint_wear;
pub(crate) mod uv_mode;
pub(crate) mod weathering;
