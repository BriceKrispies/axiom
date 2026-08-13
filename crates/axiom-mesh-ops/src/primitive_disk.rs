//! The flat circular primitives: the filled disk and the annular ring, both in
//! the XZ plane facing `+Y`.
//!
//! The two are separate generators rather than one radius pair with a zero case,
//! because their topology genuinely differs: a disk is a triangle fan around a
//! centre vertex with a *radial* UV chart, an annulus is a quad band with two
//! rings and a *wrapping* UV chart. Only the annulus needs a duplicated seam
//! vertex — the disk's `u` is `0.5 + 0.5·cos θ`, which closes on itself at
//! `θ = 0` and `θ = τ`, so a duplicate there would be a redundant vertex rather
//! than a seam.

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

use crate::tessellation::Segments;

/// A filled circle centred on the origin in the XZ plane, with `+Y` normals.
///
/// Vertex 0 is the centre; vertices `1..=segments` are the rim, counter-clockwise
/// from `+X` seen from `+Y`. The fan is `segments` triangles, all
/// counter-clockwise about the normal. UVs place the disk in the unit circle
/// inscribed in `0..1` — the centre at `(0.5, 0.5)`, `u` growing with `+X` and
/// `v` with `+Z` — so a square texture maps onto it without distortion.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when the radius is zero or negative.
pub fn disk(radius: Meters, segments: Segments) -> MeshResult<Mesh> {
    let r = radius.get();
    (r > 0.0)
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a disk needs a strictly positive radius",
            )
        })
        .and_then(|()| build_disk(r, segments.get()))
}

/// A flat ring between `inner_radius` and `outer_radius`, in the XZ plane with
/// `+Y` normals.
///
/// Two concentric rings of `segments + 1` vertices — the seam vertex is
/// duplicated so `u` reaches both `0` and `1` — inner ring first, then outer.
/// UVs wrap `u` once around the ring and run `v` from `0` at the inner edge to
/// `1` at the outer edge. The band is `2 * segments` counter-clockwise
/// triangles.
///
/// An `inner_radius` of exactly zero is legal and collapses the inner ring onto
/// the origin, which is the ring's own limiting case rather than an error; a
/// caller that wants that shape with fan topology and a radial chart should use
/// [`disk`] instead.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when the inner radius is negative, or
/// when the outer radius does not strictly exceed the inner one.
pub fn annulus(
    inner_radius: Meters,
    outer_radius: Meters,
    segments: Segments,
) -> MeshResult<Mesh> {
    let (inner, outer) = (inner_radius.get(), outer_radius.get());
    ((inner >= 0.0) & (outer > inner))
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "an annulus needs a non-negative inner radius and a strictly larger outer radius",
            )
        })
        .and_then(|()| build_annulus(inner, outer, segments.get()))
}

fn build_disk(radius: f32, segments: u32) -> MeshResult<Mesh> {
    let step = core::f32::consts::TAU / segments as f32;
    let angle = move |i: u32| step * i as f32;
    Mesh::from_streams(MeshStreams {
        normals: vec![Vec3::UNIT_Y; segments as usize + 1],
        uvs: core::iter::once(Vec2::new(0.5, 0.5))
            .chain((0..segments).map(|i| {
                let a = angle(i);
                Vec2::new(0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin())
            }))
            .collect(),
        ..MeshStreams::new(
            core::iter::once(Vec3::ZERO)
                .chain((0..segments).map(|i| {
                    let a = angle(i);
                    Vec3::new(radius * a.cos(), 0.0, radius * a.sin())
                }))
                .collect(),
            (0..segments)
                .flat_map(|i| [0, 1 + (i + 1) % segments, 1 + i])
                .collect(),
        )
    })
}

