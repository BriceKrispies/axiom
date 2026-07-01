//! The dual region-adjacency graph in compressed-sparse-row (CSR) form, derived
//! from an icosphere's triangle faces.
//!
//! Two regions are adjacent when they share a triangle edge. The graph is
//! symmetric, each region's neighbour list is sorted ascending and deduplicated,
//! and the whole structure is deterministic — the adjacency is accumulated in
//! region-index order, never in map order.

use crate::icosphere::Icosphere;
use crate::ids::RegionId;

/// Region adjacency in compressed-sparse-row form: `offsets[r]..offsets[r+1]`
/// slices `neighbours` for region `r`. This is the graph substrate higher layers
/// (hydrology flow, plate boundaries) run their traversals over.
#[derive(Debug, Clone, Default)]
pub struct RegionGraph {
    /// `offsets[r]..offsets[r+1]` slices into `neighbours` for region `r`; length
    /// is `region_count + 1`.
    pub offsets: Vec<u32>,
    /// Flattened neighbour region indices, each region's block sorted ascending.
    pub neighbours: Vec<u32>,
}

impl RegionGraph {
    /// The neighbour region indices of `region`, sorted ascending. An id outside
    /// the graph yields an empty slice.
    pub fn neighbours_of(&self, region: RegionId) -> &[u32] {
        let i = region.index();
        let start = self.offsets.get(i).copied().unwrap_or(0) as usize;
        let end = self.offsets.get(i + 1).copied().unwrap_or(0) as usize;
        self.neighbours.get(start..end).unwrap_or(&[])
    }
}

/// Build the CSR region-adjacency graph from an icosphere's triangle faces: for
/// each region, the set of regions it shares a triangle edge with, sorted
/// ascending and deduplicated for determinism.
pub fn build_region_graph(ico: &Icosphere) -> RegionGraph {
    let region_count = ico.sites.len();
    let mut adjacency: Vec<Vec<u32>> = vec![Vec::new(); region_count];

    // Each triangle contributes its three undirected edges, both directions.
    ico.triangles.iter().for_each(|&[a, b, c]| {
        [(a, b), (b, c), (c, a)].iter().for_each(|&(u, v)| {
            adjacency[u as usize].push(v);
            adjacency[v as usize].push(u);
        });
    });

    adjacency.iter_mut().for_each(|set| {
        set.sort_unstable();
        set.dedup();
    });

    let neighbours: Vec<u32> = adjacency.iter().flatten().copied().collect();
    // Offsets are the running prefix sum of per-region neighbour counts, led by 0.
    let offsets: Vec<u32> = std::iter::once(0u32)
        .chain(adjacency.iter().scan(0u32, |acc, set| {
            *acc += set.len() as u32;
            Some(*acc)
        }))
        .collect();

    RegionGraph {
        offsets,
        neighbours,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icosphere::build_icosphere;

    #[test]
    fn every_region_has_five_or_six_neighbours_with_twelve_pentagons() {
        (0..=3u32).for_each(|n| {
            let ico = build_icosphere(n);
            let graph = build_region_graph(&ico);
            let pentagons = (0..ico.sites.len())
                .filter(|&r| {
                    let deg = graph.neighbours_of(RegionId(r as u32)).len();
                    assert!(deg == 5 || deg == 6, "region {r} degree {deg} (subdiv {n})");
                    deg == 5
                })
                .count();
            assert_eq!(pentagons, 12, "exactly 12 pentagons at subdiv {n}");
        });
    }

    #[test]
    fn graph_is_symmetric() {
        let ico = build_icosphere(3);
        let graph = build_region_graph(&ico);
        (0..ico.sites.len() as u32).for_each(|a| {
            graph.neighbours_of(RegionId(a)).iter().for_each(|&b| {
                let back = graph.neighbours_of(RegionId(b));
                assert!(back.binary_search(&a).is_ok(), "asymmetric edge {a}->{b}");
            });
        });
    }

    #[test]
    fn neighbours_are_sorted_ascending() {
        let ico = build_icosphere(2);
        let graph = build_region_graph(&ico);
        (0..ico.sites.len() as u32).for_each(|r| {
            let ns = graph.neighbours_of(RegionId(r));
            assert!(ns.windows(2).all(|w| w[0] < w[1]), "region {r} unsorted");
        });
    }

    #[test]
    fn out_of_range_region_has_no_neighbours() {
        let ico = build_icosphere(1);
        let graph = build_region_graph(&ico);
        let past_end = RegionId(ico.sites.len() as u32);
        assert!(graph.neighbours_of(past_end).is_empty());
        // The empty default graph yields empty neighbours for any id, too.
        assert!(RegionGraph::default().neighbours_of(RegionId(0)).is_empty());
    }

    #[test]
    fn euler_characteristic_holds() {
        // V - E + F = 2 for a sphere triangulation.
        let ico = build_icosphere(2);
        let graph = build_region_graph(&ico);
        let v = ico.sites.len();
        let edges = graph.neighbours.len() / 2;
        let f = ico.triangles.len();
        assert_eq!(v as i64 - edges as i64 + f as i64, 2);
    }
}
