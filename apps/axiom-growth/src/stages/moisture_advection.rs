//! `moisture_advection` stage (OW-E8): advect moisture downwind along the region
//! graph; ocean stays a moisture source; values stay in `[0,1]`.
//! Audit: OW-E8 wind-advected moisture, deterministic, runs after elevation.
//!
//! For a fixed number of passes, each land region pulls moisture from its most
//! *upwind* neighbour — the neighbour whose offset direction best opposes the
//! region's wind (i.e. the wind blows from that neighbour toward this region).
//! Ocean regions are pinned wet (= 1) so they act as an infinite source.

use crate::ids::RegionId;
use crate::model_planet::PlanetGlobe;
use crate::pipeline::{GenContext, Stage};

/// Advection passes (deterministic, fixed). More passes carry moisture further.
const PASSES: u32 = 6;
/// How strongly upwind moisture is blended in each pass.
const ADVECT: f32 = 0.5;

pub struct MoistureAdvectionStage;

impl Stage for MoistureAdvectionStage {
    fn id(&self) -> &'static str {
        "moisture_advection"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let region_count = globe.region_count();
        if region_count == 0 {
            return;
        }
        if globe.region_moisture.len() != region_count {
            globe.region_moisture.resize(region_count, 0.0);
        }

        // Pin oceans wet up front so they source moisture every pass.
        for r in 0..region_count {
            if globe.region_elevation[r] < 0.0 {
                globe.region_moisture[r] = 1.0;
            }
        }

        let mut next = globe.region_moisture.clone();
        for _ in 0..PASSES {
            for r in 0..region_count {
                if globe.region_elevation[r] < 0.0 {
                    next[r] = 1.0; // ocean source
                    continue;
                }
                let site = globe.topology.sites[r];
                let wind = globe.region_wind[r];
                let neighbours = globe.graph.neighbours_of(RegionId(r as u32));
                if neighbours.is_empty() {
                    next[r] = globe.region_moisture[r];
                    continue;
                }
                // Most-upwind neighbour: the one the wind blows from. The wind
                // blows toward `wind`; upwind direction is `-wind`. Pick the
                // neighbour whose direction (from region) best matches -wind.
                let mut best: Option<usize> = None;
                let mut best_score = 0.0f32;
                for &n in neighbours {
                    let dir = globe.topology.sites[n as usize].subtract(site);
                    let dir = dir.normalize().unwrap_or(dir);
                    let score = dir.dot(wind.mul_scalar(-1.0));
                    if score > best_score {
                        best_score = score;
                        best = Some(n as usize);
                    }
                }
                if let Some(up) = best {
                    let upwind_m = globe.region_moisture[up];
                    let here = globe.region_moisture[r];
                    next[r] = (here + ADVECT * (upwind_m - here)).clamp(0.0, 1.0);
                } else {
                    next[r] = globe.region_moisture[r];
                }
            }
            globe.region_moisture.copy_from_slice(&next);
        }

        for m in globe.region_moisture.iter_mut() {
            *m = m.clamp(0.0, 1.0);
        }

        ctx.log
            .push(format!("moisture_advection: {} passes", PASSES));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_planet::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// Line of regions along +x; region 0 ocean. Wind blows +x (toward higher
    /// index) so moisture should carry inland.
    fn line_globe(n: usize) -> PlanetGlobe {
        let sites: Vec<Vec3> = (0..n)
            .map(|i| Vec3::new(i as f32 + 1.0, 0.0, 0.0))
            .collect();
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
        g.region_elevation = (0..n).map(|i| if i == 0 { -1.0 } else { 0.5 }).collect();
        // Wind blows toward +x for all regions.
        g.region_wind = vec![Vec3::new(1.0, 0.0, 0.0); n];
        // Dry land, wet ocean to start.
        g.region_moisture = (0..n).map(|i| if i == 0 { 1.0 } else { 0.0 }).collect();
        g
    }

    #[test]
    fn moisture_stays_in_range_and_carries_inland() {
        let mut g = line_globe(5);
        let mut ctx = GenContext::new(1);
        MoistureAdvectionStage.run(&mut g, &mut ctx);
        for &m in &g.region_moisture {
            assert!((0.0..=1.0).contains(&m), "moisture {} out of range", m);
        }
        // Region 1 (next to ocean, downwind) should have gained moisture.
        assert!(g.region_moisture[1] > 0.0);
        // Closer to source should be at least as wet as further inland.
        assert!(g.region_moisture[1] >= g.region_moisture[4]);
    }

    #[test]
    fn ocean_stays_source() {
        let mut g = line_globe(5);
        let mut ctx = GenContext::new(1);
        MoistureAdvectionStage.run(&mut g, &mut ctx);
        assert_eq!(g.region_moisture[0], 1.0);
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = line_globe(6);
        let mut b = line_globe(6);
        let mut ca = GenContext::new(1);
        let mut cb = GenContext::new(1);
        MoistureAdvectionStage.run(&mut a, &mut ca);
        MoistureAdvectionStage.run(&mut b, &mut cb);
        assert_eq!(a.region_moisture, b.region_moisture);
    }
}
