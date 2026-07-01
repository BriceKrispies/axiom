//! Multi-source ocean-distance over the region graph, as a bounded wavefront.
//!
//! A classic multi-source BFS (a `VecDeque` seeded from every ocean region) is
//! rewritten as **bounded wavefront relaxation**, the sanctioned Axiom substitute
//! for a queue: seed each source region at [`HopDistance::ZERO`] and every other
//! region at [`HopDistance::UNREACHABLE`], then run `region_count` relaxation
//! passes — one more than the graph's diameter can ever be — each pass lowering
//! every region to `min(self, 1 + least neighbour)`. No `while`, no queue, no
//! neighbour `if`: out-of-range neighbours and empty neighbourhoods read
//! `UNREACHABLE`, and [`HopDistance::plus_one`] saturates so the sentinel never
//! yields a finite hop. The result is byte-identical run-to-run (ordered fold,
//! saturating arithmetic, fixed pass count).

use axiom_geosphere::{RegionGraph, RegionId};

use crate::hop_distance::HopDistance;

/// The graph-hop distance from every region to the nearest `true` in `sources`.
///
/// `sources[r]` marks region `r` as a wavefront source (for moisture, "is this
/// region ocean?"). A region with no path to any source reads
/// [`HopDistance::UNREACHABLE`]; with no source at all, every region is
/// unreachable. The returned vector is `sources.len()` long and indexed by
/// region.
pub fn ocean_distance(graph: &RegionGraph, sources: &[bool]) -> Vec<HopDistance> {
    let mut dist: Vec<HopDistance> = sources
        .iter()
        .map(|&is_src| {
            is_src
                .then_some(HopDistance::ZERO)
                .unwrap_or(HopDistance::UNREACHABLE)
        })
        .collect();
    // region_count passes ≥ graph diameter, so the field is fully converged.
    let passes = dist.len();
    (0..passes).for_each(|_| relax_pass(graph, &mut dist));
    dist
}

/// One relaxation pass over every region, in index order. A region takes
/// `min(current, 1 + least neighbour)`; an out-of-range or missing neighbour
/// reads `UNREACHABLE` and so cannot lower anything. In-place, so a region
/// relaxed earlier in the same pass propagates forward immediately — this only
/// speeds convergence; the bounded pass count fixes the final result regardless
/// of order.
fn relax_pass(graph: &RegionGraph, dist: &mut [HopDistance]) {
    let n = dist.len();
    (0..n).for_each(|r| {
        let least_neighbour = graph.neighbours_of(RegionId(r as u32)).iter().fold(
            HopDistance::UNREACHABLE,
            |acc, &nb| {
                acc.min(
                    dist.get(nb as usize)
                        .copied()
                        .unwrap_or(HopDistance::UNREACHABLE),
                )
            },
        );
        dist[r] = dist[r].min(least_neighbour.plus_one());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_graphs::line_graph;

    #[test]
    fn line_distances_match_hand_bfs() {
        // Line 0-1-2-3-4, only region 0 is a source. Distance == index.
        let graph = line_graph(5);
        let sources = [true, false, false, false, false];
        let dist = ocean_distance(&graph, &sources);
        assert_eq!(
            dist.iter().map(|d| d.steps()).collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2), Some(3), Some(4)]
        );
    }

    #[test]
    fn multi_source_takes_the_nearer_source() {
        // Sources at both ends: the middle region is 2 hops from either.
        let graph = line_graph(5);
        let sources = [true, false, false, false, true];
        let dist = ocean_distance(&graph, &sources);
        assert_eq!(
            dist.iter().map(|d| d.steps()).collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2), Some(1), Some(0)]
        );
    }

    #[test]
    fn no_source_leaves_everything_unreachable() {
        let graph = line_graph(3);
        let sources = [false, false, false];
        let dist = ocean_distance(&graph, &sources);
        assert!(dist.iter().all(|d| !d.is_reachable()));
    }

    #[test]
    fn disconnected_region_is_unreachable() {
        // Region 2 has no neighbours (line only wires 0-1); it never reaches the
        // source at 0, exercising the empty-neighbourhood + UNREACHABLE arm.
        let graph = RegionGraph {
            offsets: vec![0, 1, 2, 2],
            neighbours: vec![1, 0],
        };
        let sources = [true, false, false];
        let dist = ocean_distance(&graph, &sources);
        assert_eq!(dist[0].steps(), Some(0));
        assert_eq!(dist[1].steps(), Some(1));
        assert!(!dist[2].is_reachable());
    }

    #[test]
    fn empty_graph_yields_empty_field() {
        let dist = ocean_distance(&RegionGraph::default(), &[]);
        assert!(dist.is_empty());
    }
}
