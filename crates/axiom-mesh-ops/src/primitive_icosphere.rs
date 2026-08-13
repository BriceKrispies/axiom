//! The geodesic ("ico") sphere: an icosahedron refined by recursive edge
//! bisection and reprojected onto the sphere.

use core::f32::consts::{PI, TAU};
use std::collections::BTreeMap;

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

use crate::tessellation::Subdivisions;

/// The golden ratio, `(1 + sqrt 5) / 2`. Written out because `sqrt` is not a
/// `const fn`.
const GOLDEN: f32 = 1.618_034;

/// The twelve icosahedron vertices: three mutually orthogonal golden rectangles.
const BASE_VERTICES: [[f32; 3]; 12] = [
    [-1.0, GOLDEN, 0.0],
    [1.0, GOLDEN, 0.0],
    [-1.0, -GOLDEN, 0.0],
    [1.0, -GOLDEN, 0.0],
    [0.0, -1.0, GOLDEN],
    [0.0, 1.0, GOLDEN],
    [0.0, -1.0, -GOLDEN],
    [0.0, 1.0, -GOLDEN],
    [GOLDEN, 0.0, -1.0],
    [GOLDEN, 0.0, 1.0],
    [-GOLDEN, 0.0, -1.0],
    [-GOLDEN, 0.0, 1.0],
];

/// The twenty icosahedron faces, each wound counter-clockwise seen from outside.
const BASE_FACES: [[u32; 3]; 20] = [
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

/// A geodesic sphere of `radius` centred on the origin.
///
/// Every triangle has near-equal area, which is what a
/// [`crate::uv_sphere`](crate::uv_sphere) cannot offer: there are no poles, no
/// pinched fans, and no band of slivers. The price is that there is no clean
/// rectangular texture wrap.
///
/// Each subdivision level bisects every edge and reprojects the three new
/// vertices onto the sphere, turning one triangle into four. Because the two
/// triangles sharing an edge agree on that edge's midpoint, the counts are
/// exactly `10 * 4^n + 2` vertices and `20 * 4^n` triangles — 12/20 at level 0,
/// 42/80 at level 1. (A generator that forgets to share midpoints reports 60
/// vertices at level 1; the count *is* the proof that they are shared.)
///
/// # Seams
///
/// Unlike every ring-based generator in this layer, the icosphere does **not**
/// duplicate a seam vertex: its vertex set is exactly the shared geodesic
/// lattice, which the counts above pin down. UVs are therefore a spherical
/// projection — `u = atan2(z, x) / TAU` wrapped into `0..1`, `v = 0` at `-Y` and
/// `1` at `+Y` — and the triangles that straddle `u = 0` interpolate the long way
/// round. A caller who needs a seamless wrap wants the UV sphere.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when `radius` is not strictly positive.
pub fn icosphere(radius: Meters, subdivisions: Subdivisions) -> MeshResult<Mesh> {
    (radius.get() > 0.0)
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "an icosphere needs a strictly positive radius",
            )
        })
        .and_then(|()| {
            let r = radius.get();
            let (directions, faces) = (0..subdivisions.get()).fold(
                (base_directions(), BASE_FACES.to_vec()),
                |(vertices, faces), _| refine(vertices, &faces),
            );
            let positions = directions.iter().map(|d| d.mul_scalar(r)).collect();
            let uvs = directions.iter().copied().map(spherical_uv).collect();
            Mesh::from_streams(MeshStreams {
                normals: directions,
                uvs,
                ..MeshStreams::new(positions, faces.into_iter().flatten().collect())
            })
        })
}

/// The twelve base vertices projected onto the unit sphere.
fn base_directions() -> Vec<Vec3> {
    BASE_VERTICES
        .iter()
        .map(|v| unit(Vec3::new(v[0], v[1], v[2])))
        .collect()
}

/// One refinement level: every triangle becomes four, sharing the three edge
/// midpoints with its neighbours.
///
/// The whole level is a single `fold` over the current faces, carrying the
/// growing vertex list and the edge cache. There is no recursion — `icosphere`
/// applies this as a bounded fold over `0..levels` — and the cache is a
/// [`BTreeMap`] keyed on the *sorted* endpoint pair, so a midpoint is minted
/// once, indices are handed out in traversal order, and the output is
/// byte-identical on every run (a hash map's iteration order is not a fact this
/// layer is allowed to depend on).
fn refine(vertices: Vec<Vec3>, faces: &[[u32; 3]]) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let (vertices, _, refined) = faces.iter().fold(
        (
            vertices,
            BTreeMap::<(u32, u32), u32>::new(),
            Vec::with_capacity(faces.len() * 4),
        ),
        |(mut verts, mut cache, mut refined), face| {
            let ab = midpoint(&mut verts, &mut cache, face[0], face[1]);
            let bc = midpoint(&mut verts, &mut cache, face[1], face[2]);
            let ca = midpoint(&mut verts, &mut cache, face[2], face[0]);
            // The corner triangles keep the parent's winding; the centre
            // triangle inherits it from the midpoint order.
            refined.extend([
                [face[0], ab, ca],
                [face[1], bc, ab],
                [face[2], ca, bc],
                [ab, bc, ca],
            ]);
            (verts, cache, refined)
        },
    );
    (vertices, refined)
}

