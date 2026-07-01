//! The value-type vocabulary [`crate::PlanetGenApi`] traffics in: the generation
//! parameters it consumes, the durable atlas + surface sample it returns, the
//! coarse spatial locator the atlas carries, and the plate / biome id newtypes.
//!
//! These are plain data contracts (public fields, no behaviour) so an app can
//! name what the facade hands back and read it, exactly as the vertical-slice
//! modules expose `SceneSnapshot` / `RenderInput`. Scalar fields carry kernel
//! quantity types ([`Meters`] / [`Ratio`]) so no naked float reaches the surface.

use axiom_geosphere::{RegionGraph, RegionId};
use axiom_kernel::{Meters, Ratio};
use axiom_math::Vec3;

/// A tectonic plate index. Audit: worldgen `tectonic_plates`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PlateId(pub u32);

/// A derived biome id (a `axiom_biome::BiomeApi` climate code). Audit: OW-E3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct BiomeId(pub u32);

/// The neutral parameters [`crate::PlanetGenApi::generate`] consumes. All typed —
/// the seed keys the deterministic entropy stream, `radius_m` / `land_target`
/// carry kernel quantity types, and the counts quantise the topology + shape the
/// tectonics / erosion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanetGenParams {
    /// Deterministic root seed for the whole generation.
    pub seed: u64,
    /// Planet radius (metres), carried into the atlas for local-map scaling.
    pub radius_m: Meters,
    /// Target land fraction in `[0, 1]` (fitted by `fit_land_coverage`).
    pub land_target: Ratio,
    /// Requested region count (quantised to an icosphere subdivision level).
    pub site_target: u32,
    /// Number of tectonic plate seeds.
    pub plate_count: u32,
    /// Stream-power erosion iterations.
    pub erosion_iters: u32,
}

/// A single overworld surface query result. Audit: OW-E3 `sample_surface`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceSample {
    pub region: RegionId,
    pub plate: PlateId,
    pub elevation: Meters,
    pub moisture: Ratio,
    /// Derived at query time from latitude + elevation, not stored.
    pub temperature: Ratio,
    pub biome: BiomeId,
}

impl Default for SurfaceSample {
    fn default() -> Self {
        Self {
            region: RegionId::default(),
            plate: PlateId::default(),
            elevation: Meters::finite_or_zero(0.0),
            moisture: Ratio::finite_or_zero(0.0),
            temperature: Ratio::finite_or_zero(0.0),
            biome: BiomeId::default(),
        }
    }
}

/// The durable, queryable overworld output owned for the session.
/// Audit: OW-E1/E2 `PlanetSurfaceAtlas`, OW-E3 query API.
#[derive(Debug, Clone)]
pub struct PlanetSurfaceAtlas {
    /// Fixed region centre directions (unit).
    pub sites: Vec<Vec3>,
    pub graph: RegionGraph,
    pub region_plate: Vec<u32>,
    pub plate_oceanic: Vec<bool>,
    pub region_elevation: Vec<f32>,
    pub region_moisture: Vec<f32>,
    /// Planet radius in metres (from the genome, via the params).
    pub planet_radius_m: Meters,
    /// Coarse spatial index for fast `locate`. An empty locator falls back to a
    /// linear nearest-site scan.
    pub locator: RegionLocator,
}

impl Default for PlanetSurfaceAtlas {
    fn default() -> Self {
        Self {
            sites: Vec::new(),
            graph: RegionGraph::default(),
            region_plate: Vec::new(),
            plate_oceanic: Vec::new(),
            region_elevation: Vec::new(),
            region_moisture: Vec::new(),
            planet_radius_m: Meters::finite_or_zero(0.0),
            locator: RegionLocator::default(),
        }
    }
}

impl PlanetSurfaceAtlas {
    /// Number of regions in the atlas.
    pub fn region_count(&self) -> usize {
        self.sites.len()
    }
}

/// Coarse spatial acceleration for `locate(unit_dir)` so it is not an O(R) scan.
/// A uniform lat/long band grid over the unit sphere; an empty locator (default)
/// falls back to a linear scan.
#[derive(Debug, Clone, Default)]
pub struct RegionLocator {
    /// Coarse-cell → candidate region indices (implementation-defined binning).
    pub cell_regions: Vec<Vec<u32>>,
    /// Number of latitude/longitude bands the binning uses.
    pub bands: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_empty_and_zeroed() {
        let atlas = PlanetSurfaceAtlas::default();
        assert_eq!(atlas.region_count(), 0);
        assert_eq!(atlas.planet_radius_m.get(), 0.0);
        assert_eq!(atlas.locator.bands, 0);
        assert!(atlas.locator.cell_regions.is_empty());

        let sample = SurfaceSample::default();
        assert_eq!(sample.region, RegionId::default());
        assert_eq!(sample.plate, PlateId::default());
        assert_eq!(sample.elevation.get(), 0.0);
        assert_eq!(sample.moisture.get(), 0.0);
        assert_eq!(sample.temperature.get(), 0.0);
        assert_eq!(sample.biome, BiomeId::default());
    }

    #[test]
    fn value_types_are_debug_and_copy() {
        let params = PlanetGenParams {
            seed: 1,
            radius_m: Meters::finite_or_zero(6_371_000.0),
            land_target: Ratio::finite_or_zero(0.3),
            site_target: 1024,
            plate_count: 24,
            erosion_iters: 120,
        };
        let copy = params;
        assert_eq!(copy, params);
        assert!(!format!("{params:?}").is_empty());
        assert!(!format!("{:?}", PlateId(3)).is_empty());
        assert!(!format!("{:?}", BiomeId(2)).is_empty());
        assert!(!format!("{:?}", SurfaceSample::default()).is_empty());
        assert!(!format!("{:?}", PlanetSurfaceAtlas::default()).is_empty());
        assert!(!format!("{:?}", RegionLocator::default()).is_empty());
    }
}
