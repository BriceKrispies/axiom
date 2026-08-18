//! The capsule↔sphere narrow-phase pairing.
//!
//! A capsule is every point within its radius of its axis segment, so a capsule
//! against a sphere is exactly the sphere/sphere test taken at the point on the
//! axis closest to the sphere's centre. The convention is inherited unchanged
//! from `sphere_sphere`: the normal points from collider A toward collider B,
//! the depth is the strictly positive overlap of the two radii, and the contact
//! point sits at the middle of the overlapped region.
//!
//! A sphere centred exactly *on* the capsule's axis has no defined separating
//! direction and produces no contact — the same documented degeneracy as two
//! coincident sphere centres.

use axiom_math::{Quat, Vec3};

use crate::collider_capsule::world_capsule;
use crate::contact_geom::{flip, ContactGeom};
use crate::physics_collider_shape::PhysicsColliderShape;

/// Capsule (A) vs sphere (B). The capsule's rotation orients its axis; a sphere
/// is rotation-invariant.
pub(crate) fn capsule_sphere(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    _rb: Quat,
) -> Option<ContactGeom> {
    world_capsule(a, ca, ra).and_then(|capsule| {
        let axis_point = capsule.segment().closest_point_to(cb);
        let delta = cb.subtract(axis_point);
        let dist = delta.length_squared().sqrt();
        let sum = capsule.radius() + b.radius();
        let penetrating = (dist > 0.0) & (dist < sum);
        let inv = 1.0 / dist.max(f32::MIN_POSITIVE);
        let normal = delta.mul_scalar(inv);
        let depth = sum - dist;
        let point = axis_point.add(normal.mul_scalar(capsule.radius() - depth * 0.5));
        penetrating.then_some(ContactGeom {
            normal,
            depth,
            point,
        })
    })
}

/// Sphere (A) vs capsule (B) — the canonical capsule/sphere with roles (and
/// rotations) swapped.
pub(crate) fn sphere_capsule(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    rb: Quat,
) -> Option<ContactGeom> {
    capsule_sphere(b, cb, rb, a, ca, ra).map(flip)
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

    fn sphere(radius: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::sphere(Meters::new(radius).unwrap()).unwrap()
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
    fn a_sphere_against_the_shaft_pushes_out_radially() {
        // Capsule r = 1 with a 2-unit axis about the origin; sphere r = 1 at
        // x = 1.5, level with the middle of the shaft. Overlap is 0.5.
        let g = capsule_sphere(
            capsule(1.0, 1.0),
            Vec3::ZERO,
            id(),
            sphere(1.0),
            Vec3::new(1.5, 0.0, 0.0),
            id(),
        )
        .expect("overlapping capsule and sphere are in contact");
        approx(g.normal, Vec3::UNIT_X);
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        approx(g.point, Vec3::new(0.75, 0.0, 0.0));
    }

    #[test]
    fn a_sphere_beyond_the_end_of_the_axis_meets_the_cap() {
        // The axis ends at y = 1; a sphere at y = 2.5 is 1.5 from that endpoint,
        // radii sum to 2.0, so the overlap is 0.5 along +Y.
        let g = capsule_sphere(
            capsule(1.0, 1.0),
            Vec3::ZERO,
            id(),
            sphere(1.0),
            Vec3::new(0.0, 2.5, 0.0),
            id(),
        )
        .expect("the cap reaches a full radius past the axis");
        approx(g.normal, Vec3::UNIT_Y);
        assert!((g.depth - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn the_capsule_rotation_genuinely_orients_the_shaft() {
        // Tipped a quarter turn about Z the axis lies along world X and reaches
        // x = 2, so a sphere at x = 3.5 now meets a cap it would have missed
        // entirely upright (where the axis is 3.5 away, past the summed radii).
        let tipped = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_2).unwrap();
        let far = Vec3::new(3.5, 0.0, 0.0);
        assert!(capsule_sphere(
            capsule(1.0, 2.0),
            Vec3::ZERO,
            id(),
            sphere(1.0),
            far,
            id()
        )
        .is_none());
        let g = capsule_sphere(capsule(1.0, 2.0), Vec3::ZERO, tipped, sphere(1.0), far, id())
            .expect("the tipped shaft reaches x = 2");
        approx(g.normal, Vec3::UNIT_X);
        assert!((g.depth - 0.5).abs() < 1.0e-5);
    }

    #[test]
    fn separated_touching_and_coincident_configurations_report_no_contact() {
        let shaft = capsule(1.0, 1.0);
        // Clear of the capsule entirely.
        assert!(capsule_sphere(
            shaft,
            Vec3::ZERO,
            id(),
            sphere(1.0),
            Vec3::new(5.0, 0.0, 0.0),
            id()
        )
        .is_none());
        // Exactly touching (distance == sum of radii) is not a contact.
        assert!(capsule_sphere(
            shaft,
            Vec3::ZERO,
            id(),
            sphere(1.0),
            Vec3::new(2.0, 0.0, 0.0),
            id()
        )
        .is_none());
        // Centre on the axis: no defined normal.
        assert!(
            capsule_sphere(shaft, Vec3::ZERO, id(), sphere(1.0), Vec3::ZERO, id()).is_none()
        );
    }

    #[test]
    fn an_unliftable_capsule_reports_no_contact() {
        assert!(capsule_sphere(
            capsule(1.0, 1.0),
            Vec3::new(f32::NAN, 0.0, 0.0),
            id(),
            sphere(1.0),
            Vec3::ZERO,
            id()
        )
        .is_none());
    }

    #[test]
    fn sphere_capsule_flips_the_canonical_normal() {
        let g = sphere_capsule(
            sphere(1.0),
            Vec3::new(1.5, 0.0, 0.0),
            id(),
            capsule(1.0, 1.0),
            Vec3::ZERO,
            id(),
        )
        .expect("the same overlap, seen from the sphere");
        // sphere(A) -> capsule(B) is -X, the reverse of the canonical +X.
        approx(g.normal, Vec3::new(-1.0, 0.0, 0.0));
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        assert!(sphere_capsule(
            sphere(1.0),
            Vec3::new(9.0, 0.0, 0.0),
            id(),
            capsule(1.0, 1.0),
            Vec3::ZERO,
            id()
        )
        .is_none());
    }
}
