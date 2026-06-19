//! `moisture` stage: ocean-distance BFS baseline moisture.
//! Audit: Wind/moisture reqs "Current moisture is ocean-distance BFS only".
//!
//! A multi-source BFS over the region graph seeded from every ocean region
//! (`elevation < 0`) computes graph distance to the nearest ocean. Moisture is
//! `1` at the coast and decays linearly with distance, normalised to `[0,1]`.
//! Regions on an all-land world with no ocean get a flat baseline.

use std::collections::VecDeque;

use crate::ids::RegionId;
use crate::model_planet::PlanetGlobe;
use crate::pipeline::{GenContext, Stage};

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

        let unreached = u32::MAX;
        let mut dist = vec![unreached; region_count];
        let mut queue: VecDeque<usize> = VecDeque::new();

        for r in 0..region_count {
            if globe.region_elevation[r] < 0.0 {
                dist[r] = 0;
                queue.push_back(r);
            }
        }

        let no_ocean = queue.is_empty();

        let mut max_dist = 0u32;
        while let Some(r) = queue.pop_front() {
            let d = dist[r];
            for &n in globe.graph.neighbours_of(RegionId(r as u32)) {
                let ni = n as usize;
                if dist[ni] == unreached {
                    dist[ni] = d + 1;
                    if d + 1 > max_dist {
                        max_dist = d + 1;
                    }
                    queue.push_back(ni);
                }
            }
        }

        if no_ocean {
            // No ocean anywhere: uniform dry baseline.
            for m in globe.region_moisture.iter_mut() {
                *m = 0.2;
            }
        } else {
            let denom = if max_dist == 0 { 1.0 } else { max_dist as f32 };
            for r in 0..region_count {
                let d = if dist[r] == unreached {
                    max_dist
                } else {
                    dist[r]
                };
                // Nearer ocean = wetter; clamp to [0,1].
                let m = 1.0 - (d as f32 / denom);
                globe.region_moisture[r] = m.clamp(0.0, 1.0);
            }
        }

        ctx.log
            .push(format!("moisture: ocean-distance BFS, max_dist {}", max_dist));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_planet::{Icosphere, RegionGraph};
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
            graph: RegionGraph { offsets, neighbours },
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
