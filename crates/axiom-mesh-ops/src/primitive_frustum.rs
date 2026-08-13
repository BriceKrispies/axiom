//! The conical frustum about `+Y` — the general ring surface every round,
//! axis-aligned side wall is a case of.
//!
//! A frustum is two coaxial circles of independent radius joined by a ruled
//! surface, optionally closed by a disc at either end. A **cylinder** is the
//! case `bottom_radius == top_radius`; a **cone** is the case `top_radius == 0`.
//! Both live in their own files as named primitives, and both delegate here
//! rather than re-deriving the same trigonometry — in particular the *slant*
//! normal, which is the one part of this family that is easy to get wrong.
//!
//! # The slant normal
//!
//! The outward normal of the side wall is **not** the horizontal radial
//! direction unless the two radii are equal. Differentiating the surface
//!
//! ```text
//! p(theta, t) = ( R(t) cos theta,  -h + 2h t,  R(t) sin theta )
//! R(t) = bottom + (top - bottom) t
//! ```
//!
//! and taking `dp/dt x dp/dtheta` (the outward-facing order) gives, after
//! dropping the positive factor `R`,
//!
//! ```text
//! n(theta) ~ ( 2h cos theta,  bottom - top,  2h sin theta )
//! ```
//!
//! so the vertical component is exactly `0.0` when the radii match (a cylinder)
//! and strictly positive when the surface tapers inward going up (a cone).

use core::f32::consts::TAU;

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

use crate::cap_policy::CapPolicy;
use crate::tessellation::Segments;

/// A conical frustum about the `+Y` axis, centred on the origin.
///
/// The side wall runs from a circle of `bottom_radius` at `-half_height` to a
/// circle of `top_radius` at `+half_height`. `caps` selects the bottom
/// (`caps_start`) and top (`caps_end`) discs; a cap whose radius is zero is a
/// point and is silently omitted, because a fan of degenerate triangles is not
/// a cap.
///
/// UVs: `u` wraps `0..1` once around the axis with the seam vertex duplicated so
/// it reaches both ends, `v` is `0` on the bottom ring and `1` on the top ring.
/// Cap discs carry a planar projection instead, `(0.5 + 0.5 cos, 0.5 + 0.5 sin)`,
/// mirrored on the bottom so both discs read right-side-out.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when `half_height` is not strictly
/// positive, when either radius is negative, or when both radii are zero (which
/// would describe a line segment, not a surface).
pub fn frustum(
    bottom_radius: Meters,
    top_radius: Meters,
    half_height: Meters,
    segments: Segments,
    caps: CapPolicy,
) -> MeshResult<Mesh> {
    let bottom = bottom_radius.get();
    let top = top_radius.get();
    let half = half_height.get();
    validate(bottom, top, half).and_then(|()| {
        let n = segments.get();
        let start = caps.caps_start() & (bottom > 0.0);
        let end = caps.caps_end() & (top > 0.0);
        // An absent cap contributes no part at all — `Option` is an iterator of
        // zero or one elements, so cap selection is a chain, not a branch.
        let parts = core::iter::once(side_streams(bottom, top, half, n))
            .chain(start.then(|| disc_streams(bottom, -half, n, false)))
            .chain(end.then(|| disc_streams(top, half, n, true)));
        Mesh::from_streams(parts.fold(MeshStreams::default(), append))
    })
}

/// Radii must be non-negative with at least one of them real, and the surface
/// must have height. `Meters` is finite by construction, so finiteness is the
/// kernel's guarantee and is not re-checked here.
fn validate(bottom: f32, top: f32, half_height: f32) -> MeshResult<()> {
    ((bottom >= 0.0) & (top >= 0.0) & (bottom + top > 0.0) & (half_height > 0.0))
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a frustum needs a positive half-height and non-negative radii, at least one of them positive",
            )
        })
}

/// Concatenate `add` onto `base`, rebasing its indices onto the appended
/// vertices. Every part this module builds carries positions, normals and uvs,
/// so the streams stay aligned.
fn append(mut base: MeshStreams, add: MeshStreams) -> MeshStreams {
    let offset = base.positions.len() as u32;
    base.indices.extend(add.indices.iter().map(|i| i + offset));
    base.positions.extend(add.positions);
    base.normals.extend(add.normals);
    base.uvs.extend(add.uvs);
    base
}

