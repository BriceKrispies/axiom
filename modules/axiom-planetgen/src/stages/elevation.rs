//! `elevation`: tectonic-boundary uplift + FBM detail on the plate base.
//!
//! Regions on a plate boundary (a neighbour on a different plate) get a ridge
//! bump proportional to the foreign-neighbour fraction; a deterministic
//! [`Fbm`] field keyed off the seed adds fractal detail. Writes `region_elevation`.
//! Branchless: the uplift and detail are per-region `map`s.

use axiom_geosphere::RegionId;
use axiom_noise::{Fbm, FbmConfig, Frequency};

use crate::globe::PlanetGlobe;

/// Height added at a plate boundary (mountain ridge / island arc).
const BOUNDARY_UPLIFT: f32 = 0.55;
/// Amplitude of the FBM detail layer.
const DETAIL_AMPLITUDE: f32 = 0.35;
/// FBM octaves / base frequency for elevation detail.
const DETAIL_OCTAVES: u32 = 5;
const DETAIL_FREQUENCY: f32 = 1.8;

/// Boundary uplift for region `r`: `BOUNDARY_UPLIFT` scaled by the fraction of
/// its neighbours on a different plate (an isolated region contributes 0).
fn boundary_uplift(globe: &PlanetGlobe, r: usize) -> f32 {
    let my_plate = globe.region_plate[r];
    let neighbours = globe.graph.neighbours_of(RegionId(r as u32));
    let foreign = neighbours
        .iter()
        .filter(|&&n| globe.region_plate[n as usize] != my_plate)
        .count();
    let denom = neighbours.len().max(1) as f32;
    BOUNDARY_UPLIFT * (foreign as f32 / denom)
}

pub(crate) fn elevation(globe: &mut PlanetGlobe, seed: u64) {
    let region_count = globe.region_count();
    let fbm = Fbm::new(
        seed ^ 0x_E1E7_A710,
        FbmConfig::new(DETAIL_OCTAVES, Frequency::finite_or_zero(DETAIL_FREQUENCY)),
    );

    let uplift: Vec<f32> = (0..region_count)
        .map(|r| boundary_uplift(globe, r))
        .collect();

    let region_elevation: Vec<f32> = (0..region_count)
        .map(|r| {
            let site = globe.topology.sites[r];
            let detail = fbm.sample(site).get() * DETAIL_AMPLITUDE;
            globe.region_elevation[r] + uplift[r] + detail
        })
        .collect();
    globe.region_elevation = region_elevation;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_geosphere::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// 4 regions in a ring 0-1-2-3-0; plates split {0,1} vs {2,3} so every region
    /// sits on the boundary.
    fn ring_globe() -> PlanetGlobe {
        let sites = vec![
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        ];
        let offsets = vec![0u32, 2, 4, 6, 8];
        let neighbours = vec![1, 3, 0, 2, 1, 3, 2, 0];
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites,
                triangles: Vec::new(),
                subdivisions: 0,
            },
            graph: RegionGraph {
                offsets,
                neighbours,
            },
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        g.region_plate = vec![0, 0, 1, 1];
        g
    }

    #[test]
    fn boundary_regions_get_uplift() {
        let mut g = ring_globe();
        elevation(&mut g, 5);
        assert!(g.region_elevation.iter().all(|&e| e > 0.0));
    }

    #[test]
    fn isolated_region_gets_no_uplift() {
        // A single region with no neighbours: uplift 0, only FBM detail.
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites: vec![Vec3::new(0.0, 1.0, 0.0)],
                triangles: Vec::new(),
                subdivisions: 0,
            },
            graph: RegionGraph::default(),
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        assert_eq!(boundary_uplift(&g, 0), 0.0);
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = ring_globe();
        let mut b = ring_globe();
        elevation(&mut a, 321);
        elevation(&mut b, 321);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
