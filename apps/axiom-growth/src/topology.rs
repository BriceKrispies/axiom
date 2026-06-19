//! Icosphere construction, subdivision, region-neighbour graph, ring validation.
//! Audit: worldgen `topology`/`region_neighbours`, OW-E18/SC-E1 ring validity.
use std::collections::HashMap;

use axiom_math::Vec3;

use crate::ids::RegionId;
use crate::model_planet::{Icosphere, PlanetGlobe, RegionGraph};

/// Choose an icosphere subdivision level for a region-count target.
/// Audit: perf cap at subdivision 9 (~2.6M sites).
pub fn subdivisions_for_target(target: u32) -> u32 {
    // 10*4^n+2 regions; pick smallest n meeting target, capped at 9.
    let mut n = 0u32;
    while n < 9 {
        let regions = 10u64 * 4u64.pow(n) + 2;
        if regions as u64 >= target as u64 {
            break;
        }
        n += 1;
    }
    n
}

/// Quantise a unit-sphere position to an integer key so coincident vertices
/// (shared along subdivided edges/corners) collapse to a single region index.
/// 1e6 buckets per axis is far finer than any inter-vertex spacing at subdiv 9.
fn quantise_key(p: Vec3) -> (i32, i32, i32) {
    const SCALE: f32 = 1.0e6;
    (
        (p.x * SCALE).round() as i32,
        (p.y * SCALE).round() as i32,
        (p.z * SCALE).round() as i32,
    )
}

/// Vertex pool that deduplicates by quantised unit position.
struct VertexPool {
    sites: Vec<Vec3>,
    index: HashMap<(i32, i32, i32), u32>,
}

