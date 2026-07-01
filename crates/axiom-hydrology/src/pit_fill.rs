//! Priority-flood pit filling as a monotone wavefront relaxation.
//!
//! The classic Barnes / Planchon-Darboux priority flood uses a `BinaryHeap`
//! ordered by spill level and a `while let Some(pop)` drain — inherently branchy.
//! The same fixpoint is reachable **branchlessly** as a monotone relaxation:
//!
//! * **Outlets** — regions that drain out of the domain — are pinned at their own
//!   elevation. An outlet is any region below `sea_level` (ocean), plus the single
//!   globally-lowest region, so there is always at least one outlet even on an
//!   ocean-free world (an endorheic basin draining to its global minimum). When
//!   ocean exists the global minimum is already one of them, so the union is a
//!   no-op there.
//! * Every non-outlet region starts at `+∞` and each pass takes
//!   `filled[r] = max(elevation[r], least filled neighbour)`.
//!
//! This converges (in ≤ `region_count` passes, bounded by graph diameter) to the
//! priority-flood surface: each region's filled height is the minimum over all
//! paths to an outlet of the maximum elevation along the path — i.e. it is raised
//! exactly to its lowest spill saddle, and no lower. Afterward every region has a
//! non-ascending path to an outlet: no interior pit survives. Deterministic —
//! ordered folds, a fixed pass count, `(elevation, index)` tie-breaks.

use axiom_geosphere::{RegionGraph, RegionId};
use axiom_kernel::Meters;

/// Raise `elevation` in place to a monotone drainage surface: fill every interior
/// pit up to its lowest outflow saddle so the field drains to an outlet.
///
/// Outlets are regions strictly below `sea_level`, together with the single
/// globally-lowest region (the guaranteed outlet when no region is below sea
/// level). Outlets keep their elevation; interior regions are only ever raised,
/// never lowered. A region with no path to any outlet (a disconnected component)
/// keeps its original elevation.
pub fn pit_fill(graph: &RegionGraph, elevation: &mut [Meters], sea_level: Meters) {
    let n = elevation.len();
    let sea = sea_level.get();
    let argmin = global_min_region(elevation);
    // Outlet = below sea level, OR the guaranteed global-minimum outlet.
    let outlet: Vec<bool> = (0..n)
        .map(|r| (elevation[r].get() < sea) | (r as u32 == argmin))
        .collect();
    // Working buffer: outlets pinned at their elevation, interior at +∞.
    let mut filled: Vec<f32> = (0..n)
        .map(|r| {
            let e = elevation[r].get();
            outlet[r].then_some(e).unwrap_or(f32::INFINITY)
        })
        .collect();
    // region_count passes ≥ graph diameter ⇒ fully converged.
    (0..n).for_each(|_| relax_pass(graph, elevation, &outlet, &mut filled));
    // Write back: a converged interior is finite; a disconnected region (stays
    // +∞) keeps its original elevation. Meters::finite_or_zero is the total
    // constructor — its input is already finite here.
    (0..n).for_each(|r| {
        let e = elevation[r].get();
        let f = filled[r];
        let out = f.is_finite().then_some(f).unwrap_or(e);
        elevation[r] = Meters::finite_or_zero(out);
    });
}

/// Index of the `(elevation, index)`-least region, or `0` for an empty field
/// (unused — an empty field runs zero passes). Deterministic argmin: lowest
/// elevation wins, ties break to the smallest index.
fn global_min_region(elevation: &[Meters]) -> u32 {
    let n = elevation.len();
    (0..n)
        .fold((f32::INFINITY, 0u32), |(best_v, best_i), r| {
            let v = elevation[r].get();
            ((v, r as u32) < (best_v, best_i))
                .then_some((v, r as u32))
                .unwrap_or((best_v, best_i))
        })
        .1
}

