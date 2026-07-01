//! River hydrology stages: `river_downflow`, `river_flow`, `river_carve`.
//!
//! - `river_downflow`: each region drains to its lowest neighbour (its receiver);
//!   regions with no lower neighbour are local sinks/ocean. Initialises
//!   `region_flow` to the per-region rainfall (1 unit each).
//! - `river_flow`: accumulate flow downstream — every region pushes its
//!   accumulated flow to its receiver in order of descending elevation, so each
//!   region ends holding the total upstream contributing area.
//! - `river_carve`: lower high-flow regions slightly (valley incision) and fill
//!   `triangle_flow` by averaging the region flow of each triangle's corners.
//!
//! All three recompute receivers from the current elevation, so they are
//! independent and deterministic and need no extra shared fields.

use crate::growth::ids::RegionId;
use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage};

/// Compute each region's receiver: index of its lowest neighbour strictly below
/// it, or itself if it is a local minimum (a sink). Deterministic: lowest
/// elevation wins, ties broken by smallest index.
fn compute_receivers(globe: &PlanetGlobe) -> Vec<u32> {
    let n = globe.region_count();
    let mut recv = vec![0u32; n];
    for (r, slot) in recv.iter_mut().enumerate() {
        let h = globe.region_elevation[r];
        let mut best = r as u32;
        let mut best_h = h;
        for &nb in globe.graph.neighbours_of(RegionId(r as u32)) {
            let nh = globe.region_elevation[nb as usize];
            if nh < best_h || (nh == best_h && nb < best) {
                best_h = nh;
                best = nb;
            }
        }
        *slot = best;
    }
    recv
}

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
        for f in globe.region_flow.iter_mut() {
            *f = 1.0;
        }
        let recv = compute_receivers(globe);
        let sinks = (0..n).filter(|&r| recv[r] as usize == r).count();
        ctx.log.push(format!("river_downflow: {} sinks", sinks));
    }
}

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
        if globe.region_flow.len() != n {
            globe.region_flow.resize(n, 1.0);
        }
        let recv = compute_receivers(globe);

        // Process highest to lowest elevation so a region's full accumulation
        // is known before it drains into its receiver.
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| {
            globe.region_elevation[b]
                .partial_cmp(&globe.region_elevation[a])
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(&b))
        });

        for &r in &order {
            let target = recv[r] as usize;
            if target != r {
                globe.region_flow[target] += globe.region_flow[r];
            }
        }

        ctx.log.push("river_flow: accumulated drainage".to_string());
    }
}

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

        // Carve proportional to log(flow); never carve below sea level.
        for r in 0..n {
            if globe.region_elevation[r] < 0.0 {
                continue;
            }
            let flow = globe.region_flow[r].max(1.0);
            let incision = CARVE_K * (1.0 + flow).ln();
            let new_e = globe.region_elevation[r] - incision;
            globe.region_elevation[r] = new_e.max(0.0);
        }

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
        assert!(g.region_flow[0] > g.region_flow[4]);
        assert!(g.region_flow[0] >= 4.0);
    }

    #[test]
    fn carve_lowers_high_flow_and_sets_triangle_flow() {
        let mut g = slope_globe();
        let mut ctx = GenContext::new(1);
        let before = g.region_elevation[1];
        run_all(&mut g, &mut ctx);
        assert!(g.region_elevation[1] <= before);
        assert!(g.region_elevation[1] >= 0.0);
        assert!(g.triangle_flow[0] > 0.0);
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
