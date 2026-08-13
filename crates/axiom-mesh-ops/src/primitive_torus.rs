//! The torus about `+Y`: a circular tube swept around a circular spine.

use core::f32::consts::{FRAC_PI_2, TAU};

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

use crate::tessellation::Segments;

/// A torus about the `+Y` axis, centred on the origin.
///
/// The spine is the circle of `major_radius` in the `XZ` plane; the surface is
/// every point exactly `minor_radius` from it. `major_segments` divides the
/// sweep around the axis, `minor_segments` divides the tube's own cross-section.
///
/// Vertices form a `(major_segments + 1) x (minor_segments + 1)` grid — **both**
/// seams are duplicated, so `u` and `v` each reach `0.0` and `1.0` and neither
/// wrap is collapsed. `u` wraps once around the `+Y` axis; `v` wraps once around
/// the tube, starting at its bottom (`-Y`-most point, `v = 0`), passing the
/// outer equator at `v = 0.25` and the top at `v = 0.5`. `v` cannot be monotone
/// bottom-to-top on a torus — the surface visits every height twice — so the
/// convention is "starts at the bottom, reaches the top halfway".
///
/// Normals point away from the spine: `(position - nearest spine point) /
/// minor_radius`.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when `minor_radius` is not strictly
/// positive, or when it is not strictly less than `major_radius` — at
/// `minor >= major` the tube swallows the axis and the surface intersects
/// itself, which is not a torus.
pub fn torus(
    major_radius: Meters,
    minor_radius: Meters,
    major_segments: Segments,
    minor_segments: Segments,
) -> MeshResult<Mesh> {
    let (major, minor) = (major_radius.get(), minor_radius.get());
    ((minor > 0.0) & (minor < major))
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a torus needs a strictly positive minor radius that is strictly less than its major radius",
            )
        })
        .and_then(|()| {
            let (majors, minors) = (major_segments.get(), minor_segments.get());
            let cols = minors + 1;
            let normals: Vec<Vec3> = (0..=majors)
                .flat_map(|i| (0..=minors).map(move |j| normal(i, j, majors, minors)))
                .collect();
            let positions = (0..=majors)
                .flat_map(|i| {
                    let theta = TAU * i as f32 / majors as f32;
                    let spine = Vec3::new(major * theta.cos(), 0.0, major * theta.sin());
                    (0..=minors).map(move |_| spine)
                })
                .zip(&normals)
                .map(|(spine, n)| spine.add(n.mul_scalar(minor)))
                .collect();
            let uvs = (0..=majors)
                .flat_map(|i| {
                    (0..=minors)
                        .map(move |j| Vec2::new(i as f32 / majors as f32, j as f32 / minors as f32))
                })
                .collect();
            Mesh::from_streams(MeshStreams {
                normals,
                uvs,
                ..MeshStreams::new(positions, grid_indices(majors, minors, cols))
            })
        })
}

/// The outward unit normal at grid vertex `(i, j)`: the direction from the
/// spine point at major angle `theta` to the surface point at minor angle `phi`.
///
/// `phi` is offset by a quarter turn so `v = 0` lands on the bottom of the tube.
fn normal(i: u32, j: u32, majors: u32, minors: u32) -> Vec3 {
    let theta = TAU * i as f32 / majors as f32;
    let phi = TAU * j as f32 / minors as f32 - FRAC_PI_2;
    Vec3::new(
        phi.cos() * theta.cos(),
        phi.sin(),
        phi.cos() * theta.sin(),
    )
}

