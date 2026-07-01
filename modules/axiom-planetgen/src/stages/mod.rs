//! The thirteen worldgen stages as pure, branchless free functions.
//!
//! Each stage is a `(&mut PlanetGlobe, ...)` transform run in a fixed order by
//! [`crate::PlanetGenApi::generate`] — there is no stage registry, no
//! `Box<dyn Fn>`, and no runner loop. The order *is* the pipeline contract,
//! expressed as a straight-line call sequence in the facade. Stages that need
//! drainage math wrap the `hydrology` layer; elevation detail comes from the
//! `noise` layer's FBM; the two tectonic stages draw from an `entropy` stream.

mod elevation;
mod erosion;
mod fit_land_coverage;
mod moisture;
mod moisture_advection;
mod plate_properties;
mod priority_flood;
mod rain_shadow;
mod rivers;
mod tectonic_plates;
mod triangle_values;
mod wind_field;

pub(crate) use elevation::elevation;
pub(crate) use erosion::erosion;
pub(crate) use fit_land_coverage::fit_land_coverage;
pub(crate) use moisture::moisture;
pub(crate) use moisture_advection::moisture_advection;
pub(crate) use plate_properties::plate_properties;
pub(crate) use priority_flood::priority_flood;
pub(crate) use rain_shadow::rain_shadow;
pub(crate) use rivers::{river_carve, river_downflow, river_flow};
pub(crate) use tectonic_plates::tectonic_plates;
pub(crate) use triangle_values::triangle_values;
pub(crate) use wind_field::wind_field;
