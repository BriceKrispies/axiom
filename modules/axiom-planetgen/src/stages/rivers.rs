//! River hydrology stages: `river_downflow`, `river_flow`, `river_carve`.
//!
//! - `river_downflow` seeds `region_flow` to per-region rainfall (1 unit each).
//! - `river_flow` replaces it with the `hydrology` layer's downstream flow
//!   accumulation ([`axiom_hydrology::flow_accumulation`]) — each region ends
//!   holding its total upstream contributing area.
//! - `river_carve` lowers high-flow land regions (valley incision, never below
//!   sea level) and fills `triangle_flow` from corner-region flow. Branchless.

use axiom_hydrology::flow_accumulation;
use axiom_kernel::Meters;

use crate::globe::{corner_mean, PlanetGlobe};

/// Elevation lowered per unit of log-flow at a region.
const CARVE_K: f32 = 0.02;

/// Lift the globe's `f32` elevation field into the layer's typed `Meters`.
fn elevation_meters(globe: &PlanetGlobe) -> Vec<Meters> {
    globe
        .region_elevation
        .iter()
        .map(|&e| Meters::finite_or_zero(e))
        .collect()
}

pub(crate) fn river_downflow(globe: &mut PlanetGlobe) {
    let n = globe.region_count();
    globe.region_flow = vec![1.0; n];
}

pub(crate) fn river_flow(globe: &mut PlanetGlobe) {
    let flow = flow_accumulation(&globe.graph, &elevation_meters(globe));
    globe.region_flow = flow.into_iter().map(|r| r.get()).collect();
}

/// Incised elevation of region `r`: land regions are lowered by `CARVE_K *
/// ln(1 + flow)` (never below sea level 0); ocean regions are left unchanged.
fn carve_region(globe: &PlanetGlobe, r: usize) -> f32 {
    let e = globe.region_elevation[r];
    let land = e >= 0.0;
    let flow = globe.region_flow[r].max(1.0);
    let incision = CARVE_K * (1.0 + flow).ln();
    let carved = (e - incision).max(0.0);
    [e, carved][usize::from(land)]
}

pub(crate) fn river_carve(globe: &mut PlanetGlobe) {
    let n = globe.region_count();
    let region_elevation: Vec<f32> = (0..n).map(|r| carve_region(globe, r)).collect();
    globe.region_elevation = region_elevation;

    let triangle_flow: Vec<f32> = globe
        .topology
        .triangles
        .iter()
        .map(|&tri| corner_mean(&globe.region_flow, tri))
        .collect();
    globe.triangle_flow = triangle_flow;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_geosphere::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// Line 0-1-2-3-4 sloping down toward region 0 (ocean); flow accumulates at
    /// the bottom.
    fn slope_globe() -> PlanetGlobe {
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
                triangles: vec![[0, 1, 2]],
                subdivisions: 0,
            },
            graph: RegionGraph {
                offsets,
                neighbours,
            },
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        g.region_elevation = vec![-1.0, 1.0, 2.0, 3.0, 4.0];
        g
    }

    fn run_all(g: &mut PlanetGlobe) {
        river_downflow(g);
        river_flow(g);
        river_carve(g);
    }

    #[test]
    fn downflow_seeds_unit_rainfall() {
        let mut g = slope_globe();
        river_downflow(&mut g);
        assert_eq!(g.region_flow, vec![1.0; 5]);
    }

    #[test]
    fn flow_accumulates_downstream() {
        let mut g = slope_globe();
        river_downflow(&mut g);
        river_flow(&mut g);
        assert!(g.region_flow[0] > g.region_flow[4]);
        assert!(g.region_flow[0] >= 4.0);
    }

    #[test]
    fn river_flow_empty_globe_is_a_noop() {
        let mut g = PlanetGlobe::default();
        river_flow(&mut g);
        river_carve(&mut g);
        assert!(g.region_flow.is_empty());
    }

    #[test]
    fn carve_lowers_high_flow_and_sets_triangle_flow() {
        let mut g = slope_globe();
        let before = g.region_elevation[1];
        run_all(&mut g);
        // Region 1 (land, high flow) is incised but stays >= 0.
        assert!(g.region_elevation[1] <= before);
        assert!(g.region_elevation[1] >= 0.0);
        assert!(g.triangle_flow[0] > 0.0);
    }

    #[test]
    fn carve_leaves_ocean_region_unchanged() {
        let mut g = slope_globe();
        river_downflow(&mut g);
        river_flow(&mut g);
        let ocean_before = g.region_elevation[0];
        river_carve(&mut g);
        assert_eq!(g.region_elevation[0], ocean_before);
    }

    #[test]
    fn deterministic_same_input() {
        let mut a = slope_globe();
        let mut b = slope_globe();
        run_all(&mut a);
        run_all(&mut b);
        assert_eq!(a.region_flow, b.region_flow);
        assert_eq!(a.region_elevation, b.region_elevation);
        assert_eq!(a.triangle_flow, b.triangle_flow);
    }
}
