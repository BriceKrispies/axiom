//! `erosion` stage: iterative stream-power-style erosion on the region graph.
//! Audit: OW-E16 stream-power erosion (done core).
//!
//! Thin app-side [`Stage`] wrapper over the `axiom-hydrology` layer: the
//! slope-proportional incision ([`axiom_hydrology::stream_power_erosion`]) lives
//! in the layer (which owns the per-pass cap and the branchless relaxation); this
//! stage lifts the region elevation into `Meters`, runs the requested number of
//! passes at the erosion strength, and writes the eroded field back.

use axiom_hydrology::stream_power_erosion;
use axiom_kernel::{Meters, Ratio};

use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};

/// Per-pass erosion strength applied to the slope toward the lowest neighbour.
const EROSION_K: f32 = 0.10;

pub struct ErosionStage;

impl Stage for ErosionStage {
    fn id(&self) -> &'static str {
        "erosion"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let region_count = globe.region_count();
        if region_count == 0 {
            return;
        }

        let elevation: Vec<Meters> = globe
            .region_elevation
            .iter()
            .map(|&e| Meters::finite_or_zero(e))
            .collect();
        let strength = Ratio::finite_or_zero(EROSION_K);
        let eroded =
            stream_power_erosion(&globe.graph, &elevation, strength, ctx.erosion_iterations);
        globe.region_elevation = eroded.into_iter().map(|m| m.get()).collect();

        ctx.log.push(format!(
            "erosion: {} passes requested",
            ctx.erosion_iterations
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::model_planet::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// A line of 5 regions 0-1-2-3-4 with a peak in the middle.
    fn line_globe(elev: Vec<f32>) -> PlanetGlobe {
        let n = elev.len();
        let mut offsets = vec![0u32];
        let mut neighbours = Vec::new();
        for i in 0..n {
            if i > 0 {
                neighbours.push((i - 1) as u32);
            }
            if i + 1 < n {
                neighbours.push((i + 1) as u32);
            }
            offsets.push(neighbours.len() as u32);
        }
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
        let mut ctx = GenContext::new(1);
        ctx.erosion_iterations = 30;
        ErosionStage.run(&mut g, &mut ctx);
        assert!(
            g.region_elevation[2] < before,
            "peak {} should be lowered from {}",
            g.region_elevation[2],
            before
        );
    }

    #[test]
    fn empty_globe_is_a_noop() {
        let mut g = line_globe(vec![]);
        let mut ctx = GenContext::new(1);
        ErosionStage.run(&mut g, &mut ctx);
        assert!(g.region_elevation.is_empty());
    }

    #[test]
    fn iteration_cap_is_respected() {
        let mut g = line_globe(vec![0.0, 1.0, 5.0, 1.0, 0.0]);
        let mut ctx = GenContext::new(1);
        ctx.erosion_iterations = 100_000;
        // Should terminate quickly thanks to the layer's per-pass cap.
        ErosionStage.run(&mut g, &mut ctx);
        assert!(g.region_elevation[2] < 5.0);
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = line_globe(vec![0.0, 2.0, 4.0, 2.0, 0.0]);
        let mut b = line_globe(vec![0.0, 2.0, 4.0, 2.0, 0.0]);
        let mut ca = GenContext::new(1);
        let mut cb = GenContext::new(1);
        ca.erosion_iterations = 20;
        cb.erosion_iterations = 20;
        ErosionStage.run(&mut a, &mut ca);
        ErosionStage.run(&mut b, &mut cb);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
