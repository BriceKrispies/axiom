//! `erosion` stage: iterative stream-power-style erosion on the region graph.
//! Audit: OW-E16 stream-power erosion (done core).
//!
//! For `min(ctx.erosion_iterations, MAX_ITERS)` passes, each region is lowered
//! toward its lowest downhill neighbour by a fraction proportional to the local
//! slope — a cheap, deterministic stand-in for stream-power incision that
//! smooths peaks and deepens valleys without depending on flow accumulation yet.

use crate::growth::ids::RegionId;
use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};

/// Cap on erosion passes regardless of the requested count. Audit: perf cap.
const MAX_ITERS: u32 = 60;
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
        let iters = ctx.erosion_iterations.min(MAX_ITERS);

        let mut next = globe.region_elevation.clone();
        for _ in 0..iters {
            for (r, slot) in next.iter_mut().enumerate() {
                let h = globe.region_elevation[r];
                let neighbours = globe.graph.neighbours_of(RegionId(r as u32));
                if neighbours.is_empty() {
                    *slot = h;
                    continue;
                }
                // Lowest downhill neighbour.
                let mut min_h = h;
                for &n in neighbours {
                    let nh = globe.region_elevation[n as usize];
                    if nh < min_h {
                        min_h = nh;
                    }
                }
                let slope = h - min_h;
                // Incise proportional to slope; only erodes where there is a drop.
                *slot = if slope > 0.0 {
                    h - EROSION_K * slope
                } else {
                    h
                };
            }
            globe.region_elevation.copy_from_slice(&next);
        }

        ctx.log
            .push(format!("erosion: {} passes (cap {})", iters, MAX_ITERS));
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
    fn iteration_cap_is_respected() {
        let mut g = line_globe(vec![0.0, 1.0, 5.0, 1.0, 0.0]);
        let mut ctx = GenContext::new(1);
        ctx.erosion_iterations = 100_000;
        // Should terminate quickly thanks to MAX_ITERS cap.
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
