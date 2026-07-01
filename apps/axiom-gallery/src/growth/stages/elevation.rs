//! `elevation` stage: tectonic-boundary uplift + FBM detail on the plate base.
//! Audit: worldgen `elevation`; OW-E16 uplift from plate seams.
//!
//! Starting from the plate base elevation (`plate_properties`), regions on a
//! plate boundary (a neighbour belongs to a different plate) get a ridge bump —
//! collision/divergence heuristically modelled as uplift. A deterministic
//! [`Fbm`] field keyed off `ctx.seed` adds fractal detail so the relief is not
//! flat per plate. Writes `region_elevation`.

use crate::growth::ids::RegionId;
use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};
use axiom_noise::{Fbm, FbmConfig, Frequency};

/// Height added at a plate boundary (mountain ridge / island arc).
const BOUNDARY_UPLIFT: f32 = 0.55;
/// Amplitude of the FBM detail layer.
const DETAIL_AMPLITUDE: f32 = 0.35;
/// FBM octaves / base frequency for elevation detail.
const DETAIL_OCTAVES: u32 = 5;
const DETAIL_FREQUENCY: f32 = 1.8;

pub struct ElevationStage;

impl Stage for ElevationStage {
    fn id(&self) -> &'static str {
        "elevation"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let region_count = globe.region_count();
        if globe.region_elevation.len() != region_count {
            globe.region_elevation.resize(region_count, 0.0);
        }

        let fbm = Fbm::new(
            ctx.seed ^ 0x_E1E7_A710,
            FbmConfig::new(
                DETAIL_OCTAVES,
                Frequency::new(DETAIL_FREQUENCY).expect("DETAIL_FREQUENCY is finite"),
            ),
        );

        // Boundary uplift falls off with how many neighbours share the plate:
        // an isolated seam region is a peak; an interior region is unchanged.
        let mut uplift = vec![0.0f32; region_count];
        for (r, slot) in uplift.iter_mut().enumerate() {
            let my_plate = globe.region_plate[r];
            let neighbours = globe.graph.neighbours_of(RegionId(r as u32));
            if neighbours.is_empty() {
                continue;
            }
            let mut foreign = 0usize;
            for &n in neighbours {
                if globe.region_plate[n as usize] != my_plate {
                    foreign += 1;
                }
            }
            if foreign > 0 {
                let frac = foreign as f32 / neighbours.len() as f32;
                *slot = BOUNDARY_UPLIFT * frac;
            }
        }

        for (r, &up) in uplift.iter().enumerate() {
            let site = globe.topology.sites[r];
            let detail = fbm.sample(site).get() * DETAIL_AMPLITUDE;
            globe.region_elevation[r] += up + detail;
        }

        ctx.log
            .push("elevation: boundary uplift + fbm detail".to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::model_planet::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// 4 regions in a ring: 0-1-2-3-0. Plates split 0,1 vs 2,3 so regions 1 and 2
    /// (and the wrap edge 3-0) sit on the boundary.
    fn ring_globe() -> PlanetGlobe {
        let sites = vec![
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        ];
        // CSR ring adjacency.
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
        for e in g.region_elevation.iter_mut() {
            *e = 0.0;
        }
        g
    }

    #[test]
    fn boundary_regions_get_uplift() {
        let mut g = ring_globe();
        let mut ctx = GenContext::new(5);
        ElevationStage.run(&mut g, &mut ctx);
        // Every region in this ring touches the other plate, so all get uplift.
        // With Fbm a stub (0), elevation equals uplift only; assert positive.
        for r in 0..g.region_count() {
            assert!(
                g.region_elevation[r] > 0.0,
                "region {} expected uplift, got {}",
                r,
                g.region_elevation[r]
            );
        }
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = ring_globe();
        let mut b = ring_globe();
        let mut ca = GenContext::new(321);
        let mut cb = GenContext::new(321);
        ElevationStage.run(&mut a, &mut ca);
        ElevationStage.run(&mut b, &mut cb);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