impl VertexPool {
    fn new() -> Self {
        VertexPool {
            sites: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Normalise `p` to the unit sphere and return its deduplicated region index.
    fn intern(&mut self, p: Vec3) -> u32 {
        let unit = p.normalize().unwrap_or(Vec3::UNIT_X);
        let key = quantise_key(unit);
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        let i = self.sites.len() as u32;
        self.sites.push(unit);
        self.index.insert(key, i);
        i
    }
}

/// The 12 base icosahedron vertices and 20 faces, oriented CCW when viewed from
/// outside (so face normals point outward).
fn base_icosahedron() -> (Vec<Vec3>, Vec<[usize; 3]>) {
    // Golden ratio.
    let t = (1.0 + 5.0f32.sqrt()) / 2.0;
    let verts = vec![
        Vec3::new(-1.0, t, 0.0),
        Vec3::new(1.0, t, 0.0),
        Vec3::new(-1.0, -t, 0.0),
        Vec3::new(1.0, -t, 0.0),
        Vec3::new(0.0, -1.0, t),
        Vec3::new(0.0, 1.0, t),
        Vec3::new(0.0, -1.0, -t),
        Vec3::new(0.0, 1.0, -t),
        Vec3::new(t, 0.0, -1.0),
        Vec3::new(t, 0.0, 1.0),
        Vec3::new(-t, 0.0, -1.0),
        Vec3::new(-t, 0.0, 1.0),
    ];
    // Standard outward-facing CCW winding for the icosahedron.
    let faces = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    (verts, faces)
}

/// Force a triangle of unit-sphere positions to outward CCW winding: the face
/// normal `(b-a)x(c-a)` should agree with the outward radial direction.
fn orient_outward(a: u32, b: u32, c: u32, sites: &[Vec3]) -> [u32; 3] {
    let pa = sites[a as usize];
    let pb = sites[b as usize];
    let pc = sites[c as usize];
    let normal = pb.subtract(pa).cross(pc.subtract(pa));
    let outward = pa.add(pb).add(pc);
    if normal.dot(outward) >= 0.0 {
        [a, b, c]
    } else {
        [a, c, b]
    }
}

/// Build a unit icosphere at the given subdivision.
///
/// Builds the base icosahedron, subdivides each face into `4^subdivisions`
/// triangles by recursive midpoint insertion, normalises every vertex to the
/// unit sphere, deduplicates shared edge/corner vertices into single region
/// indices, and emits outward-facing CCW triangles of region indices.
pub fn build_icosphere(subdivisions: u32) -> Icosphere {
    let (base_verts, base_faces) = base_icosahedron();
    let mut pool = VertexPool::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();

    let steps = subdivisions as usize;

    for face in &base_faces {
        // Barycentric lattice of the base face, normalised onto the sphere.
        // Resolution n = 2^subdivisions points per edge segment.
        let n = 1usize << steps;
        let v0 = base_verts[face[0]];
        let v1 = base_verts[face[1]];
        let v2 = base_verts[face[2]];

        // grid[i][j] for i in 0..=n, j in 0..=i — region index of lattice point.
        let mut grid: Vec<Vec<u32>> = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let mut row = Vec::with_capacity(i + 1);
            for j in 0..=i {
                // Barycentric weights: w0 toward v0 (apex), w1 toward v1, w2 toward v2.
                let fi = i as f32 / n as f32;
                let fj = if i == 0 { 0.0 } else { j as f32 / i as f32 };
                let w2 = fi * fj;
                let w1 = fi * (1.0 - fj);
                let w0 = 1.0 - fi;
                let p = v0
                    .mul_scalar(w0)
                    .add(v1.mul_scalar(w1))
                    .add(v2.mul_scalar(w2));
                row.push(pool.intern(p));
            }
            grid.push(row);
        }

        // Stitch the lattice into upward- and downward-pointing triangles.
        for i in 1..=n {
            for j in 0..i {
                let a = grid[i][j];
                let b = grid[i][j + 1];
                let c = grid[i - 1][j];
                triangles.push(orient_outward(c, a, b, &pool.sites));
                if j + 1 < i {
                    let d = grid[i - 1][j + 1];
                    triangles.push(orient_outward(c, b, d, &pool.sites));
                }
            }
        }
    }

    Icosphere {
        sites: pool.sites,
        triangles,
        subdivisions,
    }
}

/// Build CSR region adjacency from triangle faces: for each region, the set of
/// regions it shares a triangle edge with, sorted ascending for determinism.
pub fn build_region_graph(ico: &Icosphere) -> RegionGraph {
    let region_count = ico.sites.len();
    let mut adjacency: Vec<Vec<u32>> = vec![Vec::new(); region_count];

    for tri in &ico.triangles {
        let [a, b, c] = *tri;
        for &(u, v) in &[(a, b), (b, c), (c, a)] {
            adjacency[u as usize].push(v);
            adjacency[v as usize].push(u);
        }
    }

    let mut offsets = Vec::with_capacity(region_count + 1);
    let mut neighbours = Vec::new();
    offsets.push(0u32);
    for set in adjacency.iter_mut() {
        set.sort_unstable();
        set.dedup();
        neighbours.extend_from_slice(set);
        offsets.push(neighbours.len() as u32);
    }

    RegionGraph {
        offsets,
        neighbours,
    }
}

/// Region-ring validation report. Audit: SC-E1 (bad_adjacency, tris_not_in_3_rings).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RingReport {
    pub bad_adjacency: u32,
    pub tris_not_in_3_rings: u32,
}

impl RingReport {
    pub fn is_valid(&self) -> bool {
        self.bad_adjacency == 0 && self.tris_not_in_3_rings == 0
    }
}

