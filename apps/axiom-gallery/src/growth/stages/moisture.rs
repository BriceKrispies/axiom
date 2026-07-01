//! `moisture` stage: ocean-distance BFS baseline moisture.
//! Audit: Wind/moisture reqs "Current moisture is ocean-distance BFS only".
//!
//! Thin app-side [`Stage`] wrapper over the `axiom-hydrology` layer: the
//! multi-source ocean-distance solver ([`axiom_hydrology::ocean_distance`]) does
//! the graph traversal; this stage marks ocean regions (`elevation < 0`), calls
//! the layer, and folds the returned hop distances into a `[0,1]` moisture field
//! (`1` at the coast, decaying linearly with distance). An all-land world with no
//! ocean gets a flat dry baseline.

use axiom_hydrology::ocean_distance;

use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};

pub struct MoistureStage;

impl Stage for MoistureStage {
    fn id(&self) -> &'static str {
        "moisture"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let region_count = globe.region_count();
        if globe.region_moisture.len() != region_count {
            globe.region_moisture.resize(region_count, 0.0);
        }
        if region_count == 0 {
            return;
        }

        // Ocean regions (below sea level) are the BFS sources.
        let is_ocean: Vec<bool> = globe.region_elevation.iter().map(|&e| e < 0.0).collect();
        let no_ocean = !is_ocean.iter().any(|&o| o);

        let dist = ocean_distance(&globe.graph, &is_ocean);
        let max_dist = dist.iter().filter_map(|d| d.steps()).max().unwrap_or(0);

        if no_ocean {
            // No ocean anywhere: uniform dry baseline.
            for m in globe.region_moisture.iter_mut() {
                *m = 0.2;
            }
        } else {
            let denom = if max_dist == 0 { 1.0 } else { max_dist as f32 };
            for (r, hop) in dist.iter().enumerate() {
                // Unreached interior sits at the far end of the gradient.
                let d = hop.steps().unwrap_or(max_dist);
                // Nearer ocean = wetter; clamp to [0,1].
                let m = 1.0 - (d as f32 / denom);
                globe.region_moisture[r] = m.clamp(0.0, 1.0);
            }
        }

        ctx.log.push(format!(
            "moisture: ocean-distance BFS, max_dist {}",
            max_dist
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::model_planet::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// Line 0-1-2-3-4; region 0 is ocean.
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
    fn moisture_in_unit_range_and_decays() {
        let mut g = line_globe(vec![-1.0, 0.5, 0.5, 0.5, 0.5]);
        let mut ctx = GenContext::new(1);
        MoistureStage.run(&mut g, &mut ctx);
        for &m in &g.region_moisture {
            assert!((0.0..=1.0).contains(&m), "moisture {} out of range", m);
        }
        // Coast wetter than interior.
        assert!(g.region_moisture[0] >= g.region_moisture[4]);
        assert!(g.region_moisture[1] > g.region_moisture[3]);
    }

    #[test]
    fn no_ocean_gets_baseline() {
        let mut g = line_globe(vec![1.0, 1.0, 1.0]);
        let mut ctx = GenContext::new(1);
        MoistureStage.run(&mut g, &mut ctx);
        for &m in &g.region_moisture {
            assert!((0.0..=1.0).contains(&m));
        }
    }

    #[test]
    fn empty_globe_is_a_noop() {
        let mut g = line_globe(vec![]);
        let mut ctx = GenContext::new(1);
        MoistureStage.run(&mut g, &mut ctx);
        assert!(g.region_moisture.is_empty());
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = line_globe(vec![-1.0, 0.5, 0.5, 0.5, 0.5]);
        let mut b = line_globe(vec![-1.0, 0.5, 0.5, 0.5, 0.5]);
        let mut ca = GenContext::new(1);
        let mut cb = GenContext::new(1);
        MoistureStage.run(&mut a, &mut ca);
        MoistureStage.run(&mut b, &mut cb);
        assert_eq!(a.region_moisture, b.region_moisture);
    }
}
