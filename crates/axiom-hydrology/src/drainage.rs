//! Steepest-descent receivers and downstream flow accumulation.
//!
//! [`compute_receivers`] gives each region its *receiver* — the neighbour it
//! drains into — as the `(elevation, index)`-least region over its own site and
//! its neighbours. A region that is its own least (a local minimum / sink) is its
//! own receiver. This is a single deterministic `fold` per region: lowest
//! elevation wins, ties break to the smallest index, and the region itself seeds
//! the fold so a genuine sink resolves to itself with no branch.
//!
//! [`flow_accumulation`] pushes one unit of rainfall from every region down the
//! receiver chain. Processing regions in **descending elevation order** (ties by
//! ascending index) guarantees a region's full upstream contribution is known
//! before it scatters into its receiver — the classic O(n) accumulation, but the
//! `sort_by` + mutating loop become a sorted index vector (sorting is data
//! movement, not control flow) plus a `for_each` scatter whose only conditional
//! (don't add a sink to itself) is a branchless `then_some`/`unwrap_or` select.

use std::cmp::Ordering;

use axiom_geosphere::{RegionGraph, RegionId};
use axiom_kernel::{Meters, Ratio};

/// Each region's receiver: the `(elevation, index)`-least region among itself and
/// its neighbours. A local minimum is its own receiver (a sink). Deterministic:
/// lowest elevation wins, ties break to the smallest region index.
pub fn compute_receivers(graph: &RegionGraph, elevation: &[Meters]) -> Vec<RegionId> {
    let n = elevation.len();
    (0..n)
        .map(|r| {
            let here = elevation[r].get();
            let best = graph.neighbours_of(RegionId(r as u32)).iter().fold(
                (here, r as u32),
                |(best_v, best_i), &nb| {
                    let nv = elevation.get(nb as usize).map_or(best_v, |m| m.get());
                    ((nv, nb) < (best_v, best_i))
                        .then_some((nv, nb))
                        .unwrap_or((best_v, best_i))
                },
            );
            RegionId(best.1)
        })
        .collect()
}

/// Downstream flow accumulation: every region contributes one unit of rainfall,
/// summed down the receiver chain, so each region ends holding its total upstream
/// contributing count (as a dimensionless [`Ratio`]). Deterministic — a stable
/// descending-elevation processing order with an ascending-index tie-break.
pub fn flow_accumulation(graph: &RegionGraph, elevation: &[Meters]) -> Vec<Ratio> {
    let n = elevation.len();
    let receivers = compute_receivers(graph, elevation);
    // Descending elevation, ascending index on ties — a region drains only after
    // all higher regions have scattered into it.
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by(|&a, &b| {
        elevation[b as usize]
            .get()
            .partial_cmp(&elevation[a as usize].get())
            .unwrap_or(Ordering::Equal)
            .then(a.cmp(&b))
    });
    let mut flow: Vec<f32> = vec![1.0; n];
    order.iter().for_each(|&r| {
        let ri = r as usize;
        let target = receivers[ri].index();
        let contrib = flow[ri];
        // A sink (receiver == self) adds nothing to itself; otherwise push the
        // region's accumulated flow into its receiver — a branchless select.
        let delta = (target != ri).then_some(contrib).unwrap_or(0.0);
        flow[target] += delta;
    });
    flow.into_iter().map(Ratio::finite_or_zero).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_graphs::{line_graph, meters};

    #[test]
    fn receivers_point_downhill() {
        // Ascending line: every region drains to its lower-indexed neighbour,
        // region 0 (the global min) is its own sink.
        let graph = line_graph(5);
        let elev = meters(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        let recv = compute_receivers(&graph, &elev);
        assert_eq!(
            recv,
            vec![
                RegionId(0),
                RegionId(0),
                RegionId(1),
                RegionId(2),
                RegionId(3)
            ]
        );
    }

    #[test]
    fn tie_breaks_to_smallest_index() {
        // Region 1 sits between two equal-elevation neighbours (0 and 2); the
        // (elevation, index) tie-break sends it to the smaller index, 0.
        let graph = line_graph(3);
        let elev = meters(&[1.0, 1.0, 1.0]);
        let recv = compute_receivers(&graph, &elev);
        // All equal ⇒ each region's least (elevation,index) candidate is index 0
        // for regions 0 and 1; region 2's neighbours are {1}, min(1,2)->1.
        assert_eq!(recv[0], RegionId(0));
        assert_eq!(recv[1], RegionId(0));
        assert_eq!(recv[2], RegionId(1));
    }

    #[test]
    fn flow_accumulates_downstream_and_conserves_mass() {
        // Descending toward region 0: flow piles up at the bottom.
        let graph = line_graph(5);
        let elev = meters(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        let flow = flow_accumulation(&graph, &elev);
        let values: Vec<f32> = flow.iter().map(|r| r.get()).collect();
        // Region 0 collects everything upstream: all 5 units.
        assert_eq!(values, vec![5.0, 4.0, 3.0, 2.0, 1.0]);
    }

    #[test]
    fn monotonic_non_increasing_upstream() {
        let graph = line_graph(6);
        let elev = meters(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        let flow = flow_accumulation(&graph, &elev);
        // Each region holds at least as much as the one above it.
        assert!(flow.windows(2).all(|w| w[0].get() >= w[1].get()));
    }

    #[test]
    fn deterministic_same_input() {
        let graph = line_graph(5);
        let elev = meters(&[0.0, 2.0, 1.0, 3.0, 2.0]);
        let a = flow_accumulation(&graph, &elev);
        let b = flow_accumulation(&graph, &elev);
        let av: Vec<f32> = a.iter().map(|r| r.get()).collect();
        let bv: Vec<f32> = b.iter().map(|r| r.get()).collect();
        assert_eq!(av, bv);
    }

    #[test]
    fn out_of_range_neighbour_is_ignored() {
        // A malformed graph with a neighbour index past the field: the map_or
        // fallback keeps the current best rather than panicking.
        let graph = RegionGraph {
            offsets: vec![0, 1, 1],
            neighbours: vec![9],
        };
        let elev = meters(&[5.0, 3.0]);
        let recv = compute_receivers(&graph, &elev);
        // Region 0's only neighbour (9) is out of range ⇒ region 0 is its own sink.
        assert_eq!(recv[0], RegionId(0));
        assert_eq!(recv[1], RegionId(1));
    }
}
