//! World-space axis-aligned bounds of a collider.
//!
//! The broad phase and the overlap queries both need a collider's world AABB.
//! Bounds place a collider at its owning body's translation and **scale is
//! ignored** (the module carries no transform scale), but body **rotation is
//! honoured for boxes**: an oriented box's world AABB is the axis-aligned
//! envelope of its rotated corners, so a tilted platform bounds correctly. A
//! sphere and a capsule are rotation-invariant, so their local axis-aligned
//! half-extents already bound them at any orientation. A plane is infinite and
//! has no finite AABB, so it returns `None` and is handled directly by the narrow
//! phase and ray/plane queries instead of by bounds culling.

use axiom_math::{Aabb, Quat, Vec3};

use crate::physics_collider_shape::PhysicsColliderShape;

/// The world AABB of `shape` centered at `center` under body `rotation`, or
/// `None` for an infinite (plane) shape. For every finite kind the half-extents
/// were validated positive and finite at shape construction and `center`/`rotation`
/// come from a finite, validated body transform, so `Aabb::from_center_extents`
/// always succeeds — the `None` arm of the conversion is reachable only for a
/// plane.
pub(crate) fn world_aabb(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
) -> Option<Aabb> {
    shape
        .kind()
        .is_finite()
        .then(|| Aabb::from_center_extents(center, world_extents(shape, rotation)))
        .and_then(Result::ok)
}

/// The world-space half-extents of a finite shape's bounds. A sphere/capsule is
/// rotation-invariant, so its local half-extents already bound it. A box's — and a
/// heightfield's — bound is the OBB→AABB projection of its rotated axes. The two
/// candidates are blended arithmetically by `oriented` (`0`/`1`), so there is no
/// branch and a rolling sphere body keeps a tight `(r, r, r)` bound while only
/// boxes and heightfields widen with orientation.
fn world_extents(shape: PhysicsColliderShape, rotation: Quat) -> Vec3 {
    let aligned = shape.half_extents();
    let rotated = rotated_box_extents(rotation, aligned);
    let oriented = ((shape.is_box() as u32) | (shape.is_heightfield() as u32)) as f32;
    aligned.add(rotated.subtract(aligned).mul_scalar(oriented))
}

/// The world AABB half-extents of a box with local half-extents `he` rotated by
/// `rotation`: the sum of the absolute rotated half-axis vectors. At the identity
/// rotation this collapses to `he` exactly.
fn rotated_box_extents(rotation: Quat, he: Vec3) -> Vec3 {
    let ax = rotation.rotate(Vec3::new(he.x, 0.0, 0.0));
    let ay = rotation.rotate(Vec3::new(0.0, he.y, 0.0));
    let az = rotation.rotate(Vec3::new(0.0, 0.0, he.z));
    componentwise_abs(ax)
        .add(componentwise_abs(ay))
        .add(componentwise_abs(az))
}

/// The componentwise absolute value of a vector (branchless — `f32::abs` is
/// arithmetic).
fn componentwise_abs(v: Vec3) -> Vec3 {
    Vec3::new(v.x.abs(), v.y.abs(), v.z.abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;
    use core::f32::consts::FRAC_PI_4;

    fn m(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    #[test]
    fn finite_shapes_have_a_centered_world_aabb() {
        let sphere = PhysicsColliderShape::sphere(m(2.0)).unwrap();
        let aabb =
            world_aabb(sphere, Vec3::new(1.0, 5.0, 0.0), Quat::IDENTITY).expect("sphere is finite");
        assert_eq!(aabb.min(), Vec3::new(-1.0, 3.0, -2.0));
        assert_eq!(aabb.max(), Vec3::new(3.0, 7.0, 2.0));
    }

    #[test]
    fn box_world_aabb_uses_half_extents() {
        let shape = PhysicsColliderShape::box_shape(Vec3::new(1.0, 2.0, 3.0)).unwrap();
        let aabb = world_aabb(shape, Vec3::ZERO, Quat::IDENTITY).expect("box is finite");
        assert_eq!(aabb.min(), Vec3::new(-1.0, -2.0, -3.0));
        assert_eq!(aabb.max(), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn a_plane_has_no_finite_aabb() {
        let plane = PhysicsColliderShape::plane(Vec3::UNIT_Y, m(0.0)).unwrap();
        assert!(world_aabb(plane, Vec3::ZERO, Quat::IDENTITY).is_none());
    }

    #[test]
    fn a_rotated_box_widens_its_world_aabb_to_the_oriented_envelope() {
        // A unit box yawed 45° about Y: its X/Z footprint grows to the rotated
        // diagonal (half-extent sqrt(2)), while Y is unchanged.
        let shape = PhysicsColliderShape::box_shape(Vec3::new(1.0, 1.0, 1.0)).unwrap();
        let yaw = Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_4).unwrap();
        let aabb = world_aabb(shape, Vec3::ZERO, yaw).expect("box is finite");
        let expected = 2.0_f32.sqrt();
        let mx = aabb.max();
        assert!(
            (mx.x - expected).abs() < 1.0e-5,
            "x widened to the rotated diagonal, got {mx:?}"
        );
        assert!(
            (mx.z - expected).abs() < 1.0e-5,
            "z widened to the rotated diagonal, got {mx:?}"
        );
        assert!(
            (mx.y - 1.0).abs() < 1.0e-6,
            "y unchanged under yaw, got {mx:?}"
        );
    }

    #[test]
    fn a_rotated_sphere_keeps_its_tight_axis_aligned_bound() {
        // A sphere is rotation-invariant: any body rotation must leave its AABB at
        // the exact `center ± r`, never the inflated box envelope.
        let sphere = PhysicsColliderShape::sphere(m(2.0)).unwrap();
        let yaw = Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_4).unwrap();
        let aabb = world_aabb(sphere, Vec3::ZERO, yaw).expect("sphere is finite");
        assert_eq!(aabb.min(), Vec3::new(-2.0, -2.0, -2.0));
        assert_eq!(aabb.max(), Vec3::new(2.0, 2.0, 2.0));
    }

    #[test]
    fn a_heightfield_is_finite_and_widens_its_aabb_like_a_box_when_yawed() {
        // A heightfield footprint (4, 1, 6): axis-aligned it bounds exactly; yawed
        // it widens to the oriented envelope, unlike a sphere.
        let hf = PhysicsColliderShape::heightfield_shape(Vec3::new(4.0, 1.0, 6.0)).unwrap();
        let aligned = world_aabb(hf, Vec3::ZERO, Quat::IDENTITY).expect("heightfield is finite");
        assert_eq!(aligned.max(), Vec3::new(4.0, 1.0, 6.0));
        let yaw = Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_4).unwrap();
        let rotated = world_aabb(hf, Vec3::ZERO, yaw).expect("heightfield is finite");
        let rmax = rotated.max();
        assert!(
            rmax.x > 4.0 + 1.0,
            "footprint widened under yaw, got {rmax:?}"
        );
    }
}
