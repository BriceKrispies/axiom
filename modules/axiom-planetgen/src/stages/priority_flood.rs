//! `priority_flood`: Barnes-style priority-flood pit filling so land drains
//! monotonically to the ocean.
//!
//! Thin wrapper over the `hydrology` layer: it lifts the region elevation into
//! `Meters`, fills to sea level (0) via [`axiom_hydrology::pit_fill`] (a
//! branchless monotone wavefront relaxation), and writes the result back. After
//! it, every region has a non-ascending path to an outlet.

use axiom_hydrology::pit_fill;
use axiom_kernel::Meters;

use crate::globe::PlanetGlobe;

pub(crate) fn priority_flood(globe: &mut PlanetGlobe) {
    let mut elevation: Vec<Meters> = globe
        .region_elevation
        .iter()
        .map(|&e| Meters::finite_or_zero(e))
        .collect();
    pit_fill(&globe.graph, &mut elevation, Meters::finite_or_zero(0.0));
    globe.region_elevation = elevation.into_iter().map(|m| m.get()).collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_geosphere::{Icosphere, RegionGraph, RegionId};
    use axiom_math::Vec3;

    /// Line 0-1-2-3-4. Region 0 ocean (-1); region 2 is a pit (0.1) below its
    /// neighbours (1.0). After flooding it must rise to the spill level.
    fn pit_globe() -> PlanetGlobe {
        let n = 5;
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
        g.region_elevation = vec![-1.0, 1.0, 0.1, 1.0, 2.0];
        g
    }

    /// Every land region has a neighbour at lower-or-equal elevation — no strict
    /// interior pit remains.
    fn has_monotonic_drainage(g: &PlanetGlobe) -> bool {
        (0..g.region_count()).all(|r| {
            g.region_elevation[r] < 0.0
                || g.graph
                    .neighbours_of(RegionId(r as u32))
                    .iter()
                    .any(|&nb| g.region_elevation[nb as usize] <= g.region_elevation[r])
        })
    }

    #[test]
    fn pit_is_filled_to_spill() {
        let mut g = pit_globe();
        priority_flood(&mut g);
        assert!(g.region_elevation[2] >= 1.0, "pit not filled");
    }

    #[test]
    fn empty_globe_is_a_noop() {
        let mut g = PlanetGlobe::default();
        priority_flood(&mut g);
        assert!(g.region_elevation.is_empty());
    }

    #[test]
    fn drainage_is_monotonic() {
        let mut g = pit_globe();
        priority_flood(&mut g);
        assert!(has_monotonic_drainage(&g));
    }

    #[test]
    fn deterministic_same_input() {
        let mut a = pit_globe();
        let mut b = pit_globe();
        priority_flood(&mut a);
        priority_flood(&mut b);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
