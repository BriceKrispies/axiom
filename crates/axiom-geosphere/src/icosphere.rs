//! Geodesic-icosphere construction: base icosahedron, barycentric subdivision,
//! unit-sphere projection, shared-vertex deduplication, and outward triangles.
//!
//! The pipeline is deterministic and platform-stable: for a given subdivision
//! level the emitted `sites` and `triangles` are byte-identical on every run,
//! because region indices are assigned in the fixed lattice traversal order (the
//! dedup map is only ever probed by key — its iteration order never reaches the
//! output).

use std::collections::HashMap;

use axiom_math::Vec3;

/// A fixed geodesic icosphere: unit-sphere region sites and outward-CCW triangle
/// faces of region indices. Topology is fixed for one generation; consumers hang
/// their own per-region / per-triangle scalar fields off these indices.
#[derive(Debug, Clone, Default)]
pub struct Icosphere {
    /// Unit-length region-centre directions, indexed by region id.
    pub sites: Vec<Vec3>,
    /// Triangle faces, each three region indices, wound CCW when viewed from
    /// outside (so the face normal points away from the sphere centre).
    pub triangles: Vec<[u32; 3]>,
    /// Subdivision level used to build this sphere (quantises the region count).
    pub subdivisions: u32,
}

impl Icosphere {
    /// Number of regions (unit sites) on this sphere.
    pub fn region_count(&self) -> usize {
        self.sites.len()
    }
}

