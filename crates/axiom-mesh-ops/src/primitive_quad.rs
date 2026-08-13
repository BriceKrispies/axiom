//! The flat quad primitive: one rectangle in the XZ plane, facing `+Y`.
//!
//! This is the ground plane, the sprite card, and the floor tile — the shape
//! every other flat generator is a refinement of. It is deliberately the
//! two-triangle case with no subdivision vocabulary; a caller that wants
//! interior vertices asks [`crate::grid`] instead.

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

/// A rectangle centred on the origin in the XZ plane, with `+Y` normals.
///
/// Vertices are ordered `(-x,-z)`, `(+x,-z)`, `(-x,+z)`, `(+x,+z)` — the same
/// row-major order [`crate::grid`] uses — and carry the unit-square UVs, `u`
/// increasing with `+X` and `v` with `+Z`. Both triangles are counter-clockwise
/// seen from `+Y`.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when either half-extent is zero or
/// negative. A zero extent would collapse the rectangle to a line, which has no
/// front face to wind.
pub fn quad(half_width: Meters, half_depth: Meters) -> MeshResult<Mesh> {
    let (w, d) = (half_width.get(), half_depth.get());
    ((w > 0.0) & (d > 0.0))
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a quad needs strictly positive half-extents",
            )
        })
        .and_then(|()| {
            Mesh::from_streams(MeshStreams {
                normals: vec![Vec3::UNIT_Y; 4],
                uvs: vec![
                    Vec2::new(0.0, 0.0),
                    Vec2::new(1.0, 0.0),
                    Vec2::new(0.0, 1.0),
                    Vec2::new(1.0, 1.0),
                ],
                ..MeshStreams::new(
                    vec![
                        Vec3::new(-w, 0.0, -d),
                        Vec3::new(w, 0.0, -d),
                        Vec3::new(-w, 0.0, d),
                        Vec3::new(w, 0.0, d),
                    ],
                    vec![0, 2, 3, 0, 3, 1],
                )
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn built() -> Mesh {
        quad(meters(2.0), meters(0.5)).unwrap()
    }

    #[test]
    fn a_quad_is_two_triangles_over_four_corners() {
        let m = built();
        assert_eq!(m.vertex_count(), 4);
        assert_eq!(m.triangle_count(), 2);
        assert_eq!(
            m.positions(),
            &[
                Vec3::new(-2.0, 0.0, -0.5),
                Vec3::new(2.0, 0.0, -0.5),
                Vec3::new(-2.0, 0.0, 0.5),
                Vec3::new(2.0, 0.0, 0.5),
            ]
        );
    }

    #[test]
    fn every_normal_is_exactly_up() {
        assert_eq!(built().normals(), &[Vec3::UNIT_Y; 4]);
    }

    #[test]
    fn the_uvs_are_the_unit_square_corners() {
        assert_eq!(
            built().uvs(),
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0),
            ]
        );
    }

    #[test]
    fn both_triangles_wind_counter_clockwise_about_the_normal() {
        let m = built();
        let p = m.positions();
        for t in m.indices().chunks(3) {
            let (a, b, c) = (
                p[t[0] as usize],
                p[t[1] as usize],
                p[t[2] as usize],
            );
            assert!(b.subtract(a).cross(c.subtract(a)).dot(Vec3::UNIT_Y) > 0.0);
        }
    }

    #[test]
    fn non_positive_half_extents_are_rejected() {
        for (w, d) in [(0.0, 1.0), (1.0, 0.0), (-1.0, 1.0), (1.0, -1.0)] {
            assert_eq!(
                quad(meters(w), meters(d)).unwrap_err().code(),
                MeshErrorCode::InvalidParameter
            );
        }
    }
}
