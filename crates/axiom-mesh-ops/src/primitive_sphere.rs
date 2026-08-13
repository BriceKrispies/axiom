//! The latitude/longitude ("UV") sphere about `+Y`.

use core::f32::consts::{PI, TAU};

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

use crate::tessellation::{Rings, Segments};

/// A sphere of `radius` centred on the origin, tessellated as `rings` latitude
/// bands by `segments` longitude divisions about the `+Y` axis.
///
/// This is the parameterized sphere — the one with a clean `u`/`v` texture wrap
/// and visible pole convergence. Use [`crate::icosphere`] instead when uniform
/// triangle area matters more than the wrap.
///
/// Vertices form a `(rings + 1) x (segments + 1)` grid: the extra column
/// duplicates the seam so `u` reaches both `0.0` and `1.0`, and each pole row is
/// a full row of coincident positions so the converging triangles each carry
/// their own `u`. `v` is `0` at the `-Y` pole and `1` at the `+Y` pole.
///
/// Normals are exactly radial (`position / radius`) — they are generated as unit
/// directions and the positions are scaled copies of them, so the two can never
/// disagree.
///
/// The two triangles that would span a pole row are degenerate and are not
/// emitted, giving `2 * segments * (rings - 1)` triangles.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when `radius` is not strictly positive.
pub fn uv_sphere(radius: Meters, rings: Rings, segments: Segments) -> MeshResult<Mesh> {
    (radius.get() > 0.0)
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a sphere needs a strictly positive radius",
            )
        })
        .and_then(|()| {
            let (r, ring_count, seg) = (radius.get(), rings.get(), segments.get());
            let cols = seg + 1;
            let directions: Vec<Vec3> = (0..=ring_count)
                .flat_map(|i| (0..cols).map(move |j| direction(i, j, ring_count, seg)))
                .collect();
            let positions = directions.iter().map(|d| d.mul_scalar(r)).collect();
            let uvs = (0..=ring_count)
                .flat_map(|i| {
                    (0..cols).map(move |j| {
                        Vec2::new(j as f32 / seg as f32, i as f32 / ring_count as f32)
                    })
                })
                .collect();
            Mesh::from_streams(MeshStreams {
                normals: directions,
                uvs,
                ..MeshStreams::new(positions, band_indices(ring_count, seg))
            })
        })
}

/// The outward unit direction of grid vertex `(i, j)`.
///
/// `v = i / rings` runs from the `-Y` pole to the `+Y` pole, so the polar angle
/// is `PI * v` measured from `-Y`: `y = -cos(PI v)`.
fn direction(i: u32, j: u32, rings: u32, segments: u32) -> Vec3 {
    let phi = PI * i as f32 / rings as f32;
    let theta = TAU * j as f32 / segments as f32;
    Vec3::new(phi.sin() * theta.cos(), -phi.cos(), phi.sin() * theta.sin())
}

/// Stitch the vertex grid into quads, dropping the degenerate half of each
/// pole band. Winding is counter-clockwise seen from outside.
fn band_indices(rings: u32, segments: u32) -> Vec<u32> {
    let cols = segments + 1;
    (0..rings)
        .flat_map(move |i| {
            (0..segments).flat_map(move |j| {
                let (a, b) = (i * cols + j, i * cols + j + 1);
                let (c, d) = (a + cols, b + cols);
                (i > 0)
                    .then_some([a, c, b])
                    .into_iter()
                    .chain((i + 1 < rings).then_some([b, c, d]))
            })
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: f32) -> Meters {
        Meters::finite_or_zero(v)
    }

    fn build(radius: f32, rings: u32, segments: u32) -> Mesh {
        uv_sphere(
            m(radius),
            Rings::new(rings).unwrap(),
            Segments::new(segments).unwrap(),
        )
        .unwrap()
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
    fn a_four_by_eight_sphere_has_exact_counts() {
        let s = build(1.0, 4, 8);
        // (rings + 1) * (segments + 1) grid vertices.
        assert_eq!(s.vertex_count(), 5 * 9);
        // 2 * segments * (rings - 1) after dropping both pole rows' degenerates.
        assert_eq!(s.triangle_count(), 2 * 8 * 3);
        assert!(s.has_normals() & s.has_uvs());
        assert!(s.tangents().is_empty() & s.colors().is_empty());
    }

    #[test]
    fn the_coarsest_sphere_is_a_single_band_of_quads() {
        let s = build(1.0, 2, 3);
        assert_eq!(s.vertex_count(), 3 * 4);
        // 2 * segments * (rings - 1), with rings == 2.
        assert_eq!(s.triangle_count(), 2 * 3);
        assert_ccw_outward(&s);
    }

    #[test]
    fn every_vertex_lies_on_the_sphere() {
        let s = build(2.5, 6, 12);
        for (i, p) in s.positions().iter().enumerate() {
            let d = p.length();
            assert!((d - 2.5).abs() < 1.0e-4, "vertex {i} at {d} is off the sphere");
        }
    }

    #[test]
    fn normals_are_exactly_the_radial_direction() {
        let s = build(3.0, 5, 9);
        for (p, n) in s.positions().iter().zip(s.normals()) {
            let radial = p.normalize().unwrap();
            assert!(n.subtract(radial).length() < 1.0e-5);
            assert!((n.length() - 1.0).abs() < 1.0e-5);
        }
    }

    #[test]
    fn every_triangle_faces_outward() {
        assert_ccw_outward(&build(1.5, 8, 16));
    }

    #[test]
    fn the_poles_sit_on_the_axis_with_v_at_zero_and_one() {
        let s = build(2.0, 4, 8);
        for j in 0..9 {
            assert!(s.positions()[j].subtract(Vec3::new(0.0, -2.0, 0.0)).length() < 1.0e-5);
            assert_eq!(s.uvs()[j].y, 0.0);
        }
        let top = 4 * 9;
        for j in 0..9 {
            assert!(s.positions()[top + j]
                .subtract(Vec3::new(0.0, 2.0, 0.0))
                .length()
                < 1.0e-5);
            assert_eq!(s.uvs()[top + j].y, 1.0);
        }
    }

    #[test]
    fn the_seam_vertex_is_duplicated_so_u_reaches_both_ends() {
        let s = build(1.0, 4, 8);
        assert_eq!(s.uvs()[0], Vec2::new(0.0, 0.0));
        assert_eq!(s.uvs()[8], Vec2::new(1.0, 0.0));
        // Same place, different u — the seam is duplicated, never collapsed.
        let equator = 2 * 9;
        assert!(s.positions()[equator].distance(s.positions()[equator + 8]) < 1.0e-5);
        assert_eq!(s.uvs()[equator].x, 0.0);
        assert_eq!(s.uvs()[equator + 8].x, 1.0);
    }

    #[test]
    fn no_triangle_is_degenerate() {
        let s = build(1.0, 4, 8);
        let p = s.positions();
        for t in s.indices().chunks(3) {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let area = b.subtract(a).cross(c.subtract(a)).length();
            assert!(area > 1.0e-6, "degenerate triangle {t:?}");
        }
    }

    #[test]
    fn a_non_positive_radius_is_rejected() {
        let (r, s) = (Rings::new(4).unwrap(), Segments::new(8).unwrap());
        assert_eq!(
            uv_sphere(m(0.0), r, s).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            uv_sphere(m(-1.0), r, s).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(build(1.0, 6, 10), build(1.0, 6, 10));
    }
}
