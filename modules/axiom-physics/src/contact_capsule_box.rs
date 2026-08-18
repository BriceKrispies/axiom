//! The capsule↔box narrow-phase pairing.
//!
//! A capsule touches a box when its axis segment comes within one radius of the
//! box's surface, so the pairing reduces to the closest pair of points between a
//! segment and an oriented box. That pair is found over the box's twelve surface
//! triangles (see [`crate::collider_obb`]), which carry every face, edge and
//! vertex the closest point can land on, using the math layer's exact
//! [`axiom_math::Segment::closest_points_to_triangle`] solve. The box's rotation
//! therefore genuinely orients it: a capsule resting on a tilted ramp contacts
//! the ramp's real face.
//!
//! ## The axis-inside-the-box degeneracy
//! A capsule whose axis has been driven *inside* the box has no defined
//! separating direction — the nearest surface point lies behind the axis, and
//! the direction toward it points out of the box rather than into it. That
//! configuration produces **no** contact, exactly as a sphere whose centre is
//! inside a box does (`sphere_box`). It is the same documented degeneracy, and
//! the same structural answer applies: preventing it is the job of a swept test
//! (see `PhysicsApi::capsule_cast`), not of a deeper-penetration heuristic here.

use axiom_math::{Quat, Vec3};

use crate::collider_capsule::world_capsule;
use crate::collider_obb::{obb_triangles, world_obb};
use crate::contact_geom::{flip, ContactGeom};
use crate::physics_collider_shape::PhysicsColliderShape;

/// Capsule (A) vs box (B). Both rotations are genuinely used: the capsule's
/// orients its shaft, the box's orients its faces.
pub(crate) fn capsule_box(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    rb: Quat,
) -> Option<ContactGeom> {
    world_capsule(a, ca, ra)
        .zip(world_obb(b, cb, rb))
        .and_then(|(capsule, obb)| {
            let axis = capsule.segment();
            let (mine, theirs) = obb_triangles(&obb).iter().fold(
                (axis.start(), obb.center()),
                |best, triangle| {
                    let candidate = axis.closest_points_to_triangle(triangle);
                    let closer = candidate.0.subtract(candidate.1).length_squared()
                        < best.0.subtract(best.1).length_squared();
                    [best, candidate][usize::from(closer)]
                },
            );
            let delta = theirs.subtract(mine);
            let dist = delta.length_squared().sqrt();
            let penetrating =
                (dist > 0.0) & (dist < capsule.radius()) & !obb.contains_point(mine);
            let inv = 1.0 / dist.max(f32::MIN_POSITIVE);
            penetrating.then_some(ContactGeom {
                normal: delta.mul_scalar(inv),
                depth: capsule.radius() - dist,
                point: theirs,
            })
        })
}

