//! Iterative stream-power-style erosion over the region graph.
//!
//! For a bounded number of passes, each region is lowered toward its lowest
//! neighbour by a fraction (`strength`) of the local downhill slope — a cheap,
//! deterministic stand-in for stream-power incision that smooths peaks and
//! deepens valleys. Each pass reads a frozen snapshot of the previous pass so the
//! update is order-independent and byte-deterministic.
//!
//! Branchless by construction: the per-region slope folds the neighbourhood to
//! its least filled height *seeded with the region's own height*, so a region
//! with no lower neighbour (or no neighbour at all) yields slope `0` and is left
//! unchanged — the old `if neighbours.is_empty()` and `if slope > 0` arms both
//! collapse into `h - strength · slope` with `slope ≥ 0`.

use axiom_geosphere::{RegionGraph, RegionId};
use axiom_kernel::{Meters, Ratio};

/// Hard cap on erosion passes regardless of the requested count, matching the
/// app's original perf cap.
const MAX_ITERS: u32 = 60;

/// Erode `elevation` for `min(iterations, MAX_ITERS)` passes at the given
/// `strength` (fraction of local slope incised per pass), returning the new
/// elevation field. Pure: the input is not mutated.
pub fn stream_power_erosion(
    graph: &RegionGraph,
    elevation: &[Meters],
    strength: Ratio,
    iterations: u32,
) -> Vec<Meters> {
    let iters = iterations.min(MAX_ITERS);
    let k = strength.get();
    let start: Vec<f32> = elevation.iter().map(|m| m.get()).collect();
    let eroded = (0..iters).fold(start, |current, _| erode_pass(graph, &current, k));
    eroded.into_iter().map(Meters::finite_or_zero).collect()
}

/// One erosion pass over a frozen `current` snapshot: each region drops by
/// `k · (h − least neighbour height)`, clamped so an uphill/flat region (slope
/// `0`) is unchanged. The fold seeds with `h`, so the least is `≤ h` and the
/// slope is always `≥ 0`.
fn erode_pass(graph: &RegionGraph, current: &[f32], k: f32) -> Vec<f32> {
    let n = current.len();
    (0..n)
        .map(|r| {
            let h = current[r];
            let least_neighbour = graph
                .neighbours_of(RegionId(r as u32))
                .iter()
                .fold(h, |acc, &nb| {
                    acc.min(*current.get(nb as usize).unwrap_or(&h))
                });
            let slope = h - least_neighbour;
            h - k * slope
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_graphs::{line_graph, meters};

    fn ratio(v: f32) -> Ratio {
        Ratio::finite_or_zero(v)
    }

    #[test]
    fn peak_is_lowered() {
        // A central peak 0-1-5-1-0 is incised over several passes.
        let graph = line_graph(5);
        let elev = meters(&[0.0, 1.0, 5.0, 1.0, 0.0]);
        let out = stream_power_erosion(&graph, &elev, ratio(0.10), 30);
        assert!(out[2].get() < 5.0);
    }

    #[test]
    fn iteration_cap_is_respected() {
        // A huge request is capped at MAX_ITERS and still terminates + erodes.
        let graph = line_graph(5);
        let elev = meters(&[0.0, 1.0, 5.0, 1.0, 0.0]);
        let out = stream_power_erosion(&graph, &elev, ratio(0.10), 100_000);
        assert!(out[2].get() < 5.0);
    }

    #[test]
    fn zero_iterations_returns_input_unchanged() {
        let graph = line_graph(3);
        let elev = meters(&[0.0, 4.0, 0.0]);
        let out = stream_power_erosion(&graph, &elev, ratio(0.10), 0);
        assert_eq!(
            out.iter().map(|m| m.get()).collect::<Vec<_>>(),
            vec![0.0, 4.0, 0.0]
        );
    }

    #[test]
    fn flat_field_is_unchanged() {
        // No slope anywhere ⇒ nothing erodes (the slope==0 collapse).
        let graph = line_graph(4);
        let elev = meters(&[2.0, 2.0, 2.0, 2.0]);
        let out = stream_power_erosion(&graph, &elev, ratio(0.25), 10);
        assert!(out.iter().all(|m| m.get() == 2.0));
    }

    #[test]
    fn isolated_region_with_no_neighbours_is_unchanged() {
        // Region 2 has no neighbours: seed-with-h ⇒ slope 0 ⇒ unchanged.
        let graph = RegionGraph {
            offsets: vec![0, 1, 2, 2],
            neighbours: vec![1, 0],
        };
        let elev = meters(&[0.0, 3.0, 9.0]);
        let out = stream_power_erosion(&graph, &elev, ratio(0.5), 5);
        assert_eq!(out[2].get(), 9.0);
    }

    #[test]
    fn deterministic_same_input() {
        let graph = line_graph(5);
        let elev = meters(&[0.0, 2.0, 4.0, 2.0, 0.0]);
        let a = stream_power_erosion(&graph, &elev, ratio(0.15), 20);
        let b = stream_power_erosion(&graph, &elev, ratio(0.15), 20);
        let av: Vec<f32> = a.iter().map(|m| m.get()).collect();
        let bv: Vec<f32> = b.iter().map(|m| m.get()).collect();
        assert_eq!(av, bv);
    }

    #[test]
    fn out_of_range_neighbour_is_ignored() {
        // Neighbour index past the field ⇒ the unwrap_or(&h) fallback keeps slope
        // 0 for that edge; region stays put rather than panicking.
        let graph = RegionGraph {
            offsets: vec![0, 1, 1],
            neighbours: vec![9],
        };
        let elev = meters(&[5.0, 3.0]);
        let out = stream_power_erosion(&graph, &elev, ratio(0.5), 3);
        assert_eq!(out[0].get(), 5.0);
    }
}