/// The ruled side wall: two rings of `segments + 1` vertices (the last
/// duplicating the first so `u` reaches `1.0`), stitched into quads.
///
/// A ring of zero radius collapses to a point, so the triangle that would span
/// it is dropped — that is what turns this surface into a cone's apex fan.
fn side_streams(bottom: f32, top: f32, half_height: f32, n: u32) -> MeshStreams {
    let cols = n + 1;
    let radii = [bottom, top];
    let levels = [-half_height, half_height];
    // The slant normal, constant along a meridian: horizontal magnitude `2h`,
    // vertical component `bottom - top`, normalized.
    let horizontal = 2.0 * half_height;
    let vertical = bottom - top;
    let inv = 1.0 / (horizontal * horizontal + vertical * vertical).sqrt();

    let positions = (0..2usize)
        .flat_map(|row| {
            (0..cols).map(move |j| {
                let a = angle(j, n);
                Vec3::new(radii[row] * a.cos(), levels[row], radii[row] * a.sin())
            })
        })
        .collect();
    let normals = (0..2usize)
        .flat_map(|_| {
            (0..cols).map(move |j| {
                let a = angle(j, n);
                Vec3::new(
                    horizontal * a.cos() * inv,
                    vertical * inv,
                    horizontal * a.sin() * inv,
                )
            })
        })
        .collect();
    let uvs = (0..2usize)
        .flat_map(|row| (0..cols).map(move |j| Vec2::new(j as f32 / n as f32, row as f32)))
        .collect();
    let indices = (0..n)
        .flat_map(|j| {
            let (a, b) = (j, j + 1);
            let (c, d) = (cols + j, cols + j + 1);
            (bottom > 0.0)
                .then_some([a, c, b])
                .into_iter()
                .chain((top > 0.0).then_some([b, c, d]))
        })
        .flatten()
        .collect();

    MeshStreams {
        normals,
        uvs,
        ..MeshStreams::new(positions, indices)
    }
}

/// One end disc: a triangle fan of `segments` rim vertices around a centre
/// vertex, all sharing the flat `±Y` normal. `up` selects the `+Y` disc, which
/// also reverses the fan so it stays counter-clockwise seen from outside.
fn disc_streams(radius: f32, y: f32, n: u32, up: bool) -> MeshStreams {
    let sign = [-1.0f32, 1.0][usize::from(up)];
    let positions = (0..n)
        .map(|j| {
            let a = angle(j, n);
            Vec3::new(radius * a.cos(), y, radius * a.sin())
        })
        .chain(core::iter::once(Vec3::new(0.0, y, 0.0)))
        .collect();
    let normals = vec![Vec3::new(0.0, sign, 0.0); n as usize + 1];
    let uvs = (0..n)
        .map(|j| {
            let a = angle(j, n);
            Vec2::new(0.5 + 0.5 * a.cos() * sign, 0.5 + 0.5 * a.sin())
        })
        .chain(core::iter::once(Vec2::new(0.5, 0.5)))
        .collect();
    let indices = (0..n)
        .flat_map(|j| {
            let (p, q) = (j, (j + 1) % n);
            [[n, p, q], [n, q, p]][usize::from(up)]
        })
        .collect();

    MeshStreams {
        normals,
        uvs,
        ..MeshStreams::new(positions, indices)
    }
}

