//! Lifting a capsule collider into the math layer's [`Capsule`] volume.
//!
//! A [`crate::physics_collider_shape::PhysicsColliderShape`] stores a capsule as
//! a packed radius plus the local AABB half-size `(r, half_height + r, r)`; the
//! math layer's `Capsule` is a segment plus a radius, in **world** space. This is
//! the single place that conversion happens, so every capsule contact, overlap
//! and sweep in the module agrees on where a capsule collider actually is.
//!
//! The capsule's axis runs along the collider's **local Y**, so the owning body's
//! rotation genuinely orients it — unlike the legacy axis-aligned box tests, a
//! tilted capsule collides on its true tilted shaft.

use axiom_math::{Capsule, Quat, Segment, Vec3};

use crate::physics_collider_shape::PhysicsColliderShape;

/// The world-space [`Capsule`] of a capsule collider whose owning body sits at
/// `center` with rotation `rotation`.
///
/// Returns `None` only for geometry the math layer refuses (a non-finite centre,
/// or a radius that is not finite and non-negative) — states a validated
/// collider on a validated body cannot reach, so a `None` here is a miss, never
/// a silent approximation.
pub(crate) fn world_capsule(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
) -> Option<Capsule> {
    // `half_extents.y = half_height + radius`, so the cylinder half-length (the
    // axis's reach from the centre) is that minus the radius.
    let half_height = shape.half_extents().y - shape.radius();
    let up = rotation.rotate(Vec3::new(0.0, half_height, 0.0));
    Segment::new(center.subtract(up), center.add(up))
        .and_then(|axis| Capsule::new(axis, shape.radius()))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;
    use core::f32::consts::FRAC_PI_2;

    fn capsule_shape(radius: f32, half_height: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::capsule(
            Meters::new(radius).unwrap(),
            Meters::new(half_height).unwrap(),
        )
        .unwrap()
    }

    fn approx(a: Vec3, b: Vec3) {
        assert!(
            a.subtract(b).length_squared() < 1.0e-9,
            "expected {b:?}, got {a:?}"
        );
    }

    #[test]
    fn an_upright_capsule_spans_its_half_height_about_the_body_centre() {
        let world = world_capsule(
            capsule_shape(0.5, 1.0),
            Vec3::new(0.0, 2.0, 0.0),
            Quat::IDENTITY,
        )
        .expect("a validated capsule lifts");
        approx(world.segment().start(), Vec3::new(0.0, 1.0, 0.0));
        approx(world.segment().end(), Vec3::new(0.0, 3.0, 0.0));
        assert_eq!(world.radius(), 0.5);
    }

    #[test]
    fn a_rotated_capsule_lies_along_the_rotated_axis() {
        // A quarter turn about +Z carries local +Y onto world -X.
        let tipped = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_2).unwrap();
        let world = world_capsule(capsule_shape(0.5, 2.0), Vec3::ZERO, tipped)
            .expect("a validated capsule lifts");
        approx(world.segment().start(), Vec3::new(2.0, 0.0, 0.0));
        approx(world.segment().end(), Vec3::new(-2.0, 0.0, 0.0));
    }

    #[test]
    fn a_zero_half_height_capsule_degenerates_to_a_sphere() {
        let world = world_capsule(capsule_shape(1.0, 0.0), Vec3::ZERO, Quat::IDENTITY)
            .expect("a validated capsule lifts");
        assert_eq!(world.segment().length(), 0.0);
        assert!(world.contains_point(Vec3::new(0.0, 0.99, 0.0)));
        assert!(!world.contains_point(Vec3::new(0.0, 1.01, 0.0)));
    }

    #[test]
    fn a_non_finite_centre_lifts_to_nothing() {
        assert!(world_capsule(
            capsule_shape(0.5, 1.0),
            Vec3::new(f32::NAN, 0.0, 0.0),
            Quat::IDENTITY
        )
        .is_none());
    }
}
