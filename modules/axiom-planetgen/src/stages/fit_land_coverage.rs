//! `fit_land_coverage`: shift all elevations by a constant so the land fraction
//! matches `land_target`, with sea level fixed at 0.
//!
//! Land is `elevation >= 0`. Adding a constant offset monotonically changes the
//! land fraction, so a bisection over the offset (bounded by the actual min/max
//! elevation) hits any reachable target. Branchless: the bisection is a `fold`
//! over a fixed iteration count, and the offset is applied with `iter_mut`.

use crate::globe::PlanetGlobe;

/// Bisection iterations for the sea-level offset.
const SEARCH_ITERS: u32 = 48;

/// Land fraction if `offset` were added to every region elevation (0 for empty).
fn land_fraction_with(elev: &[f32], offset: f32) -> f32 {
    let land = elev.iter().filter(|&&e| e + offset >= 0.0).count();
    land as f32 / elev.len().max(1) as f32
}

pub(crate) fn fit_land_coverage(globe: &mut PlanetGlobe, land_target: f32) {
    let elev = &globe.region_elevation;
    let target = land_target.clamp(0.0, 1.0);

    let (min_e, max_e) = elev
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &e| {
            (lo.min(e), hi.max(e))
        });

    // offset in [-max_e - 1 .. -min_e + 1]: at lo ~0 land, at hi ~all land. Land
    // fraction is monotone increasing in offset, so bisect toward `target`.
    let (lo, hi) = (0..SEARCH_ITERS).fold((-max_e - 1.0, -min_e + 1.0), |(lo, hi), _| {
        let mid = 0.5 * (lo + hi);
        let need_more = land_fraction_with(elev, mid) < target;
        // need_more → raise the low bound to mid; else lower the high bound.
        [(lo, mid), (mid, hi)][usize::from(need_more)]
    });
    let offset = 0.5 * (lo + hi);

    globe.region_elevation.iter_mut().for_each(|e| *e += offset);
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_geosphere::{Icosphere, RegionGraph};
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

    /// Land fraction (elevation >= 0) after running the stage to `target`.
    fn run_target(elev: Vec<f32>, target: f32) -> f32 {
        let mut g = globe_with_elev(elev);
        fit_land_coverage(&mut g, target);
        let land = g.region_elevation.iter().filter(|&&e| e >= 0.0).count();
        land as f32 / g.region_elevation.len().max(1) as f32
    }

    #[test]
    fn hits_targets_within_five_percent() {
        // A deterministic spread across [-2, 2).
        let elev: Vec<f32> = (0..1000)
            .map(|i| ((i * 7919 % 400) as f32 / 100.0) - 2.0)
            .collect();
        [0.1f32, 0.3, 0.5, 0.7, 0.9].iter().for_each(|&target| {
            let result = run_target(elev.clone(), target);
            assert!(
                (result - target).abs() <= 0.05,
                "target {target} got {result}"
            );
        });
    }

    #[test]
    fn extremes_clamp() {
        let elev: Vec<f32> = (0..100).map(|i| i as f32 * 0.01 - 0.5).collect();
        assert!(run_target(elev.clone(), 0.0) <= 0.05);
        assert!(run_target(elev, 1.0) >= 0.95);
    }

    #[test]
    fn empty_globe_stays_empty() {
        let mut g = globe_with_elev(Vec::new());
        fit_land_coverage(&mut g, 0.4);
        assert!(g.region_elevation.is_empty());
    }

    #[test]
    fn deterministic_same_target() {
        let elev: Vec<f32> = (0..200).map(|i| (i as f32 * 0.013).sin()).collect();
        let mut a = globe_with_elev(elev.clone());
        let mut b = globe_with_elev(elev);
        fit_land_coverage(&mut a, 0.4);
        fit_land_coverage(&mut b, 0.4);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
