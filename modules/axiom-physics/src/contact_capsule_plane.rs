//! The capsule↔plane narrow-phase pairing.
//!
//! A plane is a one-sided **solid half-space** `n · x = offset` whose stored unit
//! normal points to the empty side. A capsule crosses into the solid exactly when
//! the signed distance of the *deepest* point of its axis drops below its radius,
//! so the depth is `radius - min(signed distance of the two endpoints)` — the
//! same form `sphere_plane` uses, with the single centre replaced by whichever
//! end of the shaft is lower.
//!
//! ## Why the contact point is weighted, not picked
//! A capsule lying parallel to the plane has *both* endpoints equally deep, and
//! there is no reason to prefer either: picking one would push the body
//! off-centre and spin a capsule that should simply rest. So the contact point
//! is the two endpoints blended by **how far each has penetrated**,
//! `w_i = max(0, radius - signed_i)`. A capsule with only one end in the solid
//! contacts at that end (the other weight is exactly zero); a parallel capsule
//! contacts at its midpoint (the weights are equal); a partly tilted capsule
//! contacts between them, where its real contact patch is centred. The blend is
//! continuous in the capsule's tilt, so a body settling onto the plane does not
//! snap its contact point from one end to the other, and it never divides by
//! zero: the deepest endpoint's weight *is* the penetration depth, which is
//! strictly positive for any reported contact.

use axiom_math::{Quat, Vec3};

use crate::collider_capsule::world_capsule;
use crate::contact_geom::{flip, ContactGeom};
use crate::physics_collider_shape::PhysicsColliderShape;

/// Capsule (A) vs plane (B). The plane's centre and rotation are irrelevant —
/// it is defined entirely by its unit normal and signed offset.
pub(crate) fn capsule_plane(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    _cb: Vec3,
    _rb: Quat,
) -> Option<ContactGeom> {
    world_capsule(a, ca, ra).and_then(|capsule| {
        let n = b.normal();
        let (start, end) = (capsule.segment().start(), capsule.segment().end());
        let (first, second) = (n.dot(start) - b.offset(), n.dot(end) - b.offset());
        let deepest = first.min(second);
        let depth = capsule.radius() - deepest;
        // How far each endpoint has entered the solid; the deepest one's weight
        // equals `depth`, so the sum is positive for every reported contact.
        let (wa, wb) = (
            (capsule.radius() - first).max(0.0),
            (capsule.radius() - second).max(0.0),
        );
        let axis_point = start
            .mul_scalar(wa)
            .add(end.mul_scalar(wb))
            .mul_scalar(1.0 / (wa + wb).max(f32::MIN_POSITIVE));
        (depth > 0.0).then_some(ContactGeom {
            normal: n.mul_scalar(-1.0),
            depth,
            point: axis_point.subtract(n.mul_scalar(capsule.radius())),
        })
    })
}