/// Box (A) vs capsule (B) — the canonical capsule/box with roles (and rotations)
/// swapped.
pub(crate) fn box_capsule(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    rb: Quat,
) -> Option<ContactGeom> {
    capsule_box(b, cb, rb, a, ca, ra).map(flip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;
    use core::f32::consts::{FRAC_PI_2, FRAC_PI_4};

    fn capsule(radius: f32, half_height: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::capsule(
            Meters::new(radius).unwrap(),
            Meters::new(half_height).unwrap(),
        )
        .unwrap()
    }

    fn box_shape(x: f32, y: f32, z: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::box_shape(Vec3::new(x, y, z)).unwrap()
    }

    fn id() -> Quat {
        Quat::IDENTITY
    }

    fn approx(a: Vec3, b: Vec3) {
        assert!(
            a.subtract(b).length_squared() < 1.0e-7,
            "expected {b:?}, got {a:?}"
        );
    }

    #[test]
    fn a_capsule_standing_on_a_box_meets_its_top_face() {
        // Unit box at the origin (top face y = 1); capsule r = 1 whose lower
        // endpoint sits at y = 1.5, so its cap dips 0.5 into the face.
        let g = capsule_box(
            capsule(1.0, 1.0),
            Vec3::new(0.0, 2.5, 0.0),
            id(),
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
        )
        .expect("the lower cap crosses the top face");
        // capsule(A) -> box(B) points down, into the box.
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0));
        assert!((g.depth - 0.5).abs() < 1.0e-5, "depth was {}", g.depth);
        approx(g.point, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn a_capsule_shaft_against_a_box_side_meets_that_face() {
        // A tall upright capsule beside the box: the contact is shaft-to-face,
        // not cap-to-anything.
        let g = capsule_box(
            capsule(1.0, 4.0),
            Vec3::new(1.5, 0.0, 0.0),
            id(),
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
        )
        .expect("the shaft crosses the +X face");
        approx(g.normal, Vec3::new(-1.0, 0.0, 0.0));
        assert!((g.depth - 0.5).abs() < 1.0e-5, "depth was {}", g.depth);
        assert!((g.point.x - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn a_capsule_over_a_corner_meets_that_corner() {
        // Placed diagonally off the (1,1,1) corner by 0.75 of a unit diagonal:
        // the closest feature is the vertex, not a face.
        let step = 3.0_f32.sqrt().recip() * 0.75;
        let g = capsule_box(
            capsule(1.0, 0.0),
            Vec3::new(1.0 + step, 1.0 + step, 1.0 + step),
            id(),
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
        )
        .expect("a sphere-like capsule off the corner touches it");
        assert!((g.depth - 0.25).abs() < 1.0e-4, "depth was {}", g.depth);
        approx(g.point, Vec3::new(1.0, 1.0, 1.0));
    }

    #[test]
    fn the_box_rotation_genuinely_orients_its_faces() {
        // A thin slab pitched 45 degrees about Z. A capsule placed just off the
        // rotated top face must contact it on that face's real normal, and the
        // same capsule slid out past its radius must miss — the axis-aligned
        // test (which sees a wide flat slab) would report a hit for both.
        let pitch = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_4).unwrap();
        let slab = box_shape(3.0, 0.25, 1.0);
        let face_normal = pitch.rotate(Vec3::UNIT_Y);
        let face_point = pitch.rotate(Vec3::new(0.0, 0.25, 0.0));
        let touching = face_point.add(face_normal.mul_scalar(0.4));
        let g = capsule_box(capsule(0.5, 0.0), touching, id(), slab, Vec3::ZERO, pitch)
            .expect("the cap rests on the tilted face");
        approx(g.normal, face_normal.mul_scalar(-1.0));
        assert!((g.depth - 0.1).abs() < 1.0e-4, "depth was {}", g.depth);

        let clear = face_point.add(face_normal.mul_scalar(0.6));
        assert!(capsule_box(capsule(0.5, 0.0), clear, id(), slab, Vec3::ZERO, pitch).is_none());
    }

    #[test]
    fn a_tipped_capsule_lying_across_a_box_meets_it_shaft_first() {
        // Laid along world X above the box, so only the shaft can reach it.
        let laid = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_2).unwrap();
        let g = capsule_box(
            capsule(1.0, 4.0),
            Vec3::new(0.0, 1.5, 0.0),
            laid,
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
        )
        .expect("the laid shaft dips into the top face");
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0));
        assert!((g.depth - 0.5).abs() < 1.0e-5, "depth was {}", g.depth);
    }

    #[test]
    fn separated_touching_and_axis_inside_configurations_report_no_contact() {
        let shaft = capsule(1.0, 1.0);
        let cube = box_shape(1.0, 1.0, 1.0);
        // Well clear of the box.
        assert!(capsule_box(shaft, Vec3::new(0.0, 9.0, 0.0), id(), cube, Vec3::ZERO, id())
            .is_none());
        // Exactly touching (lower cap grazing the top face) is not a contact.
        assert!(capsule_box(shaft, Vec3::new(0.0, 3.0, 0.0), id(), cube, Vec3::ZERO, id())
            .is_none());
        // Axis inside the box: no defined separating direction.
        assert!(capsule_box(shaft, Vec3::ZERO, id(), cube, Vec3::ZERO, id()).is_none());
    }

    #[test]
    fn an_axis_buried_just_under_a_face_is_rejected_rather_than_pushed_inward() {
        // Axis from y = 0.5 to y = 1.5 inside a box of half-extent 2: its upper
        // end is 0.5 below the top face, well within the 1.0 radius, so the
        // distance test alone would report a contact whose normal points
        // *further into* the box. The inside test is what rules it out.
        let buried = capsule_box(
            capsule(1.0, 0.5),
            Vec3::new(0.0, 1.0, 0.0),
            id(),
            box_shape(2.0, 2.0, 2.0),
            Vec3::ZERO,
            id(),
        );
        assert!(buried.is_none(), "an interior axis has no defined normal");
    }

    #[test]
    fn an_unliftable_capsule_or_box_reports_no_contact() {
        let bad = Vec3::new(f32::NAN, 0.0, 0.0);
        assert!(capsule_box(
            capsule(1.0, 1.0),
            bad,
            id(),
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id()
        )
        .is_none());
        assert!(capsule_box(
            capsule(1.0, 1.0),
            Vec3::ZERO,
            id(),
            box_shape(1.0, 1.0, 1.0),
            bad,
            id()
        )
        .is_none());
    }

    #[test]
    fn box_capsule_flips_the_canonical_normal() {
        let g = box_capsule(
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
            capsule(1.0, 1.0),
            Vec3::new(0.0, 2.5, 0.0),
            id(),
        )
        .expect("the same contact, seen from the box");
        approx(g.normal, Vec3::UNIT_Y);
        assert!((g.depth - 0.5).abs() < 1.0e-5);
        assert!(box_capsule(
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
            capsule(1.0, 1.0),
            Vec3::new(0.0, 9.0, 0.0),
            id()
        )
        .is_none());
    }
}
