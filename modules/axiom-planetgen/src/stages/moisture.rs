//! `moisture`: ocean-distance BFS baseline moisture.
//!
//! Thin wrapper over the `hydrology` layer: it marks ocean regions
//! (`elevation < 0`), calls the multi-source ocean-distance solver
//! ([`axiom_hydrology::ocean_distance`]), and folds the hop distances into a
//! `[0,1]` field (1 at the coast, decaying with distance). An all-land world gets
//! a flat dry baseline. Branchless: a per-region `map` with a table-select.

use axiom_hydrology::ocean_distance;

use crate::globe::PlanetGlobe;

/// Dry baseline for a world with no ocean anywhere.
const DRY_BASELINE: f32 = 0.2;

pub(crate) fn moisture(globe: &mut PlanetGlobe) {
    let region_count = globe.region_count();
    let is_ocean: Vec<bool> = globe.region_elevation.iter().map(|&e| e < 0.0).collect();
    let has_ocean = is_ocean.iter().any(|&o| o);

    let dist = ocean_distance(&globe.graph, &is_ocean);
    let max_dist = dist.iter().filter_map(|d| d.steps()).max().unwrap_or(0);
    let denom = max_dist.max(1) as f32;

    let region_moisture: Vec<f32> = (0..region_count)
        .map(|r| {
            // Unreached interior sits at the far end of the gradient.
            let d = dist[r].steps().unwrap_or(max_dist);
            let gradient = (1.0 - (d as f32 / denom)).clamp(0.0, 1.0);
            [DRY_BASELINE, gradient][usize::from(has_ocean)]
        })
        .collect();
    globe.region_moisture = region_moisture;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_geosphere::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// Line 0-1-2-3-4.
    fn line_globe(elev: Vec<f32>) -> PlanetGlobe {
        let n = elev.len();
        let mut offsets = vec![0u32];
        let mut neighbours = Vec::new();
        (0..n).for_each(|i| {
            (i > 0).then(|| neighbours.push((i - 1) as u32));
            (i + 1 < n).then(|| neighbours.push((i + 1) as u32));
            offsets.push(neighbours.len() as u32);
        });
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites: vec![Vec3::new(1.0, 0.0, 0.0); n],
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
        g.region_elevation = elev;
        g
    }

    #[test]
    fn moisture_in_unit_range_and_decays() {
        let mut g = line_globe(vec![-1.0, 0.5, 0.5, 0.5, 0.5]);
        moisture(&mut g);
        assert!(g.region_moisture.iter().all(|&m| (0.0..=1.0).contains(&m)));
        assert!(g.region_moisture[0] >= g.region_moisture[4]);
        assert!(g.region_moisture[1] > g.region_moisture[3]);
    }

    #[test]
    fn no_ocean_gets_baseline() {
        let mut g = line_globe(vec![1.0, 1.0, 1.0]);
        moisture(&mut g);
        assert!(g.region_moisture.iter().all(|&m| m == DRY_BASELINE));
    }

    #[test]
    fn empty_globe_is_a_noop() {
        let mut g = line_globe(Vec::new());
        moisture(&mut g);
        assert!(g.region_moisture.is_empty());
    }

    #[test]
    fn deterministic_same_input() {
        let mut a = line_globe(vec![-1.0, 0.5, 0.5, 0.5, 0.5]);
        let mut b = line_globe(vec![-1.0, 0.5, 0.5, 0.5, 0.5]);
        moisture(&mut a);
        moisture(&mut b);
        assert_eq!(a.region_moisture, b.region_moisture);
    }
}