/// The shared index of the unit-sphere midpoint of edge `(i, j)`, minting it on
/// first sight. The key is sorted so both triangles owning the edge agree.
fn midpoint(
    vertices: &mut Vec<Vec3>,
    cache: &mut BTreeMap<(u32, u32), u32>,
    i: u32,
    j: u32,
) -> u32 {
    let key = (i.min(j), i.max(j));
    let next = vertices.len() as u32;
    let projected = unit(vertices[i as usize].add(vertices[j as usize]));
    let index = *cache.entry(key).or_insert(next);
    // Branchless get-or-insert: the vertex is appended only when the cache
    // accepted the slot we offered, i.e. when the key was new.
    (index == next).then(|| vertices.push(projected));
    index
}

/// Project onto the unit sphere. The fallback is unreachable for every vector
/// this module builds (a sum of two non-antipodal unit vectors), and exists only
/// because normalization is fallible in general.
fn unit(v: Vec3) -> Vec3 {
    v.normalize().unwrap_or(Vec3::UNIT_Y)
}

/// The spherical UV of an outward unit direction: `u` wraps once around `+Y`
/// starting at `+X`, `v` runs `0` at `-Y` to `1` at `+Y`.
fn spherical_uv(d: Vec3) -> Vec2 {
    Vec2::new(
        (d.z.atan2(d.x) / TAU).rem_euclid(1.0),
        0.5 + d.y.clamp(-1.0, 1.0).asin() / PI,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: f32) -> Meters {
        Meters::finite_or_zero(v)
    }

    fn build(radius: f32, levels: u32) -> Mesh {
        icosphere(m(radius), Subdivisions::new(levels).unwrap()).unwrap()
    }

    fn assert_ccw_outward(mesh: &Mesh) {
        let p = mesh.positions();
        let n = mesh.normals();
        for t in mesh.indices().chunks(3) {
            let (i, j, k) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let geometric = p[j].subtract(p[i]).cross(p[k].subtract(p[i]));
            let outward = n[i].add(n[j]).add(n[k]);
            assert!(
                geometric.dot(outward) > 0.0,
                "triangle {t:?} is not CCW-outward"
            );
        }
    }

    #[test]
    fn level_zero_is_the_bare_icosahedron() {
        let s = build(1.0, 0);
        assert_eq!(s.vertex_count(), 12);
        assert_eq!(s.triangle_count(), 20);
        assert!(s.has_normals() & s.has_uvs());
        assert_ccw_outward(&s);
    }

    /// The dedup proof: a naive refinement mints three fresh midpoints per
    /// triangle and reports 12 + 20*3 = 72 vertices (60 of them duplicates).
    /// Sharing them gives exactly 42.
    #[test]
    fn level_one_shares_every_edge_midpoint() {
        let s = build(1.0, 1);
        assert_eq!(s.vertex_count(), 42);
        assert_eq!(s.triangle_count(), 80);
        assert_ccw_outward(&s);
    }

    #[test]
    fn counts_follow_the_geodesic_identity() {
        for n in 0..=3u32 {
            let s = build(1.0, n);
            assert_eq!(s.vertex_count(), 10 * 4usize.pow(n) + 2, "level {n} vertices");
            assert_eq!(s.triangle_count(), 20 * 4usize.pow(n), "level {n} triangles");
        }
    }

    #[test]
    fn every_vertex_lies_on_the_sphere_of_the_requested_radius() {
        let s = build(4.0, 2);
        for (i, p) in s.positions().iter().enumerate() {
            let d = p.length();
            assert!((d - 4.0).abs() < 1.0e-4, "vertex {i} at {d} is off the sphere");
        }
    }

    #[test]
    fn normals_are_the_radial_direction() {
        let s = build(2.0, 2);
        for (p, n) in s.positions().iter().zip(s.normals()) {
            assert!(n.subtract(p.normalize().unwrap()).length() < 1.0e-5);
            assert!((n.length() - 1.0).abs() < 1.0e-5);
        }
    }

    #[test]
    fn every_triangle_faces_outward_at_every_level() {
        for n in 0..=3u32 {
            assert_ccw_outward(&build(1.0, n));
        }
    }

    #[test]
    fn uvs_are_a_bounded_spherical_projection() {
        let s = build(1.0, 2);
        for uv in s.uvs() {
            assert!((0.0..=1.0).contains(&uv.x), "u {} out of range", uv.x);
            assert!((-1.0e-5..=1.0 + 1.0e-5).contains(&uv.y));
        }
        // The extreme v values belong to the vertices nearest the poles.
        let lowest = s
            .uvs()
            .iter()
            .zip(s.positions())
            .min_by(|a, b| a.0.y.total_cmp(&b.0.y))
            .unwrap();
        assert!(lowest.1.y < -0.9, "the smallest v is not near -Y");
    }

    #[test]
    fn refinement_is_deterministic_across_runs() {
        assert_eq!(build(1.0, 3), build(1.0, 3));
    }

    #[test]
    fn triangles_are_near_uniform_in_area() {
        let s = build(1.0, 2);
        let p = s.positions();
        let areas: Vec<f32> = s
            .indices()
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
                b.subtract(a).cross(c.subtract(a)).length() * 0.5
            })
            .collect();
        let min = areas.iter().copied().fold(f32::MAX, f32::min);
        let max = areas.iter().copied().fold(0.0f32, f32::max);
        let spread = max / min;
        assert!(min > 0.0);
        assert!(spread < 1.4, "area spread {spread} is not near-uniform");
    }

    #[test]
    fn a_non_positive_radius_is_rejected() {
        let level = Subdivisions::new(1).unwrap();
        assert_eq!(
            icosphere(m(0.0), level).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            icosphere(m(-3.0), level).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }
}
