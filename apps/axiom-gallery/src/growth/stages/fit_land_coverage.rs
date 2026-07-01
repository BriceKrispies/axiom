//! `fit_land_coverage` stage (OW-E21): shift all elevations by a constant so the
//! land fraction matches `ctx.land_target`, with sea level fixed at 0.
//! Audit: OW-E21 "Land (target) matches Land (result) at sea level 0".
//!
//! Land is `elevation >= 0`. Adding a constant offset to every region monotonically
//! changes the land fraction, so we binary-search the additive offset that makes
//! `globe.land_fraction()` hit the target. The offset bounds come from the actual
//! min/max elevation, so any reachable target inside [0,1] is hit closely.

use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};

/// Binary-search iterations for the sea-level offset.
const SEARCH_ITERS: u32 = 48;

pub struct FitLandCoverageStage;

/// Land fraction if `offset` were added to every region elevation.
fn land_fraction_with(elev: &[f32], offset: f32) -> f32 {
    if elev.is_empty() {
        return 0.0;
    }
    let land = elev.iter().filter(|&&e| e + offset >= 0.0).count();
    land as f32 / elev.len() as f32
}

impl Stage for FitLandCoverageStage {
    fn id(&self) -> &'static str {
        "fit_land_coverage"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let elev = &globe.region_elevation;
        if elev.is_empty() {
            return;
        }
        let target = ctx.land_target.clamp(0.0, 1.0);

        let mut min_e = f32::INFINITY;
        let mut max_e = f32::NEG_INFINITY;
        for &e in elev.iter() {
            if e < min_e {
                min_e = e;
            }
            if e > max_e {
                max_e = e;
            }
        }

        // offset in [-max_e .. -min_e + eps]: at lo all land, at hi all ocean.
        // Larger offset → more land, so land_fraction is monotonically increasing
        // in offset. Search for the offset that gives `target`.
        let mut lo = -max_e - 1.0; // everything below sea → ~0 land
        let mut hi = -min_e + 1.0; // everything above sea → ~1 land
        for _ in 0..SEARCH_ITERS {
            let mid = 0.5 * (lo + hi);
            let frac = land_fraction_with(elev, mid);
            if frac < target {
                lo = mid; // need more land → larger offset
            } else {
                hi = mid;
            }
        }
        let offset = 0.5 * (lo + hi);

        for e in globe.region_elevation.iter_mut() {
            *e += offset;
        }

        ctx.log.push(format!(
            "fit_land_coverage: target {:.3} -> result {:.3} (offset {:.4})",
            target,
            globe.land_fraction(),
            offset
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::distributions;
    use crate::growth::model_planet::{Icosphere, RegionGraph};
    use crate::growth::pipeline::worldgen_stream;
    use axiom_math::Vec3;

    fn globe_with_elev(elev: Vec<f32>) -> PlanetGlobe {
        let n = elev.len();
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites: vec![Vec3::new(1.0, 0.0, 0.0); n],
                triangles: Vec::new(),
                subdivisions: 0,
            },
            graph: RegionGraph::default(),
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        g.region_elevation = elev;
        g
    }

    fn run_target(elev: Vec<f32>, target: f32) -> f32 {
        let mut g = globe_with_elev(elev);
        let mut ctx = GenContext::new(1);
        ctx.land_target = target;
        FitLandCoverageStage.run(&mut g, &mut ctx);
        g.land_fraction()
    }

    #[test]
    fn hits_targets_within_five_percent() {
        // 1000 random elevations.
        let mut rng = worldgen_stream(42);
        let elev: Vec<f32> = (0..1000)
            .map(|_| distributions::range(&mut rng, -2.0, 2.0))
            .collect();
        for &target in &[0.1f32, 0.3, 0.5, 0.7, 0.9] {
            let result = run_target(elev.clone(), target);
            assert!(
                (result - target).abs() <= 0.05,
                "target {} got {}",
                target,
                result
            );
        }
    }

    #[test]
    fn extremes_clamp() {
        let elev: Vec<f32> = (0..100).map(|i| i as f32 * 0.01 - 0.5).collect();
        assert!(run_target(elev.clone(), 0.0) <= 0.05);
        assert!(run_target(elev, 1.0) >= 0.95);
    }

    #[test]
    fn deterministic_same_seed() {
        let elev: Vec<f32> = (0..200).map(|i| (i as f32 * 0.013).sin()).collect();
        let mut a = globe_with_elev(elev.clone());
        let mut b = globe_with_elev(elev);
        let mut ca = GenContext::new(1);
        let mut cb = GenContext::new(1);
        ca.land_target = 0.4;
        cb.land_target = 0.4;
        FitLandCoverageStage.run(&mut a, &mut ca);
        FitLandCoverageStage.run(&mut b, &mut cb);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