/// Stitch the doubly-wrapped grid into quads, counter-clockwise seen from
/// outside. No band is degenerate: a torus has no pole.
fn grid_indices(majors: u32, minors: u32, cols: u32) -> Vec<u32> {
    (0..majors)
        .flat_map(move |i| {
            (0..minors).flat_map(move |j| {
                let (a, b) = (i * cols + j, i * cols + j + 1);
                let (c, d) = (a + cols, b + cols);
                [a, b, c, c, b, d]
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: f32) -> Meters {
        Meters::finite_or_zero(v)
    }

    fn build(major: f32, minor: f32, majors: u32, minors: u32) -> Mesh {
        torus(
            m(major),
            m(minor),
            Segments::new(majors).unwrap(),
            Segments::new(minors).unwrap(),
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

    /// The distance from `p` to the spine circle of `major` radius in `XZ`.
    fn spine_distance(p: Vec3, major: f32) -> f32 {
        let horizontal = (p.x * p.x + p.z * p.z).sqrt();
        ((horizontal - major).powi(2) + p.y * p.y).sqrt()
    }

    #[test]
    fn an_eight_by_six_torus_has_exact_counts() {
        let t = build(3.0, 1.0, 8, 6);
        // Both seams duplicated: (major + 1) * (minor + 1).
        assert_eq!(t.vertex_count(), 9 * 7);
        assert_eq!(t.triangle_count(), 2 * 8 * 6);
        assert!(t.has_normals() & t.has_uvs());
        assert!(t.tangents().is_empty() & t.colors().is_empty());
        assert_ccw_outward(&t);
    }

    #[test]
    fn every_vertex_is_exactly_the_minor_radius_from_the_spine() {
        let t = build(4.0, 1.5, 16, 12);
        for (i, p) in t.positions().iter().enumerate() {
            let d = spine_distance(*p, 4.0);
            assert!((d - 1.5).abs() < 1.0e-4, "vertex {i} is {d} from the spine");
        }
    }

    #[test]
    fn normals_point_away_from_the_tube_centre_circle() {
        let t = build(4.0, 1.5, 16, 12);
        for (p, n) in t.positions().iter().zip(t.normals()) {
            let horizontal = (p.x * p.x + p.z * p.z).sqrt();
            let spine = Vec3::new(p.x * 4.0 / horizontal, 0.0, p.z * 4.0 / horizontal);
            let outward = p.subtract(spine).normalize().unwrap();
            assert!(n.subtract(outward).length() < 1.0e-4);
            assert!((n.length() - 1.0).abs() < 1.0e-5);
        }
    }

    #[test]
    fn the_extents_follow_the_two_radii() {
        let t = build(3.0, 1.0, 24, 16);
        let highest = t.positions().iter().map(|p| p.y).fold(f32::MIN, f32::max);
        let widest = t
            .positions()
            .iter()
            .map(|p| (p.x * p.x + p.z * p.z).sqrt())
            .fold(0.0f32, f32::max);
        let narrowest = t
            .positions()
            .iter()
            .map(|p| (p.x * p.x + p.z * p.z).sqrt())
            .fold(f32::MAX, f32::min);
        assert!((highest - 1.0).abs() < 1.0e-4);
        assert!((widest - 4.0).abs() < 1.0e-4);
        assert!((narrowest - 2.0).abs() < 1.0e-4);
    }

    #[test]
    fn v_starts_at_the_bottom_of_the_tube_and_reaches_the_top_halfway() {
        let t = build(3.0, 1.0, 8, 4);
        // Column j of row 0: v = j / 4, at the bottom, outer, top, inner points.
        let column = |j: usize| t.positions()[j];
        assert!((column(0).y + 1.0).abs() < 1.0e-5, "v=0 is not the bottom");
        assert!((column(1).x - 4.0).abs() < 1.0e-5, "v=0.25 is not the outside");
        assert!((column(2).y - 1.0).abs() < 1.0e-5, "v=0.5 is not the top");
        assert!((column(3).x - 2.0).abs() < 1.0e-5, "v=0.75 is not the inside");
        assert_eq!(t.uvs()[0], Vec2::new(0.0, 0.0));
        assert_eq!(t.uvs()[2], Vec2::new(0.0, 0.5));
    }

    #[test]
    fn both_seams_are_duplicated() {
        let t = build(3.0, 1.0, 8, 6);
        let cols = 7usize;
        // The minor seam: last column of a row repeats its first column.
        assert!(t.positions()[0].distance(t.positions()[6]) < 1.0e-5);
        assert_eq!(t.uvs()[0].y, 0.0);
        assert_eq!(t.uvs()[6].y, 1.0);
        // The major seam: the last row repeats the first.
        assert!(t.positions()[0].distance(t.positions()[8 * cols]).abs() < 1.0e-5);
        assert_eq!(t.uvs()[0].x, 0.0);
        assert_eq!(t.uvs()[8 * cols].x, 1.0);
    }

    #[test]
    fn a_non_positive_minor_radius_is_rejected() {
        let s = Segments::new(8).unwrap();
        assert_eq!(
            torus(m(3.0), m(0.0), s, s).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            torus(m(3.0), m(-1.0), s, s).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }

    /// At `minor >= major` the tube reaches the axis and the surface would
    /// intersect itself, so the shape is refused rather than silently emitted.
    #[test]
    fn a_minor_radius_at_or_beyond_the_major_is_rejected() {
        let s = Segments::new(8).unwrap();
        assert_eq!(
            torus(m(2.0), m(2.0), s, s).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            torus(m(2.0), m(3.0), s, s).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            torus(m(-2.0), m(1.0), s, s).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(build(3.0, 1.0, 12, 8), build(3.0, 1.0, 12, 8));
    }
}
