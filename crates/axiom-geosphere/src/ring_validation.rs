//! Dual region-ring validation: the check a consumer runs before treating the
//! icosphere's dual as a closed 2-manifold (e.g. before routing hydrology flow
//! around region rings).
//!
//! Two properties are counted, over neutral topology inputs only:
//! * every triangle's three regions are mutually adjacent in the region graph
//!   (`bad_adjacency`), and
//! * every triangle edge is shared by exactly two triangles, so each triangle
//!   sits in three closed corner rings — an edge bordered by anything other than
//!   two triangles is a hole / non-manifold seam (`tris_not_in_3_rings`).

use std::collections::HashMap;

use crate::icosphere::Icosphere;
use crate::ids::RegionId;
use crate::region_graph::RegionGraph;

/// Region-ring validation report: counts of triangles that fail mutual
/// region-adjacency and triangles not enclosed by three closed rings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RingReport {
    /// Triangles whose three regions are not all mutually adjacent.
    pub bad_adjacency: u32,
    /// Triangles with an edge not shared by exactly two triangles.
    pub tris_not_in_3_rings: u32,
}

impl RingReport {
    /// Whether the dual is a clean closed manifold (both counts zero).
    pub fn is_valid(&self) -> bool {
        (self.bad_adjacency == 0) & (self.tris_not_in_3_rings == 0)
    }
}

/// Undirected edge key, ordered so `(u, v)` and `(v, u)` collide.
fn edge_key(u: u32, v: u32) -> (u32, u32) {
    [(v, u), (u, v)][usize::from(u <= v)]
}

/// Validate the dual region rings of `ico` against its region graph `graph`.
///
/// Reads only the neutral topology — the triangle faces and the CSR neighbour
/// lists — so it knows nothing of any consumer's planet/scalar fields.
pub fn validate_region_rings(ico: &Icosphere, graph: &RegionGraph) -> RingReport {
    let adjacent =
        |u: u32, v: u32| -> bool { graph.neighbours_of(RegionId(u)).binary_search(&v).is_ok() };

    // Pass 1: edge -> incident-triangle count, plus per-triangle adjacency faults.
    let (edge_tris, bad_adjacency) = ico.triangles.iter().fold(
        (HashMap::<(u32, u32), u32>::new(), 0u32),
        |(mut edges, bad), &[a, b, c]| {
            let ring = [(a, b), (b, c), (c, a)];
            // Both directions must be present; `&` (not `&&`) keeps both probes.
            let bad_here = ring
                .iter()
                .any(|&(u, v)| !(adjacent(u, v) & adjacent(v, u)));
            ring.iter().for_each(|&(u, v)| {
                *edges.entry(edge_key(u, v)).or_insert(0) += 1;
            });
            (edges, bad + u32::from(bad_here))
        },
    );

    // Pass 2: a triangle is in three closed rings iff all three of its edges are
    // shared by exactly two triangles.
    let tris_not_in_3_rings = ico.triangles.iter().fold(0u32, |acc, &[a, b, c]| {
        let manifold = [(a, b), (b, c), (c, a)]
            .iter()
            .all(|&(u, v)| edge_tris.get(&edge_key(u, v)).copied() == Some(2));
        acc + u32::from(!manifold)
    });

    RingReport {
        bad_adjacency,
        tris_not_in_3_rings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icosphere::build_icosphere;
    use crate::region_graph::build_region_graph;

    #[test]
    fn fresh_globe_rings_are_valid() {
        (0..=3u32).for_each(|n| {
            let ico = build_icosphere(n);
            let graph = build_region_graph(&ico);
            let report = validate_region_rings(&ico, &graph);
            assert_eq!(report.bad_adjacency, 0, "bad_adjacency at subdiv {n}");
            assert_eq!(report.tris_not_in_3_rings, 0, "non-manifold at subdiv {n}");
            assert!(report.is_valid(), "ring report valid at subdiv {n}");
        });
    }

    #[test]
    fn broken_topology_is_flagged() {
        // One lone triangle with an empty graph: its regions are not adjacent
        // (bad_adjacency), and each edge is incident to only one triangle
        // (tris_not_in_3_rings).
        let ico = Icosphere {
            sites: vec![
                axiom_math::Vec3::UNIT_X,
                axiom_math::Vec3::UNIT_Y,
                axiom_math::Vec3::UNIT_Z,
            ],
            triangles: vec![[0, 1, 2]],
            subdivisions: 0,
        };
        let graph = RegionGraph::default();
        let report = validate_region_rings(&ico, &graph);
        assert_eq!(report.bad_adjacency, 1);
        assert_eq!(report.tris_not_in_3_rings, 1);
        assert!(!report.is_valid());
    }

    #[test]
    fn edge_key_is_order_independent() {
        assert_eq!(edge_key(2, 5), (2, 5));
        assert_eq!(edge_key(5, 2), (2, 5));
        assert_eq!(edge_key(4, 4), (4, 4));
    }
}
