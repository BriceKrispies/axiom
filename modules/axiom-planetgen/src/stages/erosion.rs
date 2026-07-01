//! `erosion`: iterative stream-power incision over the region graph.
//!
//! Thin wrapper over the `hydrology` layer: it lifts the region elevation into
//! `Meters`, runs the requested passes of
//! [`axiom_hydrology::stream_power_erosion`] at the erosion strength, and writes
//! the eroded field back. The layer owns the per-pass cap and the branchless
//! relaxation.

use axiom_hydrology::stream_power_erosion;
use axiom_kernel::{Meters, Ratio};

use crate::globe::PlanetGlobe;

/// Per-pass erosion strength applied to the slope toward the lowest neighbour.
const EROSION_K: f32 = 0.10;

pub(crate) fn erosion(globe: &mut PlanetGlobe, erosion_iters: u32) {
    let elevation: Vec<Meters> = globe
        .region_elevation
        .iter()
        .map(|&e| Meters::finite_or_zero(e))
        .collect();
    let strength = Ratio::finite_or_zero(EROSION_K);
    let eroded = stream_power_erosion(&globe.graph, &elevation, strength, erosion_iters);
    globe.region_elevation = eroded.into_iter().map(|m| m.get()).collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_geosphere::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// A line of regions 0-1-2-3-4 with a peak in the middle.
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
    fn peak_is_lowered() {
        let mut g = line_globe(vec![0.0, 1.0, 5.0, 1.0, 0.0]);
        let before = g.region_elevation[2];
        erosion(&mut g, 30);
        assert!(g.region_elevation[2] < before);
    }

    #[test]
    fn empty_globe_is_a_noop() {
        let mut g = line_globe(Vec::new());
        erosion(&mut g, 10);
        assert!(g.region_elevation.is_empty());
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = line_globe(vec![0.0, 2.0, 4.0, 2.0, 0.0]);
        let mut b = line_globe(vec![0.0, 2.0, 4.0, 2.0, 0.0]);
        erosion(&mut a, 20);
        erosion(&mut b, 20);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