/// Choose the smallest icosphere subdivision level whose region count
/// (`10*4^n + 2`) meets `target`, capped at 9 (~2.6M regions) for a perf ceiling.
pub fn subdivisions_for_target(target: u32) -> u32 {
    (0..9u32)
        .find(|n| 10u64 * 4u64.pow(*n) + 2 >= target as u64)
        .unwrap_or(9)
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

/// Vertex pool that deduplicates by quantised unit position. Indices are handed
/// out as `sites.len()` at first sight of a key, so identity is insertion-order
/// deterministic and independent of the map's hasher.
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

    /// Normalise `p` onto the unit sphere and return its deduplicated region
    /// index, allocating a new one (and storing the site) only on first sight.
    fn intern(&mut self, p: Vec3) -> u32 {
        let unit = p.normalize().unwrap_or(Vec3::UNIT_X);
        let key = quantise_key(unit);
        let next = self.sites.len() as u32;
        let idx = *self.index.entry(key).or_insert(next);
        // Branchless get-or-insert: push the site only when this key was new
        // (its assigned index equals the `next` slot we offered).
        (idx == next).then(|| self.sites.push(unit));
        idx
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
/// normal `(b-a)x(c-a)` must agree with the outward radial direction `a+b+c`.
/// The table picks the CCW arm when the dot is non-negative and the flipped arm
/// otherwise, so the choice carries no control-flow branch.
fn orient_outward(a: u32, b: u32, c: u32, sites: &[Vec3]) -> [u32; 3] {
    let pa = sites[a as usize];
    let pb = sites[b as usize];
    let pc = sites[c as usize];
    let normal = pb.subtract(pa).cross(pc.subtract(pa));
    let outward = pa.add(pb).add(pc);
    [[a, c, b], [a, b, c]][usize::from(normal.dot(outward) >= 0.0)]
}

/// Row-major flat index of lattice point `(i, j)` (`0 <= j <= i`) within a face's
/// triangular grid: row `i` holds `i+1` points and starts at `i*(i+1)/2`.
const fn tri_index(i: usize, j: usize) -> usize {
    i * (i + 1) / 2 + j
}

/// Barycentric lattice point `(i, j)` of a base face, before unit projection.
/// `w0` weights the apex `v0`; `fj = j/i` splits the remaining weight between
/// `v1` and `v2` (0 on the apex row, where `i == 0`).
fn lattice_point(v0: Vec3, v1: Vec3, v2: Vec3, n: usize, i: usize, j: usize) -> Vec3 {
    let fi = i as f32 / n as f32;
    let fj = (i != 0).then(|| j as f32 / i as f32).unwrap_or(0.0);
    let w2 = fi * fj;
    let w1 = fi * (1.0 - fj);
    let w0 = 1.0 - fi;
    v0.mul_scalar(w0)
        .add(v1.mul_scalar(w1))
        .add(v2.mul_scalar(w2))
}

/// Build a unit icosphere at the given subdivision.
///
/// Builds the base icosahedron, subdivides each face into `4^subdivisions`
/// triangles by barycentric-lattice sampling, normalises every vertex to the unit
/// sphere, deduplicates shared edge/corner vertices into single region indices,
/// and emits outward-facing CCW triangles of region indices.
pub fn build_icosphere(subdivisions: u32) -> Icosphere {
    let (base_verts, base_faces) = base_icosahedron();
    // Resolution: n = 2^subdivisions lattice segments per base edge.
    let n = 1usize << subdivisions as usize;

    let (pool, triangles) = base_faces.iter().fold(
        (VertexPool::new(), Vec::<[u32; 3]>::new()),
        |(mut pool, mut triangles), face| {
            let v0 = base_verts[face[0]];
            let v1 = base_verts[face[1]];
            let v2 = base_verts[face[2]];

            // Intern the face's lattice in row-major (i, then j) order, so
            // `flat[tri_index(i, j)]` is region-index of lattice point (i, j).
            let flat: Vec<u32> = (0..=n)
                .flat_map(|i| (0..=i).map(move |j| lattice_point(v0, v1, v2, n, i, j)))
                .map(|p| pool.intern(p))
                .collect();

            // Stitch the lattice into upward triangles (`c, a, b`) and, where a
            // downward triangle fits (`j + 1 < i`), a second one (`c, b, d`).
            let sites: &[Vec3] = &pool.sites;
            let flat: &[u32] = &flat;
            let face_tris = (1..=n).flat_map(move |i| {
                (0..i).flat_map(move |j| {
                    let a = flat[tri_index(i, j)];
                    let b = flat[tri_index(i, j + 1)];
                    let c = flat[tri_index(i - 1, j)];
                    let up = orient_outward(c, a, b, sites);
                    let down = (j + 1 < i)
                        .then(|| orient_outward(c, b, flat[tri_index(i - 1, j + 1)], sites));
                    std::iter::once(up).chain(down)
                })
            });
            triangles.extend(face_tris);
            (pool, triangles)
        },
    );

    Icosphere {
        sites: pool.sites,
        triangles,
        subdivisions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdivisions_meet_target_and_cap_at_nine() {
        // Smallest n with 10*4^n + 2 >= target.
        assert_eq!(subdivisions_for_target(0), 0); // 12 >= 0
        assert_eq!(subdivisions_for_target(12), 0);
        assert_eq!(subdivisions_for_target(13), 1); // 42 >= 13
        assert_eq!(subdivisions_for_target(42), 1);
        assert_eq!(subdivisions_for_target(43), 2); // 162 >= 43
                                                    // Beyond subdiv-9's region count the search finds nothing and caps at 9.
        assert_eq!(subdivisions_for_target(u32::MAX), 9);
    }

    #[test]
    fn vertex_count_matches_icosphere_identity() {
        (0..=4u32).for_each(|n| {
            let ico = build_icosphere(n);
            let expected = 10usize * 4usize.pow(n) + 2;
            assert_eq!(ico.region_count(), expected, "subdivision {n} vertex count");
        });
    }

    #[test]
    fn triangle_count_matches_identity() {
        (0..=4u32).for_each(|n| {
            let ico = build_icosphere(n);
            let expected = 20usize * 4usize.pow(n);
            assert_eq!(ico.triangles.len(), expected, "subdivision {n} tri count");
        });
    }

    #[test]
    fn all_sites_are_unit_length() {
        let ico = build_icosphere(3);
        ico.sites.iter().enumerate().for_each(|(i, s)| {
            let len = s.length();
            assert!((len - 1.0).abs() < 1.0e-4, "site {i} length {len} not unit");
        });
    }

    #[test]
    fn triangle_indices_are_in_range() {
        let ico = build_icosphere(2);
        let n = ico.sites.len() as u32;
        ico.triangles.iter().for_each(|tri| {
            tri.iter()
                .for_each(|&v| assert!(v < n, "index {v} out of range {n}"));
        });
    }

    #[test]
    fn subdivision_zero_is_the_bare_icosahedron() {
        let ico = build_icosphere(0);
        assert_eq!(ico.subdivisions, 0);
        assert_eq!(ico.region_count(), 12);
        assert_eq!(ico.triangles.len(), 20);
    }

    #[test]
    fn build_is_deterministic() {
        let a = build_icosphere(3);
        let b = build_icosphere(3);
        assert_eq!(a.sites.len(), b.sites.len());
        assert!(a.sites.iter().zip(&b.sites).all(|(x, y)| x == y));
        assert_eq!(a.triangles, b.triangles);
    }

    /// `orient_outward` returns the triangle unchanged when its natural winding
    /// already points outward, and flips it when it points inward — exercising
    /// both arms of the branchless winding select.
    #[test]
    fn orient_outward_flips_only_inward_windings() {
        // Three unit sites; (0,1,2) is CCW seen from outside (+radial).
        let sites = vec![
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let outward = orient_outward(0, 1, 2, &sites);
        let inward = orient_outward(0, 2, 1, &sites);
        // One winding is kept, the mirror winding is flipped back to it.
        assert_eq!(outward, inward);
        // Exactly one of the two inputs was reordered.
        let kept = outward == [0, 1, 2];
        let flipped = inward == [0, 1, 2];
        assert!(kept | flipped, "one input must already be outward");
    }
}