/// Validate dual region rings before hydrology. Audit: OW-E18, SC-E1.
///
/// For each triangle the three regions must be mutually adjacent in the region
/// graph (`bad_adjacency`), and every triangle edge must be shared by exactly
/// two triangles so each triangle sits in three closed rings around its corners
/// — an edge bordered by anything other than two triangles is a hole/non-
/// manifold seam and counts toward `tris_not_in_3_rings`.
pub fn validate_region_rings(globe: &PlanetGlobe) -> RingReport {
    let graph = &globe.graph;
    let mut report = RingReport::default();

    // Pair-membership check: each triangle's 3 regions mutually adjacent.
    let adjacent = |u: u32, v: u32| -> bool {
        graph
            .neighbours_of(RegionId(u))
            .binary_search(&v)
            .is_ok()
    };

    // Edge -> count of incident triangles (undirected, ordered key).
    let mut edge_tris: HashMap<(u32, u32), u32> = HashMap::new();
    let edge_key = |u: u32, v: u32| if u <= v { (u, v) } else { (v, u) };

    for tri in &globe.topology.triangles {
        let [a, b, c] = *tri;
        let mut bad = false;
        for &(u, v) in &[(a, b), (b, c), (c, a)] {
            if !adjacent(u, v) || !adjacent(v, u) {
                bad = true;
            }
            *edge_tris.entry(edge_key(u, v)).or_insert(0) += 1;
        }
        if bad {
            report.bad_adjacency += 1;
        }
    }

    // A triangle lies in three closed corner rings iff all three of its edges
    // are shared by exactly two triangles. Count offending triangles.
    for tri in &globe.topology.triangles {
        let [a, b, c] = *tri;
        let manifold = [(a, b), (b, c), (c, a)]
            .iter()
            .all(|&(u, v)| edge_tris.get(&edge_key(u, v)).copied() == Some(2));
        if !manifold {
            report.tris_not_in_3_rings += 1;
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_planet::PlanetGlobe;

    fn build_globe(subdivisions: u32) -> PlanetGlobe {
        let topology = build_icosphere(subdivisions);
        let graph = build_region_graph(&topology);
        let mut globe = PlanetGlobe {
            topology,
            graph,
            ..Default::default()
        };
        globe.resize_fields();
        globe
    }

    #[test]
    fn vertex_count_matches_icosphere_identity() {
        for n in 0..=4u32 {
            let ico = build_icosphere(n);
            let expected = 10usize * 4usize.pow(n) + 2;
            assert_eq!(
                ico.sites.len(),
                expected,
                "subdivision {n} vertex count"
            );
        }
    }

    #[test]
    fn triangle_count_matches_identity() {
        for n in 0..=4u32 {
            let ico = build_icosphere(n);
            let expected = 20usize * 4usize.pow(n);
            assert_eq!(
                ico.triangles.len(),
                expected,
                "subdivision {n} triangle count"
            );
        }
    }

    #[test]
    fn all_sites_are_unit_length() {
        let ico = build_icosphere(3);
        for (i, s) in ico.sites.iter().enumerate() {
            let len = s.length();
            assert!(
                (len - 1.0).abs() < 1.0e-4,
                "site {i} length {len} not unit"
            );
        }
    }

    #[test]
    fn triangle_indices_are_in_range() {
        let ico = build_icosphere(2);
        let n = ico.sites.len() as u32;
        for tri in &ico.triangles {
            for &v in tri {
                assert!(v < n, "index {v} out of range {n}");
            }
        }
    }

    #[test]
    fn every_region_has_five_or_six_neighbours_with_twelve_pentagons() {
        for n in 0..=3u32 {
            let ico = build_icosphere(n);
            let graph = build_region_graph(&ico);
            let mut pentagons = 0u32;
            for r in 0..ico.sites.len() {
                let deg = graph.neighbours_of(RegionId(r as u32)).len();
                assert!(
                    deg == 5 || deg == 6,
                    "region {r} has degree {deg} (subdiv {n})"
                );
                if deg == 5 {
                    pentagons += 1;
                }
            }
            assert_eq!(pentagons, 12, "exactly 12 pentagons at subdiv {n}");
        }
    }

    #[test]
    fn graph_is_symmetric() {
        let ico = build_icosphere(3);
        let graph = build_region_graph(&ico);
        for a in 0..ico.sites.len() as u32 {
            for &b in graph.neighbours_of(RegionId(a)) {
                let back = graph.neighbours_of(RegionId(b));
                assert!(
                    back.binary_search(&a).is_ok(),
                    "asymmetric edge {a}->{b}"
                );
            }
        }
    }

    #[test]
    fn neighbours_are_sorted_ascending() {
        let ico = build_icosphere(2);
        let graph = build_region_graph(&ico);
        for r in 0..ico.sites.len() as u32 {
            let ns = graph.neighbours_of(RegionId(r));
            assert!(ns.windows(2).all(|w| w[0] < w[1]), "region {r} unsorted");
        }
    }

    #[test]
    fn validate_region_rings_valid_on_fresh_globe() {
        for n in 0..=3u32 {
            let globe = build_globe(n);
            let report = validate_region_rings(&globe);
            assert_eq!(report.bad_adjacency, 0, "bad_adjacency at subdiv {n}");
            assert_eq!(
                report.tris_not_in_3_rings, 0,
                "tris_not_in_3_rings at subdiv {n}"
            );
            assert!(report.is_valid(), "ring report valid at subdiv {n}");
        }
    }

    #[test]
    fn euler_characteristic_holds() {
        // V - E + F = 2 for a sphere triangulation.
        let ico = build_icosphere(2);
        let graph = build_region_graph(&ico);
        let v = ico.sites.len();
        let edges: usize = graph.neighbours.len() / 2;
        let f = ico.triangles.len();
        assert_eq!(v as i64 - edges as i64 + f as i64, 2);
    }
}
