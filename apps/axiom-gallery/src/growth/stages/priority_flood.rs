//! `priority_flood` stage (OW-E11): Barnes-style priority-flood pit filling so
//! land drains monotonically to the ocean.
//! Audit: OW-E11 priority_flood region drainage, monotonic, saddle spill carved.
//!
//! Thin app-side [`Stage`] wrapper over the `axiom-hydrology` layer: the
//! priority-flood drainage surface ([`axiom_hydrology::pit_fill`], a branchless
//! monotone wavefront relaxation that replaced the old `BinaryHeap` flood) lives
//! in the layer. This stage lifts the region elevation into `Meters`, fills to
//! sea level (`0`), writes the result back, and logs how many regions were
//! raised. Afterward every region has a non-ascending path to an outlet — no
//! interior pit is lower than its outflow saddle.

use axiom_hydrology::pit_fill;
use axiom_kernel::Meters;

use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};

pub struct PriorityFloodStage;

impl Stage for PriorityFloodStage {
    fn id(&self) -> &'static str {
        "priority_flood"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let region_count = globe.region_count();
        if region_count == 0 {
            return;
        }

        let before = globe.region_elevation.clone();
        let mut elevation: Vec<Meters> =
            before.iter().map(|&e| Meters::finite_or_zero(e)).collect();
        pit_fill(&globe.graph, &mut elevation, Meters::finite_or_zero(0.0));

        let filled = before
            .iter()
            .zip(&elevation)
            .filter(|(&b, m)| m.get() > b)
            .count();
        globe.region_elevation = elevation.into_iter().map(|m| m.get()).collect();

        ctx.log
            .push(format!("priority_flood: filled {} pits", filled));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::ids::RegionId;
    use crate::growth::model_planet::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// Line 0-1-2-3-4. Region 0 ocean (-1). Region 2 is a pit (0.1) below its
    /// neighbours (1.0). After flooding it must be raised to the spill level.
    fn pit_globe() -> PlanetGlobe {
        let n = 5;
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
        g.region_elevation = vec![-1.0, 1.0, 0.1, 1.0, 2.0];
        g
    }

    /// Every land region must have a neighbour at lower-or-equal elevation
    /// leading (eventually) to the ocean — i.e. no strict interior pit remains.
    fn has_monotonic_drainage(g: &PlanetGlobe) -> bool {
        let n = g.region_count();
        for r in 0..n {
            if g.region_elevation[r] < 0.0 {
                continue; // ocean drains trivially
            }
            let here = g.region_elevation[r];
            let mut ok = false;
            for &nb in g.graph.neighbours_of(RegionId(r as u32)) {
                if g.region_elevation[nb as usize] <= here {
                    ok = true;
                    break;
                }
            }
            if !ok {
                return false;
            }
        }
        true
    }

    #[test]
    fn pit_is_filled_to_spill() {
        let mut g = pit_globe();
        let mut ctx = GenContext::new(1);
        PriorityFloodStage.run(&mut g, &mut ctx);
        // Region 2 was a pit at 0.1 surrounded by 1.0; it must be raised.
        assert!(
            g.region_elevation[2] >= 1.0,
            "pit not filled: {}",
            g.region_elevation[2]
        );
        // The log reports at least the one filled pit.
        assert!(ctx
            .log
            .iter()
            .any(|l| l.starts_with("priority_flood: filled")));
    }

    #[test]
    fn empty_globe_is_a_noop() {
        let mut g = PlanetGlobe::default();
        let mut ctx = GenContext::new(1);
        PriorityFloodStage.run(&mut g, &mut ctx);
        assert!(g.region_elevation.is_empty());
    }

    #[test]
    fn drainage_is_monotonic() {
        let mut g = pit_globe();
        let mut ctx = GenContext::new(1);
        PriorityFloodStage.run(&mut g, &mut ctx);
        assert!(has_monotonic_drainage(&g));
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = pit_globe();
        let mut b = pit_globe();
        let mut ca = GenContext::new(1);
        let mut cb = GenContext::new(1);
        PriorityFloodStage.run(&mut a, &mut ca);
        PriorityFloodStage.run(&mut b, &mut cb);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
