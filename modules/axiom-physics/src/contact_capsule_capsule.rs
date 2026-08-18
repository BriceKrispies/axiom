//! The capsule↔capsule narrow-phase pairing.
//!
//! Two capsules touch exactly when their axis segments come within the sum of
//! their radii, so this is the sphere/sphere test taken at the closest pair of
//! points between the two axes — the solve
//! [`axiom_math::Segment::closest_points_to_segment`] owns, including the
//! parallel and degenerate configurations it pins deterministically.
//!
//! Both orderings share one generator: the pairing is symmetric, so the reversed
//! entry is the canonical one with its roles (and rotations) swapped and its
//! normal flipped.
//!
//! Two axes that intersect have no defined separating direction and produce no
//! contact — the same documented degeneracy as coincident sphere centres.
//!
//! ## One point, deterministically chosen
//! Like every other pairing in this narrow phase, this one reports a **single**
//! contact point. Two exactly parallel shafts touch along a whole span rather
//! than at a point, and the math layer's closest-point solve pins that
//! ambiguity to a facing endpoint pair; the resulting contact therefore sits at
//! one end of the overlap rather than its middle, which lets the solver spin two
//! side-by-side capsules slightly as it separates them. That is a property of
//! the single-point manifold shape, not of this pairing — a multi-point manifold
//! is the structural fix, and it belongs to `ContactManifold`, not here.

use axiom_math::{Quat, Vec3};

use crate::collider_capsule::world_capsule;
use crate::contact_geom::ContactGeom;
use crate::physics_collider_shape::PhysicsColliderShape;

/// Capsule (A) vs capsule (B). Each rotation orients its own shaft.
pub(crate) fn capsule_capsule(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    rb: Quat,
) -> Option<ContactGeom> {
    world_capsule(a, ca, ra)
        .zip(world_capsule(b, cb, rb))
        .and_then(|(first, second)| {
            let (mine, theirs) = first
                .segment()
                .closest_points_to_segment(&second.segment());
            let delta = theirs.subtract(mine);
            let dist = delta.length_squared().sqrt();
            let sum = first.radius() + second.radius();
            let penetrating = (dist > 0.0) & (dist < sum);
            let inv = 1.0 / dist.max(f32::MIN_POSITIVE);
            let normal = delta.mul_scalar(inv);
            let depth = sum - dist;
            let point = mine.add(normal.mul_scalar(first.radius() - depth * 0.5));
            penetrating.then_some(ContactGeom {
                normal,
                depth,
                point,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;
    use core::f32::consts::FRAC_PI_2;

    fn capsule(radius: f32, half_height: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::capsule(
            Meters::new(radius).unwrap(),
            Meters::new(half_height).unwrap(),
        )
        .unwrap()
    }

    fn id() -> Quat {
        Quat::IDENTITY
    }

    fn approx(a: Vec3, b: Vec3) {
        assert!(
            a.subtract(b).length_squared() < 1.0e-8,
            "expected {b:?}, got {a:?}"
        );
    }

    #[test]
    fn two_parallel_shafts_meet_across_the_gap_between_them() {
        // Two upright r = 1 capsules 1.5 apart on X: the axes are 1.5 apart and
        // the radii sum to 2, so they overlap by 0.5 along +X.
        let g = capsule_capsule(
            capsule(1.0, 1.0),
            Vec3::ZERO,
            id(),
            capsule(1.0, 1.0),
            Vec3::new(1.5, 0.0, 0.0),
            id(),
        )
        .expect("overlapping parallel capsules are in contact");
        approx(g.normal, Vec3::UNIT_X);
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        assert!((g.point.x - 0.75).abs() < 1.0e-6);
    }

    #[test]
    fn two_crossing_shafts_meet_where_they_pass() {
        // An upright capsule and one laid along X, offset 1.5 on Z so the shafts
        // pass skew 1.5 apart: the contact is the skew closest pair, which lies
        // in both shafts' interiors and at neither one's endpoint.
        let laid = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_2).unwrap();
        let g = capsule_capsule(
            capsule(1.0, 2.0),
            Vec3::ZERO,
            id(),
            capsule(1.0, 2.0),
            Vec3::new(0.0, 0.0, 1.5),
            laid,
        )
        .expect("crossing shafts overlap");
        approx(g.normal, Vec3::UNIT_Z);
        assert!((g.depth - 0.5).abs() < 1.0e-5);
        approx(g.point, Vec3::new(0.0, 0.0, 0.75));
    }

    #[test]
    fn a_cap_against_a_cap_meets_end_to_end() {
        // Two upright capsules stacked with 1.5 between their facing endpoints.
        let g = capsule_capsule(
            capsule(1.0, 1.0),
            Vec3::ZERO,
            id(),
            capsule(1.0, 1.0),
            Vec3::new(0.0, 3.5, 0.0),
            id(),
        )
        .expect("the two caps overlap");
        approx(g.normal, Vec3::UNIT_Y);
        assert!((g.depth - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn a_zero_length_capsule_behaves_exactly_as_a_sphere() {
        let g = capsule_capsule(
            capsule(1.0, 0.0),
            Vec3::ZERO,
            id(),
            capsule(1.0, 0.0),
            Vec3::new(1.5, 0.0, 0.0),
            id(),
        )
        .expect("two degenerate capsules are two spheres");
        approx(g.normal, Vec3::UNIT_X);
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        approx(g.point, Vec3::new(0.75, 0.0, 0.0));
    }

    #[test]
    fn separated_touching_and_coincident_capsules_report_no_contact() {
        let shaft = capsule(1.0, 1.0);
        assert!(capsule_capsule(
            shaft,
            Vec3::ZERO,
            id(),
            shaft,
            Vec3::new(9.0, 0.0, 0.0),
            id()
        )
        .is_none());
        // Exactly touching (axes 2.0 apart, radii sum 2.0) is not a contact.
        assert!(capsule_capsule(
            shaft,
            Vec3::ZERO,
            id(),
            shaft,
            Vec3::new(2.0, 0.0, 0.0),
            id()
        )
        .is_none());
        // Coincident axes have no defined separating direction.
        assert!(capsule_capsule(shaft, Vec3::ZERO, id(), shaft, Vec3::ZERO, id()).is_none());
    }

    #[test]
    fn an_unliftable_capsule_on_either_side_reports_no_contact() {
        let shaft = capsule(1.0, 1.0);
        let bad = Vec3::new(f32::NAN, 0.0, 0.0);
        assert!(capsule_capsule(shaft, bad, id(), shaft, Vec3::ZERO, id()).is_none());
        assert!(capsule_capsule(shaft, Vec3::ZERO, id(), shaft, bad, id()).is_none());
    }
}
