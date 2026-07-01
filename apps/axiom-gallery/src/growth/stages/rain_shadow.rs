//! `rain_shadow` stage: orographic lift windward, depletion leeward.
//!
//! For each land region we look at the *upwind* neighbour (the one the wind
//! blows from). If this region rises above that neighbour, the air is forced up
//! and rains out: the windward side gains moisture while the leeward side
//! (falling away downwind) loses it. Moisture stays in `[0,1]`.

use crate::growth::ids::RegionId;
use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};

/// Moisture gained per unit of windward elevation rise.
const WINDWARD_GAIN: f32 = 0.6;
/// Moisture lost per unit of leeward elevation drop.
const LEEWARD_LOSS: f32 = 0.6;

pub struct RainShadowStage;

impl Stage for RainShadowStage {
    fn id(&self) -> &'static str {
        "rain_shadow"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let region_count = globe.region_count();
        if region_count == 0 {
            return;
        }

        let mut delta = vec![0.0f32; region_count];
        for r in 0..region_count {
            if globe.region_elevation[r] < 0.0 {
                continue;
            }
            let site = globe.topology.sites[r];
            let wind = globe.region_wind[r];
            let neighbours = globe.graph.neighbours_of(RegionId(r as u32));
            if neighbours.is_empty() {
                continue;
            }

            // Most-upwind neighbour: wind blows from it toward us.
            let mut up: Option<usize> = None;
            let mut up_score = 0.0f32;
            // Most-downwind neighbour: wind blows toward it.
            let mut down: Option<usize> = None;
            let mut down_score = 0.0f32;
            for &n in neighbours {
                let dir = globe.topology.sites[n as usize].subtract(site);
                let dir = dir.normalize().unwrap_or(dir);
                let upwind = dir.dot(wind.mul_scalar(-1.0));
                if upwind > up_score {
                    up_score = upwind;
                    up = Some(n as usize);
                }
                let downwind = dir.dot(wind);
                if downwind > down_score {
                    down_score = downwind;
                    down = Some(n as usize);
                }
            }

            let h = globe.region_elevation[r];
            if let Some(u) = up {
                let rise = h - globe.region_elevation[u];
                if rise > 0.0 {
                    delta[r] += WINDWARD_GAIN * rise;
                }
            }
            if let Some(d) = down {
                let drop = h - globe.region_elevation[d];
                if drop > 0.0 {
                    delta[d] -= LEEWARD_LOSS * drop;
                }
            }
        }

        for (r, &dr) in delta.iter().enumerate() {
            if globe.region_elevation[r] < 0.0 {
                continue;
            }
            let m = globe.region_moisture[r] + dr;
            globe.region_moisture[r] = m.clamp(0.0, 1.0);
        }

        ctx.log
            .push("rain_shadow: orographic windward gain / leeward loss".to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::model_planet::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// Line of 4 regions along +x. Wind blows +x. Region 1 is a tall ridge so 1
    /// is windward-rising (wet) and region 2 behind it is in shadow (dry).
    fn ridge_globe() -> PlanetGlobe {
        let sites: Vec<Vec3> = (0..4)
            .map(|i| Vec3::new(i as f32 + 1.0, 0.0, 0.0))
            .collect();
        let offsets = vec![0u32, 1, 3, 5, 6];
        let neighbours = vec![1, 0, 2, 1, 3, 2];
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
        g.region_elevation = vec![0.1, 2.0, 0.1, 0.1];
        g.region_wind = vec![Vec3::new(1.0, 0.0, 0.0); 4];
        g.region_moisture = vec![0.5, 0.5, 0.5, 0.5];
        g
    }

    #[test]
    fn windward_wetter_leeward_drier() {
        let mut g = ridge_globe();
        let before = g.region_moisture.clone();
        let mut ctx = GenContext::new(1);
        RainShadowStage.run(&mut g, &mut ctx);
        for &m in &g.region_moisture {
            assert!((0.0..=1.0).contains(&m), "moisture {} out of range", m);
        }
        assert!(g.region_moisture[1] >= before[1]);
        assert!(g.region_moisture[2] < before[2]);
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = ridge_globe();
        let mut b = ridge_globe();
        let mut ca = GenContext::new(1);
        let mut cb = GenContext::new(1);
        RainShadowStage.run(&mut a, &mut ca);
        RainShadowStage.run(&mut b, &mut cb);
        assert_eq!(a.region_moisture, b.region_moisture);
    }
}
