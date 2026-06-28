//! World-space axis-aligned bounds of a collider.
//!
//! The broad phase and the overlap queries both need a collider's world AABB.
//! Bounds place a collider at its owning body's translation and **ignore body
//! rotation and scale** (there is no angular integration yet — see `ROADMAP.md`),
//! so a finite shape's local axis-aligned half-extents are already world-aligned
//! and the AABB is simply `center ± half_extents`. A plane is infinite and has no
//! finite AABB, so it returns `None` and is handled directly by the narrow phase
//! and ray/plane queries instead of by bounds culling.

use axiom_math::{Aabb, Vec3};

use crate::physics_collider_shape::PhysicsColliderShape;

/// The world AABB of `shape` centered at `center`, or `None` for an infinite
/// (plane) shape. For every finite kind the half-extents were validated positive
/// and finite at shape construction and `center` comes from a finite, validated
/// body transform, so `Aabb::from_center_extents` always succeeds — the `None`
/// arm of the conversion is reachable only for a plane.
pub(crate) fn world_aabb(shape: PhysicsColliderShape, center: Vec3) -> Option<Aabb> {
    shape
        .kind()
        .is_finite()
        .then(|| Aabb::from_center_extents(center, shape.half_extents()))
        .and_then(Result::ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;

    fn m(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    #[test]
    fn finite_shapes_have_a_centered_world_aabb() {
        let sphere = PhysicsColliderShape::sphere(m(2.0)).unwrap();
        let aabb = world_aabb(sphere, Vec3::new(1.0, 5.0, 0.0)).expect("sphere is finite");
        assert_eq!(aabb.min(), Vec3::new(-1.0, 3.0, -2.0));
        assert_eq!(aabb.max(), Vec3::new(3.0, 7.0, 2.0));
    }

    #[test]
    fn box_world_aabb_uses_half_extents() {
        let shape = PhysicsColliderShape::box_shape(Vec3::new(1.0, 2.0, 3.0)).unwrap();
        let aabb = world_aabb(shape, Vec3::ZERO).expect("box is finite");
        assert_eq!(aabb.min(), Vec3::new(-1.0, -2.0, -3.0));
        assert_eq!(aabb.max(), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn a_plane_has_no_finite_aabb() {
        let plane = PhysicsColliderShape::plane(Vec3::UNIT_Y, m(0.0)).unwrap();
        assert!(world_aabb(plane, Vec3::ZERO).is_none());
    }
}
