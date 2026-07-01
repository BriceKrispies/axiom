//! Re-export of the graduated planet data model.
//!
//! The overworld data shapes — the durable [`PlanetSurfaceAtlas`], the
//! [`SurfaceSample`] a query returns, and the [`RegionLocator`] the atlas carries
//! — now live in the `axiom-planetgen` feature module (which owns the whole
//! generation pipeline). The app names them through this one re-export so the
//! streaming, gameplay and rendering code keeps its `crate::growth::model_planet`
//! import path. The neutral spherical topology it composes stays in the
//! `axiom-geosphere` layer.

pub use axiom_geosphere::{Icosphere, RegionGraph};
pub use axiom_planetgen::{PlanetSurfaceAtlas, RegionLocator, SurfaceSample};
