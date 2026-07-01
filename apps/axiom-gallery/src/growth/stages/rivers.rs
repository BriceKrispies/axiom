//! River hydrology stages: `river_downflow`, `river_flow`, `river_carve`.
//! Audit: worldgen river_downflow/river_flow/river_carve; OW-E14 (carve default).
//!
//! Thin app-side [`Stage`] wrappers over the `axiom-hydrology` layer:
//! - `river_downflow`: seeds `region_flow` to the per-region rainfall (1 unit)
//!   and logs the sink count from the layer's receivers
//!   ([`axiom_hydrology::compute_receivers`]).
//! - `river_flow`: replaces `region_flow` with the layer's downstream flow
//!   accumulation ([`axiom_hydrology::flow_accumulation`]) — each region ends
//!   holding its total upstream contributing area.
//! - `river_carve`: lowers high-flow land regions (valley incision) and fills
//!   `triangle_flow` by averaging corner-region flow. This carve is app-specific
//!   worldgen shaping, not a generic graph algorithm, so it stays here.
//!
//! The receiver / accumulation math is deterministic and lives once in the layer.

use axiom_hydrology::{compute_receivers, flow_accumulation};
use axiom_kernel::Meters;

use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};

/// Lift the globe's `f32` elevation field into the layer's typed `Meters`.
fn elevation_meters(globe: &PlanetGlobe) -> Vec<Meters> {
    globe
        .region_elevation
        .iter()
        .map(|&e| Meters::finite_or_zero(e))
        .collect()
}

// --- river_downflow ---------------------------------------------------------

pub struct RiverDownflowStage;

impl Stage for RiverDownflowStage {
    fn id(&self) -> &'static str {
        "river_downflow"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let n = globe.region_count();
        if globe.region_flow.len() != n {
            globe.region_flow.resize(n, 0.0);
        }
        // Each region starts with one unit of rainfall.
        for f in globe.region_flow.iter_mut() {
            *f = 1.0;
        }
        // Computing receivers here validates that the drainage graph exists.
        let recv = compute_receivers(&globe.graph, &elevation_meters(globe));
        let sinks = (0..n).filter(|&r| recv[r].index() == r).count();
        ctx.log.push(format!("river_downflow: {} sinks", sinks));
    }
}

// --- river_flow -------------------------------------------------------------

pub struct RiverFlowStage;

impl Stage for RiverFlowStage {
    fn id(&self) -> &'static str {
        "river_flow"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let n = globe.region_count();
        if n == 0 {
            return;
        }
        let flow = flow_accumulation(&globe.graph, &elevation_meters(globe));
        globe.region_flow = flow.into_iter().map(|r| r.get()).collect();
        ctx.log.push("river_flow: accumulated drainage".to_string());
    }
}

// --- river_carve ------------------------------------------------------------

pub struct RiverCarveStage;

/// Elevation lowered per unit of log-flow at a region.
const CARVE_K: f32 = 0.02;

impl Stage for RiverCarveStage {
    fn id(&self) -> &'static str {
        "river_carve"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let n = globe.region_count();
        if n == 0 {
            return;
        }

        // Carve land regions proportional to log(flow): big rivers cut deeper.
        // Never carve below sea level (keep coastlines stable).
        for r in 0..n {
            if globe.region_elevation[r] < 0.0 {
                continue;
            }
            let flow = globe.region_flow[r].max(1.0);
            let incision = CARVE_K * (1.0 + flow).ln();
            let new_e = globe.region_elevation[r] - incision;
            globe.region_elevation[r] = new_e.max(0.0);
        }

        // Triangle flow = mean of corner-region flow.
        let tri_count = globe.topology.triangles.len();
        if globe.triangle_flow.len() != tri_count {
            globe.triangle_flow.resize(tri_count, 0.0);
        }
        for t in 0..tri_count {
            let [a, b, c] = globe.topology.triangles[t];
            let (a, b, c) = (a as usize, b as usize, c as usize);
            if a >= n || b >= n || c >= n {
                globe.triangle_flow[t] = 0.0;
                continue;
            }
            globe.triangle_flow[t] =
                (globe.region_flow[a] + globe.region_flow[b] + globe.region_flow[c]) / 3.0;
        }

        ctx.log
            .push("river_carve: incised valleys + triangle flow".to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::model_planet::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// Line 0-1-2-3-4 sloping down toward region 0 (ocean). Flow should
    /// accumulate at the bottom.
    fn slope_globe() -> PlanetGlobe {
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
        // Descending: region 0 lowest (ocean), 4 highest.
        g.region_elevation = vec![-1.0, 1.0, 2.0, 3.0, 4.0];
        g
    }

    fn run_all(g: &mut PlanetGlobe, ctx: &mut GenContext) {
        RiverDownflowStage.run(g, ctx);
        RiverFlowStage.run(g, ctx);
        RiverCarveStage.run(g, ctx);
    }

    #[test]
    fn flow_accumulates_downstream() {
        let mut g = slope_globe();
        let mut ctx = GenContext::new(1);
        RiverDownflowStage.run(&mut g, &mut ctx);
        RiverFlowStage.run(&mut g, &mut ctx);
        // Bottom of the slope collects flow from everything above it.
        assert!(g.region_flow[0] > g.region_flow[4]);
        // Total flow is conserved (sum of rainfall == 5 units somewhere down).
        assert!(g.region_flow[0] >= 4.0);
    }

    #[test]
    fn downflow_reports_the_single_sink() {
        let mut g = slope_globe();
        let mut ctx = GenContext::new(1);
        RiverDownflowStage.run(&mut g, &mut ctx);
        // Region 0 is the only local minimum ⇒ exactly one sink.
        assert!(ctx.log.iter().any(|l| l == "river_downflow: 1 sinks"));
    }

    #[test]
    fn river_flow_empty_globe_is_a_noop() {
        let mut g = PlanetGlobe::default();
        let mut ctx = GenContext::new(1);
        RiverFlowStage.run(&mut g, &mut ctx);
        RiverCarveStage.run(&mut g, &mut ctx);
        assert!(g.region_flow.is_empty());
    }

    #[test]
    fn carve_lowers_high_flow_and_sets_triangle_flow() {
        let mut g = slope_globe();
        let mut ctx = GenContext::new(1);
        let before = g.region_elevation[1];
        run_all(&mut g, &mut ctx);
        // Region 1 (land, high flow) is incised but stays >= 0.
        assert!(g.region_elevation[1] <= before);
        assert!(g.region_elevation[1] >= 0.0);
        assert!(g.triangle_flow[0] > 0.0);
    }

    #[test]
    fn carve_zeroes_triangle_with_out_of_range_corner() {
        let mut g = slope_globe();
        // A degenerate triangle referencing a corner past the region count.
        g.topology.triangles = vec![[0, 1, 99]];
        g.triangle_flow.resize(1, 0.0);
        let mut ctx = GenContext::new(1);
        run_all(&mut g, &mut ctx);
        assert_eq!(g.triangle_flow[0], 0.0);
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = slope_globe();
        let mut b = slope_globe();
        let mut ca = GenContext::new(1);
        let mut cb = GenContext::new(1);
        run_all(&mut a, &mut ca);
        run_all(&mut b, &mut cb);
        assert_eq!(a.region_flow, b.region_flow);
        assert_eq!(a.region_elevation, b.region_elevation);
        assert_eq!(a.triangle_flow, b.triangle_flow);
    }
}
