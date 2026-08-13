//! Bounds derived from a mesh's position stream.
//!
//! Both bounds are pure functions of `positions` in index order, so replaying a
//! mesh reproduces them bit-for-bit. Neither reads topology: a vertex that no
//! triangle references still counts, because bounds answer "where is this data",
//! not "where is this surface".

use axiom_math::{Aabb, MathError, Sphere};

use crate::mesh::Mesh;
use crate::mesh_error::MeshError;
use crate::mesh_error_code::MeshErrorCode;
use crate::mesh_result::MeshResult;

/// Translate a math-layer bounds rejection into this layer's vocabulary.
///
/// The `Mesh` invariant (non-empty, finite positions) already rules this out for
/// [`aabb`]: a component-wise envelope of finite points is finite and ordered.
/// It is genuinely reachable from [`bounding_sphere`], where positions near
/// `f32::MAX` overflow the box centre's `(min + max)` sum to infinity, and the
/// sphere then has no representable centre.
fn invalid_bounds(_cause: MathError) -> MeshError {
    MeshError::new(
        MeshErrorCode::InvalidParameter,
        "the mesh's positions do not describe representable bounds",
    )
}

/// The tight axis-aligned bounding box of every vertex position.
///
/// Seeded with the first position — which always exists — and grown one point at
/// a time, so a single-vertex mesh yields a degenerate box whose `min` equals its
/// `max` rather than an error.
pub fn aabb(mesh: &Mesh) -> MeshResult<Aabb> {
    let first = mesh.positions()[0];
    Aabb::new(first, first)
        .map_err(invalid_bounds)
        .map(|seed| {
            mesh.positions()
                .iter()
                .copied()
                .fold(seed, |box_, p| box_.expand(p))
        })
}

/// A bounding sphere centred on the [`aabb`] centre, with the radius that just
/// reaches the furthest position.
///
/// **This is not the minimal enclosing sphere.** Ritter's algorithm and Welzl's
/// exact solution both produce a smaller sphere for most inputs; this one can be
/// up to `sqrt(3)` times too large in the worst case (a single point in the
/// corner of a cubic box). It is chosen deliberately: it is a closed-form,
/// order-independent function of the positions, so it is deterministic and
/// replay-stable, and it is tight enough for the culling and broad-phase uses a
/// derived bound serves. A caller who needs the minimal sphere should compute
/// one; this layer will not silently trade determinism for tightness.
pub fn bounding_sphere(mesh: &Mesh) -> MeshResult<Sphere> {
    aabb(mesh).and_then(|box_| {
        let center = box_.center();
        let radius = mesh
            .positions()
            .iter()
            .copied()
            .fold(0.0_f32, |furthest, p| furthest.max(p.distance(center)));
        Sphere::new(center, radius).map_err(invalid_bounds)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_streams::MeshStreams;
    use axiom_math::{ApproxEq, Epsilon, Vec3};

    fn mesh_of(positions: Vec<Vec3>) -> Mesh {
        Mesh::from_streams(MeshStreams::new(positions, Vec::new())).unwrap()
    }

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    #[test]
    fn the_box_is_the_component_wise_envelope_of_every_position() {
        let m = mesh_of(vec![
            Vec3::new(-1.0, 4.0, 0.5),
            Vec3::new(3.0, -2.0, 0.5),
            Vec3::new(0.0, 0.0, -7.0),
        ]);
        let b = aabb(&m).unwrap();
        assert_eq!(b.min(), Vec3::new(-1.0, -2.0, -7.0));
        assert_eq!(b.max(), Vec3::new(3.0, 4.0, 0.5));
    }

    #[test]
    fn a_single_vertex_yields_a_degenerate_box_at_that_point() {
        let b = aabb(&mesh_of(vec![Vec3::new(2.0, 3.0, 4.0)])).unwrap();
        assert_eq!(b.min(), Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(b.max(), Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(b.center(), Vec3::new(2.0, 3.0, 4.0));
    }

    #[test]
    fn unreferenced_vertices_still_widen_the_box() {
        // Index buffer names only vertex 0..3; the far vertex 3 is orphaned, and
        // must still be bounded — bounds describe the data, not the surface.
        let m = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::ZERO,
                Vec3::UNIT_X,
                Vec3::UNIT_Y,
                Vec3::new(0.0, 0.0, 10.0),
            ],
            vec![0, 1, 2],
        ))
        .unwrap();
        assert_eq!(aabb(&m).unwrap().max(), Vec3::new(1.0, 1.0, 10.0));
    }

    #[test]
    fn the_sphere_is_centred_on_the_box_and_reaches_the_furthest_corner() {
        let m = mesh_of(vec![
            Vec3::new(-1.0, -1.0, -1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::ZERO,
        ]);
        let s = bounding_sphere(&m).unwrap();
        assert!(s.center().approx_eq(&Vec3::ZERO, eps()));
        // The corners of the unit cube sit at sqrt(3) from its centre.
        assert!(s.radius().approx_eq(&3.0_f32.sqrt(), eps()));
        assert!(s.contains_point(Vec3::new(1.0, 1.0, 1.0)));
    }

    #[test]
    fn the_sphere_encloses_every_position_but_is_not_minimal() {
        // Seven cube corners plus the eighth: the box centre is the origin, so
        // the radius is the full half-diagonal even though a sphere centred on
        // the point cloud's centroid would be smaller.
        let corners: Vec<Vec3> = [-1.0_f32, 1.0]
            .into_iter()
            .flat_map(|x| {
                [-1.0_f32, 1.0].into_iter().flat_map(move |y| {
                    [-1.0_f32, 1.0].into_iter().map(move |z| Vec3::new(x, y, z))
                })
            })
            .collect();
        let s = bounding_sphere(&mesh_of(corners.clone())).unwrap();
        assert!(corners.iter().all(|&c| s.contains_point(c)));

        // A flat square in the XZ plane: the tight sphere has radius sqrt(2),
        // and this construction returns exactly that — tight here, and never
        // smaller than the true bound anywhere.
        let square = mesh_of(vec![
            Vec3::new(-1.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(-1.0, 0.0, 1.0),
        ]);
        let flat = bounding_sphere(&square).unwrap();
        assert!(flat.radius().approx_eq(&2.0_f32.sqrt(), eps()));
    }

    #[test]
    fn a_single_vertex_yields_a_zero_radius_sphere() {
        let s = bounding_sphere(&mesh_of(vec![Vec3::new(5.0, 0.0, 0.0)])).unwrap();
        assert_eq!(s.radius(), 0.0);
        assert_eq!(s.center(), Vec3::new(5.0, 0.0, 0.0));
    }

    #[test]
    fn positions_whose_box_centre_overflows_report_unrepresentable_bounds() {
        // `3.0e38 + 3.0e38` exceeds `f32::MAX`, so the box centre — and with it
        // the sphere — has no finite representation. The box itself is fine.
        let m = mesh_of(vec![Vec3::new(3.0e38, 0.0, 0.0)]);
        assert!(aabb(&m).is_ok());
        assert_eq!(
            bounding_sphere(&m).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }
}
