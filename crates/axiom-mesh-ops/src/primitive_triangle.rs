//! The single-triangle primitive: three explicit corners into a one-triangle
//! mesh.
//!
//! This is the smallest thing the layer can generate, and it is the reference
//! statement of the engine's winding contract: the triangle is emitted exactly
//! as given, so `(b - a).cross(c - a)` *is* its outward normal. A caller that
//! wants the other facing passes the corners in the other order.

use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

/// How large `(b - a) × (c - a)` must be for the three corners to count as a
/// genuine triangle. The cross product's magnitude is twice the triangle's
/// area, so this is a floor on area rather than on edge length: three points a
/// metre apart but collinear to within float noise are rejected, which is
/// exactly the case that would otherwise produce an un-normalizable normal.
const AREA_EPSILON: f32 = 1.0e-12;

/// The corner texture coordinates: the natural affine parameterization of a bare
/// triangle, with `a` at the UV origin and `b`/`c` at the two unit axes.
const CORNER_UVS: [Vec2; 3] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(1.0, 0.0),
    Vec2::new(0.0, 1.0),
];

/// A single triangle through the corners `a`, `b`, `c`, in that order.
///
/// The corners are used verbatim — this generator does not re-order them — so
/// the mesh is front-facing when `(a, b, c)` is counter-clockwise as seen from
/// the side the surface faces. All three vertices carry the one geometric
/// normal, and the UVs are `(0,0)`, `(1,0)`, `(0,1)`.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when any corner has a non-finite
/// component, and [`MeshErrorCode::DegenerateTriangle`] when the three corners
/// are coincident or collinear, so no normal exists.
pub fn triangle(a: Vec3, b: Vec3, c: Vec3) -> MeshResult<Mesh> {
    let corners = [a, b, c];
    corners
        .iter()
        .all(|p| finite(*p))
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "every triangle corner must be finite",
            )
        })
        .and_then(|()| normal_of(a, b, c))
        .and_then(|normal| {
            Mesh::from_streams(MeshStreams {
                normals: vec![normal; 3],
                uvs: CORNER_UVS.to_vec(),
                ..MeshStreams::new(corners.to_vec(), vec![0, 1, 2])
            })
        })
}

const fn finite(p: Vec3) -> bool {
    p.x.is_finite() & p.y.is_finite() & p.z.is_finite()
}

/// The unit outward normal, or [`MeshErrorCode::DegenerateTriangle`].
///
/// Divides by the measured length rather than calling `normalize`, because the
/// length has already been tested against [`AREA_EPSILON`]: routing through a
/// fallible normalize would add an error arm no input can reach.
fn normal_of(a: Vec3, b: Vec3, c: Vec3) -> MeshResult<Vec3> {
    let cross = b.subtract(a).cross(c.subtract(a));
    let length = cross.length();
    (length > AREA_EPSILON)
        .then_some(length)
        .map(|length| cross.mul_scalar(1.0 / length))
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::DegenerateTriangle,
                "a triangle's three corners must not be coincident or collinear",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_triangle_keeps_its_corners_and_carries_one_normal() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(2.0, 0.0, 0.0);
        let c = Vec3::new(0.0, 0.0, -2.0);
        let m = triangle(a, b, c).unwrap();

        assert_eq!(m.vertex_count(), 3);
        assert_eq!(m.triangle_count(), 1);
        assert_eq!(m.positions(), &[a, b, c]);
        assert_eq!(m.indices(), &[0, 1, 2]);
        // (b-a) x (c-a) = (2,0,0) x (0,0,-2) = (0, 4, 0) -> +Y.
        assert_eq!(m.normals(), &[Vec3::UNIT_Y; 3]);
        assert_eq!(
            m.uvs(),
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0)
            ]
        );
        assert!(m.tangents().is_empty());
        assert!(m.colors().is_empty());
    }

    #[test]
    fn reversing_two_corners_reverses_the_facing() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(2.0, 0.0, 0.0);
        let c = Vec3::new(0.0, 0.0, -2.0);
        let flipped = triangle(a, c, b).unwrap();
        assert_eq!(flipped.normals()[0], Vec3::new(0.0, -1.0, 0.0));
    }

    #[test]
    fn the_emitted_normal_is_unit_length_and_perpendicular_to_both_edges() {
        let a = Vec3::new(-1.0, 0.5, 2.0);
        let b = Vec3::new(3.0, -2.0, 0.25);
        let c = Vec3::new(0.0, 4.0, -1.5);
        let n = triangle(a, b, c).unwrap().normals()[0];
        assert!((n.length() - 1.0).abs() < 1.0e-5);
        assert!(n.dot(b.subtract(a)).abs() < 1.0e-5);
        assert!(n.dot(c.subtract(a)).abs() < 1.0e-5);
    }

    #[test]
    fn a_non_finite_corner_is_rejected() {
        let ok = Vec3::new(1.0, 0.0, 0.0);
        for bad in [
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::new(0.0, f32::INFINITY, 0.0),
            Vec3::new(0.0, 0.0, f32::NEG_INFINITY),
        ] {
            assert_eq!(
                triangle(bad, ok, Vec3::UNIT_Z).unwrap_err().code(),
                MeshErrorCode::InvalidParameter
            );
        }
    }

    #[test]
    fn collinear_corners_are_rejected() {
        assert_eq!(
            triangle(
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 0.0)
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::DegenerateTriangle
        );
    }

    #[test]
    fn coincident_corners_are_rejected() {
        assert_eq!(
            triangle(Vec3::ZERO, Vec3::ZERO, Vec3::UNIT_X)
                .unwrap_err()
                .code(),
            MeshErrorCode::DegenerateTriangle
        );
    }
}
