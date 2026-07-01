//! Internal mutable generation state: the flat per-region / per-triangle scalar
//! fields every worldgen stage reads and writes in place.
//!
//! Topology (sites + triangles) is **fixed for one generation**; all geology,
//! climate, erosion and hydrology mutate these flat fields hung off the region
//! indices of the `geosphere` topology. This is a crate-internal working buffer —
//! the durable, queryable output is [`crate::PlanetSurfaceAtlas`], built from it
//! by [`crate::atlas`]. Not part of the public facade.

use axiom_geosphere::{Icosphere, RegionGraph};
use axiom_math::Vec3;

/// Mutable generation state. Stages read/write these flat fields in place; sea
/// level is fixed at 0 (audit: OW-E21 `fit_land_coverage`).
#[derive(Debug, Clone, Default)]
pub(crate) struct PlanetGlobe {
    pub(crate) topology: Icosphere,
    pub(crate) graph: RegionGraph,

    /// Plate id per region. Audit: `tectonic_plates`.
    pub(crate) region_plate: Vec<u32>,
    /// Whether each plate is oceanic. Audit: `plate_properties`.
    pub(crate) plate_oceanic: Vec<bool>,

    /// Elevation; `>= 0` is land after `fit_land_coverage`. Audit: OW-E21.
    pub(crate) region_elevation: Vec<f32>,
    /// Moisture in `[0,1]`. Audit: moisture / advection / rain shadow.
    pub(crate) region_moisture: Vec<f32>,
    /// Prevailing-wind tangent direction per region (unit). Audit: OW-E7.
    pub(crate) region_wind: Vec<Vec3>,
    /// Drainage / flow accumulation per region. Audit: priority_flood, rivers.
    pub(crate) region_flow: Vec<f32>,

    /// Triangle elevations averaged from regions. Audit: `triangle_values`.
    pub(crate) triangle_elevation: Vec<f32>,
    /// Per-triangle river flow. Audit: `river_flow`.
    pub(crate) triangle_flow: Vec<f32>,
}

impl PlanetGlobe {
    /// Number of regions (icosphere sites) in this globe.
    pub(crate) fn region_count(&self) -> usize {
        self.topology.region_count()
    }

    /// Allocate all per-region / per-triangle fields to match the topology.
    pub(crate) fn resize_fields(&mut self) {
        let r = self.region_count();
        let t = self.topology.triangles.len();
        self.region_plate.resize(r, 0);
        self.region_elevation.resize(r, 0.0);
        self.region_moisture.resize(r, 0.0);
        self.region_wind.resize(r, Vec3::new(1.0, 0.0, 0.0));
        self.region_flow.resize(r, 0.0);
        self.triangle_elevation.resize(t, 0.0);
        self.triangle_flow.resize(t, 0.0);
    }
}

/// Mean of a triangle's three corner-region values, or `0.0` when any corner
/// index is out of range. Shared by `triangle_values` (elevation) and
/// `river_carve` (flow). Branchless: three `get`s zipped into one mean, defaulted.
pub(crate) fn corner_mean(field: &[f32], tri: [u32; 3]) -> f32 {
    let [a, b, c] = tri;
    let ga = field.get(a as usize).copied();
    let gb = field.get(b as usize).copied();
    let gc = field.get(c as usize).copied();
    ga.zip(gb)
        .zip(gc)
        .map(|((x, y), z)| (x + y + z) / 3.0)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_mean_averages_in_range_and_zeroes_out_of_range() {
        let field = [0.0, 3.0, 6.0, 9.0];
        assert_eq!(corner_mean(&field, [0, 1, 2]), 3.0);
        assert_eq!(corner_mean(&field, [1, 2, 3]), 6.0);
        // Any out-of-range corner → 0.
        assert_eq!(corner_mean(&field, [0, 1, 99]), 0.0);
    }

    #[test]
    fn resize_fields_matches_topology_and_region_count() {
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites: vec![Vec3::new(1.0, 0.0, 0.0); 3],
                triangles: vec![[0, 1, 2]],
                subdivisions: 0,
            },
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        assert_eq!(g.region_count(), 3);
        assert_eq!(g.region_plate.len(), 3);
        assert_eq!(g.region_elevation.len(), 3);
        assert_eq!(g.region_moisture.len(), 3);
        assert_eq!(g.region_wind.len(), 3);
        assert_eq!(g.region_flow.len(), 3);
        assert_eq!(g.triangle_elevation.len(), 1);
        assert_eq!(g.triangle_flow.len(), 1);
        assert!(!format!("{g:?}").is_empty());
        let _ = g.clone();
    }
}