/// One relaxation pass: each non-outlet region takes `max(own elevation, least
/// filled neighbour)`; each outlet is pinned at its own elevation. `elevation`
/// stays the untouched original throughout relaxation (only `filled` mutates), so
/// `max` reads the true floor. An empty / out-of-range neighbourhood folds to
/// `+∞`, which cannot lower `max`.
fn relax_pass(graph: &RegionGraph, elevation: &[Meters], outlet: &[bool], filled: &mut [f32]) {
    let n = filled.len();
    (0..n).for_each(|r| {
        let e = elevation[r].get();
        let least_neighbour = graph
            .neighbours_of(RegionId(r as u32))
            .iter()
            .fold(f32::INFINITY, |acc, &nb| {
                acc.min(filled.get(nb as usize).copied().unwrap_or(f32::INFINITY))
            });
        let relaxed = e.max(least_neighbour);
        filled[r] = outlet[r].then_some(e).unwrap_or(relaxed);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_graphs::{line_graph, meters};

    /// Every land region must have a neighbour at lower-or-equal filled elevation
    /// (a non-ascending step toward an outlet) — i.e. no strict interior pit.
    fn drains_monotonically(graph: &RegionGraph, elevation: &[Meters], sea: f32) -> bool {
        let n = elevation.len();
        (0..n).all(|r| {
            let here = elevation[r].get();
            let is_ocean = here < sea;
            let has_lower = graph
                .neighbours_of(RegionId(r as u32))
                .iter()
                .any(|&nb| elevation[nb as usize].get() <= here);
            is_ocean | has_lower
        })
    }

    #[test]
    fn interior_pit_is_filled_to_spill() {
        // Line 0-1-2-3-4. Region 0 ocean (-1). Region 2 is a pit (0.1) below its
        // neighbours (1.0); it must be raised to at least the spill level (1.0).
        let graph = line_graph(5);
        let mut elev = meters(&[-1.0, 1.0, 0.1, 1.0, 2.0]);
        pit_fill(&graph, &mut elev, Meters::finite_or_zero(0.0));
        assert!(elev[2].get() >= 1.0);
    }

    #[test]
    fn filled_surface_drains_monotonically() {
        let graph = line_graph(5);
        let mut elev = meters(&[-1.0, 1.0, 0.1, 1.0, 2.0]);
        pit_fill(&graph, &mut elev, Meters::finite_or_zero(0.0));
        assert!(drains_monotonically(&graph, &elev, 0.0));
    }

    #[test]
    fn outlets_and_already_draining_land_are_untouched() {
        // A clean descending slope has no pit: nothing should be raised.
        let graph = line_graph(5);
        let before = [-1.0f32, 1.0, 2.0, 3.0, 4.0];
        let mut elev = meters(&before);
        pit_fill(&graph, &mut elev, Meters::finite_or_zero(0.0));
        before
            .iter()
            .zip(&elev)
            .for_each(|(&b, m)| assert_eq!(m.get(), b));
    }

    #[test]
    fn idempotent_second_fill_is_a_fixpoint() {
        let graph = line_graph(5);
        let mut elev = meters(&[-1.0, 1.0, 0.1, 1.0, 2.0]);
        pit_fill(&graph, &mut elev, Meters::finite_or_zero(0.0));
        let once: Vec<f32> = elev.iter().map(|m| m.get()).collect();
        pit_fill(&graph, &mut elev, Meters::finite_or_zero(0.0));
        let twice: Vec<f32> = elev.iter().map(|m| m.get()).collect();
        assert_eq!(once, twice);
    }

    #[test]
    fn no_ocean_uses_global_minimum_as_outlet() {
        // All land, no region below sea level: region 1 is the global min and
        // the interior pit (region 3 at 0.5 between 3.0 neighbours) still fills.
        let graph = line_graph(5);
        let mut elev = meters(&[3.0, 0.2, 3.0, 0.5, 3.0]);
        pit_fill(&graph, &mut elev, Meters::finite_or_zero(0.0));
        // The pit at region 3 is raised toward its spill (3.0).
        assert!(elev[3].get() >= 3.0);
        // The global-minimum outlet keeps its own elevation.
        assert_eq!(elev[1].get(), 0.2);
    }

    #[test]
    fn disconnected_region_keeps_original_elevation() {
        // Region 2 is disconnected (no neighbours), never reaching an outlet;
        // it keeps its original elevation, exercising the +∞ write-back arm.
        let graph = RegionGraph {
            offsets: vec![0, 1, 2, 2],
            neighbours: vec![1, 0],
        };
        let mut elev = meters(&[-1.0, 2.0, 7.5]);
        pit_fill(&graph, &mut elev, Meters::finite_or_zero(0.0));
        assert_eq!(elev[2].get(), 7.5);
    }

    #[test]
    fn empty_field_is_a_noop() {
        let mut elev: Vec<Meters> = Vec::new();
        pit_fill(
            &RegionGraph::default(),
            &mut elev,
            Meters::finite_or_zero(0.0),
        );
        assert!(elev.is_empty());
    }
}
