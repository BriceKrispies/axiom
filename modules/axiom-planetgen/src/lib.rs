//! # Axiom PlanetGen — composed planet-generation pipeline (feature module)
//!
//! The **composition tier** of the procedural-planet pivot: the deterministic
//! worldgen pipeline that used to live in the growth app, graduated behind one
//! facade. [`PlanetGenApi::generate`] folds the `geosphere` topology and a fixed,
//! straight-line sequence of thirteen branchless worldgen stages into one durable
//! [`PlanetSurfaceAtlas`]; [`PlanetGenApi::sample`] / [`PlanetGenApi::locate`]
//! answer deterministic surface queries over it.
//!
//! ## Why a feature module
//! An engine module may never depend on another module (`allowed_modules = []`),
//! so no engine module could compose the `biome` climate lens with the `noise`,
//! `geosphere` and `hydrology` layers. A **feature module**
//! (`kind = "feature-module"`) is the sanctioned exception: it may depend on
//! exactly the modules it lists — here `biome` — plus the curated layers, the
//! same way `axiom-levelgen` composes terrain + biome + placement.
//!
//! ## Straight-line, branchless composition
//! [`PlanetGenApi::generate`] runs the stages as **direct calls in a fixed
//! order** — no stage registry, no `Box<dyn Fn>`, no `for` loop. The order is the
//! pipeline's contract, expressed as code, and the whole spine is branchless and
//! 100%-covered like every module. Each stage is a pure `(&mut globe, &params)`
//! transform; the ones that need drainage math wrap the `hydrology` layer, and
//! elevation detail comes from the `noise` layer's FBM.
//!
//! ## Determinism
//! Everything is keyed off the seed through an `axiom-entropy` stream: the same
//! [`PlanetGenParams`] always produces a byte-identical [`PlanetSurfaceAtlas`].
//!
//! ## Public surface
//! One facade: [`PlanetGenApi`], alongside the value-type vocabulary the facade
//! traffics in ([`PlanetGenParams`], [`PlanetSurfaceAtlas`], [`SurfaceSample`],
//! [`RegionLocator`], [`PlateId`], [`BiomeId`]).

mod atlas;
mod globe;
mod ids;
mod planet_gen_api;
mod query;
mod stages;

pub use ids::{
    BiomeId, PlanetGenParams, PlanetSurfaceAtlas, PlateId, RegionLocator, SurfaceSample,
};
pub use planet_gen_api::PlanetGenApi;