fn build_annulus(inner: f32, outer: f32, segments: u32) -> MeshResult<Mesh> {
    let stride = segments + 1;
    let step = core::f32::consts::TAU / segments as f32;
    // Ring 0 is the inner edge, ring 1 the outer: indexing a two-entry table by
    // the ring keeps the radius choice a lookup rather than a branch.
    let radii = [inner, outer];
    Mesh::from_streams(MeshStreams {
        normals: vec![Vec3::UNIT_Y; 2 * stride as usize],
        uvs: (0..2 * stride)
            .map(|k| Vec2::new((k % stride) as f32 / segments as f32, (k / stride) as f32))
            .collect(),
        ..MeshStreams::new(
            (0..2 * stride)
                .map(|k| {
                    let a = step * (k % stride) as f32;
                    let r = radii[(k / stride) as usize];
                    Vec3::new(r * a.cos(), 0.0, r * a.sin())
                })
                .collect(),
            (0..segments)
                .flat_map(|i| {
                    let outer_i = stride + i;
                    [i, i + 1, outer_i + 1, i, outer_i + 1, outer_i]
                })
                .collect(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn eight() -> Segments {
        Segments::new(8).unwrap()
    }

    fn ccw_about_up(m: &Mesh) {
        let p = m.positions();
        for t in m.indices().chunks(3) {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            assert!(
                b.subtract(a).cross(c.subtract(a)).dot(Vec3::UNIT_Y) > 0.0,
                "triangle {t:?} is wound clockwise"
            );
        }
    }

    #[test]
    fn a_disk_is_a_fan_of_one_triangle_per_segment() {
        let m = disk(meters(2.0), eight()).unwrap();
        assert_eq!(m.vertex_count(), 9);
        assert_eq!(m.triangle_count(), 8);
        assert_eq!(m.positions()[0], Vec3::ZERO);
        assert_eq!(m.normals(), &[Vec3::UNIT_Y; 9]);
        assert!(m.tangents().is_empty());
    }

    #[test]
    fn every_disk_rim_vertex_sits_on_the_requested_radius() {
        let m = disk(meters(2.0), eight()).unwrap();
        for p in &m.positions()[1..] {
            assert!((p.length() - 2.0).abs() < 1.0e-5, "rim vertex {p:?}");
            assert_eq!(p.y, 0.0);
        }
        assert_eq!(m.positions()[1], Vec3::new(2.0, 0.0, 0.0));
    }

    #[test]
    fn the_disk_uv_chart_is_the_inscribed_unit_circle() {
        let m = disk(meters(2.0), eight()).unwrap();
        assert_eq!(m.uvs()[0], Vec2::new(0.5, 0.5));
        for uv in &m.uvs()[1..] {
            assert!((uv.distance(Vec2::new(0.5, 0.5)) - 0.5).abs() < 1.0e-5);
        }
        // The rim vertex at +X is at u = 1, and the one at +Z is at v = 1.
        assert!((m.uvs()[1].x - 1.0).abs() < 1.0e-6);
        assert!((m.uvs()[3].y - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn every_disk_triangle_winds_counter_clockwise() {
        ccw_about_up(&disk(meters(1.0), Segments::new(5).unwrap()).unwrap());
    }

    #[test]
    fn a_non_positive_disk_radius_is_rejected() {
        for r in [0.0, -1.0] {
            assert_eq!(
                disk(meters(r), eight()).unwrap_err().code(),
                MeshErrorCode::InvalidParameter
            );
        }
    }

    #[test]
    fn an_annulus_is_two_seam_duplicated_rings_and_a_quad_band() {
        let m = annulus(meters(1.0), meters(3.0), eight()).unwrap();
        assert_eq!(m.vertex_count(), 18);
        assert_eq!(m.triangle_count(), 16);
        assert_eq!(m.normals(), &[Vec3::UNIT_Y; 18]);
        // Nine inner then nine outer, each ring on its own radius.
        for p in &m.positions()[..9] {
            assert!((p.length() - 1.0).abs() < 1.0e-5);
        }
        for p in &m.positions()[9..] {
            assert!((p.length() - 3.0).abs() < 1.0e-5);
        }
    }

    #[test]
    fn the_annulus_seam_is_duplicated_so_u_reaches_both_ends() {
        let m = annulus(meters(1.0), meters(3.0), eight()).unwrap();
        // First and last vertex of a ring are the same point...
        assert!(m.positions()[0].distance(m.positions()[8]) < 1.0e-5);
        // ...but carry the two ends of the u range.
        assert_eq!(m.uvs()[0], Vec2::new(0.0, 0.0));
        assert_eq!(m.uvs()[8], Vec2::new(1.0, 0.0));
        assert_eq!(m.uvs()[9], Vec2::new(0.0, 1.0));
        assert_eq!(m.uvs()[17], Vec2::new(1.0, 1.0));
    }

    #[test]
    fn every_annulus_triangle_winds_counter_clockwise() {
        ccw_about_up(&annulus(meters(0.5), meters(2.0), Segments::new(6).unwrap()).unwrap());
    }

    #[test]
    fn a_zero_inner_radius_is_the_degenerate_ring_and_is_allowed() {
        let m = annulus(meters(0.0), meters(1.0), eight()).unwrap();
        assert_eq!(m.vertex_count(), 18);
        assert!(m.positions()[..9].iter().all(|p| *p == Vec3::ZERO));
    }

    #[test]
    fn a_negative_inner_or_non_increasing_outer_radius_is_rejected() {
        for (inner, outer) in [(-1.0, 2.0), (2.0, 2.0), (2.0, 1.0), (1.0, -1.0)] {
            assert_eq!(
                annulus(meters(inner), meters(outer), eight())
                    .unwrap_err()
                    .code(),
                MeshErrorCode::InvalidParameter,
                "accepted inner {inner} outer {outer}"
            );
        }
    }
}
