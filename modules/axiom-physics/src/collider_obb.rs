//! Lifting a box collider into the math layer's [`Obb`], and unfolding that box
//! into the twelve triangles of its surface.
//!
//! Two conversions, one subject: where a box collider actually is in world
//! space. The [`Obb`] answers the point/ray questions directly; the triangle
//! decomposition is what lets a *capsule* be tested against a box at all, since
//! the math layer's exact capsule solves ([`axiom_math::Segment::closest_points_to_triangle`],
//! [`axiom_math::Capsule::overlaps_triangle`], [`axiom_math::Capsule::sweep_triangle`])
//! are written against triangles.
//!
//! ## Why triangles rather than a bespoke segment/box solve
//! The closest point between a segment and a box is realized on the box's
//! surface — on a face, an edge, or a vertex — in every configuration where the
//! segment is outside it. The twelve surface triangles carry exactly those
//! features, so the minimum over them is the *exact* answer, reusing solves that
//! are already proven and fully covered in `axiom-math` rather than open-coding
//! a second, unproven one here.

use axiom_math::{Obb, Quat, Triangle, Vec3};

use crate::physics_collider_shape::PhysicsColliderShape;

/// The eight corners of a box, indexed by the bits of their sign pattern:
/// bit 0 is the X sign, bit 1 the Y sign, bit 2 the Z sign (`0` = negative).
const CORNER_COUNT: usize = 8;

/// The six faces, each as four corner indices wound counter-clockwise seen from
/// **outside** the box, so `(a, b, c)` and `(a, c, d)` both carry the face's
/// outward normal. Order: `+X, -X, +Y, -Y, +Z, -Z`.
const FACES: [[usize; 4]; 6] = [
    [1, 3, 7, 5],
    [0, 4, 6, 2],
    [2, 6, 7, 3],
    [0, 1, 5, 4],
    [4, 5, 7, 6],
    [0, 2, 3, 1],
];

/// The world-space [`Obb`] of a box collider on a body at `center` with rotation
/// `rotation`. `None` only for geometry the math layer refuses (a non-finite
/// centre or an un-normalizable rotation) — states a validated collider on a
/// validated body cannot reach.
pub(crate) fn world_obb(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
) -> Option<Obb> {
    Obb::new(center, shape.half_extents(), rotation).ok()
}

/// The eight world corners of `obb`, in sign-bit order.
fn corners(obb: &Obb) -> [Vec3; CORNER_COUNT] {
    let he = obb.half_extents();
    let (center, orientation) = (obb.center(), obb.orientation());
    core::array::from_fn(|i| {
        let signs = [
            [-1.0, 1.0][i & 1],
            [-1.0, 1.0][(i >> 1) & 1],
            [-1.0, 1.0][(i >> 2) & 1],
        ];
        center.add(orientation.rotate(Vec3::new(
            he.x * signs[0],
            he.y * signs[1],
            he.z * signs[2],
        )))
    })
}

/// The twelve triangles of `obb`'s surface, two per face, wound outward.
///
/// A `Vec` rather than a `[Triangle; 12]` because [`Triangle::new`] is fallible
/// and there is no valid triangle to substitute for one it rejects: a corner the
/// math layer refuses is simply absent from the surface rather than replaced by
/// a fabricated one. For a validated box (finite centre, finite extents) all
/// twelve are always present.
pub(crate) fn obb_triangles(obb: &Obb) -> Vec<Triangle> {
    let corners = corners(obb);
    FACES
        .into_iter()
        .flat_map(|face| {
            [[face[0], face[1], face[2]], [face[0], face[2], face[3]]]
                .into_iter()
                .filter_map(move |tri| {
                    Triangle::new(corners[tri[0]], corners[tri[1]], corners[tri[2]]).ok()
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::FRAC_PI_2;

    fn box_shape(x: f32, y: f32, z: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::box_shape(Vec3::new(x, y, z)).unwrap()
    }

    fn unit_obb() -> Obb {
        world_obb(box_shape(1.0, 1.0, 1.0), Vec3::ZERO, Quat::IDENTITY).unwrap()
    }

    #[test]
    fn a_box_collider_lifts_to_its_oriented_bounding_box() {
        let turned = Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_2).unwrap();
        let obb = world_obb(box_shape(4.0, 1.0, 1.0), Vec3::new(0.0, 2.0, 0.0), turned)
            .expect("a validated box lifts");
        assert_eq!(obb.center(), Vec3::new(0.0, 2.0, 0.0));
        assert_eq!(obb.half_extents(), Vec3::new(4.0, 1.0, 1.0));
        // The long axis has turned onto world Z.
        assert!(obb.contains_point(Vec3::new(0.0, 2.0, 3.5)));
        assert!(!obb.contains_point(Vec3::new(3.5, 2.0, 0.0)));
    }

    #[test]
    fn a_non_finite_centre_lifts_to_nothing() {
        assert!(world_obb(
            box_shape(1.0, 1.0, 1.0),
            Vec3::new(0.0, f32::INFINITY, 0.0),
            Quat::IDENTITY
        )
        .is_none());
    }

    #[test]
    fn the_surface_is_twelve_triangles_whose_normals_all_point_outward() {
        let obb = unit_obb();
        let tris = obb_triangles(&obb);
        assert_eq!(tris.len(), 12);
        assert!(
            tris.iter().all(|t| {
                let n = t.normal().expect("a box face is never degenerate");
                let centroid = t
                    .a()
                    .add(t.b())
                    .add(t.c())
                    .mul_scalar(1.0 / 3.0)
                    .subtract(obb.center());
                n.dot(centroid) > 0.0
            }),
            "every surface triangle must be wound so its normal faces away from the centre"
        );
    }

    #[test]
    fn every_corner_of_the_box_appears_on_its_surface() {
        let obb = unit_obb();
        let tris = obb_triangles(&obb);
        let corners = corners(&obb);
        assert!(
            corners.iter().all(|corner| tris
                .iter()
                .any(|t| [t.a(), t.b(), t.c()].iter().any(|v| v == corner))),
            "the decomposition must use all eight corners"
        );
        // And each corner is one of the eight sign patterns of the half-extents.
        assert!(corners
            .iter()
            .all(|c| (c.x.abs() == 1.0) & (c.y.abs() == 1.0) & (c.z.abs() == 1.0)));
    }

    #[test]
    fn a_rotated_box_carries_its_surface_with_it() {
        let turned = Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_2).unwrap();
        let obb = world_obb(box_shape(4.0, 1.0, 1.0), Vec3::ZERO, turned).unwrap();
        let tris = obb_triangles(&obb);
        // The surface now reaches |z| = 4 and only |x| = 1.
        assert!(tris
            .iter()
            .flat_map(|t| [t.a(), t.b(), t.c()])
            .all(|v| (v.x.abs() < 1.001) & (v.z.abs() < 4.001)));
        assert!(tris
            .iter()
            .flat_map(|t| [t.a(), t.b(), t.c()])
            .any(|v| v.z.abs() > 3.999));
    }
}