/// Plane (A) vs capsule (B) — the canonical capsule/plane with roles (and
/// rotations) swapped.
pub(crate) fn plane_capsule(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    rb: Quat,
) -> Option<ContactGeom> {
    capsule_plane(b, cb, rb, a, ca, ra).map(flip)
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

    fn ground() -> PhysicsColliderShape {
        PhysicsColliderShape::plane(Vec3::UNIT_Y, Meters::new(0.0).unwrap()).unwrap()
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
    fn an_upright_capsule_sinking_into_the_ground_is_pushed_up() {
        // Axis from y = 0.5 to y = 2.5, r = 1: the lower endpoint is 0.5 above
        // the surface, so the capsule sinks 0.5 into it.
        let g = capsule_plane(
            capsule(1.0, 1.0),
            Vec3::new(0.0, 1.5, 0.0),
            id(),
            ground(),
            Vec3::ZERO,
            id(),
        )
        .expect("the lower cap crosses the surface");
        // capsule(A) -> plane(B) points down, into the solid.
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0));
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        approx(g.point, Vec3::new(0.0, -0.5, 0.0));
    }

    #[test]
    fn a_capsule_clear_of_the_surface_reports_no_contact() {
        assert!(capsule_plane(
            capsule(1.0, 1.0),
            Vec3::new(0.0, 5.0, 0.0),
            id(),
            ground(),
            Vec3::ZERO,
            id()
        )
        .is_none());
        // Exactly resting (lower endpoint one radius up) is not a contact.
        assert!(capsule_plane(
            capsule(1.0, 1.0),
            Vec3::new(0.0, 2.0, 0.0),
            id(),
            ground(),
            Vec3::ZERO,
            id()
        )
        .is_none());
    }

    #[test]
    fn a_capsule_lying_flat_contacts_at_its_midpoint() {
        // Laid along world X (a quarter turn about Z), both endpoints are at the
        // same height, so neither end may be preferred: the contact is the axis
        // midpoint, directly under the body centre.
        let laid = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_2).unwrap();
        let g = capsule_plane(
            capsule(1.0, 2.0),
            Vec3::new(3.0, 0.5, 0.0),
            laid,
            ground(),
            Vec3::ZERO,
            id(),
        )
        .expect("a flat capsule at y = 0.5 sinks 0.5 into the ground");
        assert!((g.depth - 0.5).abs() < 1.0e-5);
        approx(g.point, Vec3::new(3.0, -0.5, 0.0));
    }

    #[test]
    fn a_tilted_capsule_contacts_at_its_lower_end_only() {
        // Pitched 45 degrees about Z: the two endpoints are at different heights,
        // so the lower one alone carries the contact.
        let tilt = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_4).unwrap();
        let half = 2.0_f32 * FRAC_PI_4.sin(); // the axis end's vertical drop
        let g = capsule_plane(
            capsule(1.0, 2.0),
            Vec3::new(0.0, half + 0.5, 0.0),
            tilt,
            ground(),
            Vec3::ZERO,
            id(),
        )
        .expect("the lower end dips into the ground");
        assert!((g.depth - 0.5).abs() < 1.0e-5);
        // The contact sits under the lower endpoint, off the body centre in X.
        assert!(
            g.point.x.abs() > 1.0,
            "a tilted capsule must contact off-centre, got {:?}",
            g.point
        );
        assert!((g.point.y + 0.5).abs() < 1.0e-5);
    }

    #[test]
    fn a_steep_plane_pushes_along_its_own_normal() {
        // A vertical wall at x = 0 whose empty side is +X: a capsule at x = 0.5
        // with r = 1 sinks 0.5 into it, and the push is along -X.
        let wall = PhysicsColliderShape::plane(Vec3::UNIT_X, Meters::new(0.0).unwrap()).unwrap();
        let g = capsule_plane(
            capsule(1.0, 1.0),
            Vec3::new(0.5, 0.0, 0.0),
            id(),
            wall,
            Vec3::ZERO,
            id(),
        )
        .expect("the shaft crosses the wall");
        approx(g.normal, Vec3::new(-1.0, 0.0, 0.0));
        assert!((g.depth - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn an_unliftable_capsule_reports_no_contact() {
        assert!(capsule_plane(
            capsule(1.0, 1.0),
            Vec3::new(0.0, f32::NAN, 0.0),
            id(),
            ground(),
            Vec3::ZERO,
            id()
        )
        .is_none());
    }

    #[test]
    fn plane_capsule_flips_the_canonical_normal() {
        let g = plane_capsule(
            ground(),
            Vec3::ZERO,
            id(),
            capsule(1.0, 1.0),
            Vec3::new(0.0, 1.5, 0.0),
            id(),
        )
        .expect("the same contact, seen from the plane");
        // plane(A) -> capsule(B) points up, out of the solid.
        approx(g.normal, Vec3::UNIT_Y);
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        assert!(plane_capsule(
            ground(),
            Vec3::ZERO,
            id(),
            capsule(1.0, 1.0),
            Vec3::new(0.0, 9.0, 0.0),
            id()
        )
        .is_none());
    }
}