/// The longitude angle of column `j` of `n` segments.
fn angle(j: u32, n: u32) -> f32 {
    TAU * j as f32 / n as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: f32) -> Meters {
        Meters::finite_or_zero(v)
    }

    fn seg(v: u32) -> Segments {
        Segments::new(v).unwrap()
    }

    /// Every triangle is counter-clockwise when seen from outside: its
    /// geometric normal agrees with the authored outward normal at its centroid.
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
    fn a_frustum_has_two_rings_two_caps_and_a_duplicated_seam() {
        let f = frustum(m(2.0), m(1.0), m(3.0), seg(8), CapPolicy::Both).unwrap();
        // 2 side rings of 9 (seam duplicated) + 2 caps of 8 rim + 1 centre.
        assert_eq!(f.vertex_count(), 2 * 9 + 2 * 9);
        assert_eq!(f.triangle_count(), 2 * 8 + 2 * 8);
        assert!(f.has_normals() & f.has_uvs());
        assert!(f.tangents().is_empty() & f.colors().is_empty());
        assert_ccw_outward(&f);
        // The seam reaches both ends of the u range.
        assert_eq!(f.uvs()[0], Vec2::new(0.0, 0.0));
        assert_eq!(f.uvs()[8], Vec2::new(1.0, 0.0));
        assert_eq!(f.uvs()[9], Vec2::new(0.0, 1.0));
        assert_eq!(f.uvs()[17], Vec2::new(1.0, 1.0));
    }

    #[test]
    fn the_rings_sit_at_their_radii_and_heights() {
        let f = frustum(m(2.0), m(1.0), m(3.0), seg(8), CapPolicy::None).unwrap();
        for (i, p) in f.positions().iter().enumerate() {
            let expected = if i < 9 { (2.0, -3.0) } else { (1.0, 3.0) };
            let radial = (p.x * p.x + p.z * p.z).sqrt();
            assert!((radial - expected.0).abs() < 1.0e-4, "vertex {i} radius");
            assert!((p.y - expected.1).abs() < 1.0e-4, "vertex {i} height");
        }
    }

    #[test]
    fn a_tapered_side_normal_leans_and_stays_unit_length() {
        let f = frustum(m(2.0), m(1.0), m(1.0), seg(8), CapPolicy::None).unwrap();
        // horizontal = 2h = 2, vertical = 2 - 1 = 1 -> n ~ (2cos, 1, 2sin)/sqrt(5).
        let expect_y = 1.0 / 5.0f32.sqrt();
        for n in f.normals() {
            assert!((n.length() - 1.0).abs() < 1.0e-5);
            assert!((n.y - expect_y).abs() < 1.0e-5, "slant normal y {}", n.y);
        }
    }

    #[test]
    fn caps_are_flat_discs_facing_out_along_the_axis() {
        let f = frustum(m(2.0), m(1.0), m(3.0), seg(8), CapPolicy::Both).unwrap();
        let normals = f.normals();
        // Cap vertices follow the 18 side vertices: 9 bottom then 9 top.
        assert_eq!(normals[18], Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(normals[26], Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(normals[27], Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(normals[35], Vec3::new(0.0, 1.0, 0.0));
        // The cap centres are on the axis at the ring heights.
        assert_eq!(f.positions()[26], Vec3::new(0.0, -3.0, 0.0));
        assert_eq!(f.positions()[35], Vec3::new(0.0, 3.0, 0.0));
        assert_eq!(f.uvs()[26], Vec2::new(0.5, 0.5));
    }

    #[test]
    fn each_cap_policy_selects_its_own_discs() {
        let counts = [
            CapPolicy::None,
            CapPolicy::Start,
            CapPolicy::End,
            CapPolicy::Both,
        ]
        .map(|c| {
            let f = frustum(m(2.0), m(1.0), m(3.0), seg(8), c).unwrap();
            assert_ccw_outward(&f);
            (f.vertex_count(), f.triangle_count())
        });
        assert_eq!(counts, [(18, 16), (27, 24), (27, 24), (36, 32)]);
    }

    #[test]
    fn a_zero_top_radius_is_a_cone_whose_end_cap_is_dropped() {
        let f = frustum(m(2.0), m(0.0), m(3.0), seg(8), CapPolicy::Both).unwrap();
        // The apex ring still carries 9 vertices (distinct normals and u), but
        // the degenerate half of every side quad and the whole top disc are gone.
        assert_eq!(f.vertex_count(), 18 + 9);
        assert_eq!(f.triangle_count(), 8 + 8);
        assert_ccw_outward(&f);
        assert!(f.positions()[9..18]
            .iter()
            .all(|p| (p.x.abs() < 1.0e-6) & (p.z.abs() < 1.0e-6)));
    }

    #[test]
    fn a_zero_bottom_radius_is_an_upside_down_cone() {
        let f = frustum(m(0.0), m(2.0), m(3.0), seg(8), CapPolicy::Both).unwrap();
        assert_eq!(f.vertex_count(), 18 + 9);
        assert_eq!(f.triangle_count(), 8 + 8);
        assert_ccw_outward(&f);
    }

    #[test]
    fn a_zero_height_frustum_is_rejected() {
        assert_eq!(
            frustum(m(1.0), m(1.0), m(0.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_negative_half_height_is_rejected() {
        assert_eq!(
            frustum(m(1.0), m(1.0), m(-1.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_negative_radius_is_rejected_at_either_end() {
        assert_eq!(
            frustum(m(-1.0), m(1.0), m(1.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            frustum(m(1.0), m(-1.0), m(1.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn two_zero_radii_describe_a_line_and_are_rejected() {
        assert_eq!(
            frustum(m(0.0), m(0.0), m(1.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn generation_is_deterministic() {
        let a = frustum(m(2.0), m(1.0), m(3.0), seg(12), CapPolicy::Both).unwrap();
        let b = frustum(m(2.0), m(1.0), m(3.0), seg(12), CapPolicy::Both).unwrap();
        assert_eq!(a, b);
    }
}
