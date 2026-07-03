//! Per-bone anatomical joint limits and the rotation clamp they apply.

use axiom_math::{Quat, Transform, Vec3};

use crate::animation_error::AnimationError;
use crate::animation_result::AnimationResult;
use crate::ids::BoneId;

/// An axis-aligned limit on a bone's local rotation, expressed as inclusive
/// Euler-angle bounds (radians) in the `Quat::from_euler_xyz` convention — the
/// mechanism a rig uses to keep a joint within its anatomically valid range
/// (a knee that only hinges, an elbow that cannot hyperextend). `min <= max`
/// componentwise is enforced at construction.
///
/// A `JointLimit` is a pure value the caller builds via
/// [`crate::AnimationApi::joint_limit`] and hands back to `clamp_pose` /
/// `is_pose_legal`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointLimit {
    bone: BoneId,
    min: Vec3,
    max: Vec3,
}

impl JointLimit {
    /// Build a validated limit for `bone` with per-axis Euler `min`/`max`
    /// bounds (radians). Fails with `InvalidJointLimit` if any `min` axis
    /// exceeds its `max`.
    pub(crate) fn new(bone: BoneId, min: Vec3, max: Vec3) -> AnimationResult<JointLimit> {
        ((min.x <= max.x) & (min.y <= max.y) & (min.z <= max.z))
            .then_some(JointLimit { bone, min, max })
            .ok_or_else(|| {
                AnimationError::invalid_joint_limit("joint limit min bound exceeds max bound")
            })
    }

    /// The bone this limit constrains.
    pub(crate) fn bone(self) -> BoneId {
        self.bone
    }

    /// Clamp `rotation`'s Euler angles into this limit's bounds.
    fn clamp_rotation(self, rotation: Quat) -> Quat {
        let e = rotation.to_euler_xyz();
        Quat::from_euler_xyz(
            e.x.clamp(self.min.x, self.max.x),
            e.y.clamp(self.min.y, self.max.y),
            e.z.clamp(self.min.z, self.max.z),
        )
    }

    /// Return `transform` with its rotation clamped into this limit; translation
    /// and scale are untouched.
    pub(crate) fn clamp_transform(self, transform: Transform) -> Transform {
        Transform::new(
            transform.translation,
            self.clamp_rotation(transform.rotation),
            transform.scale,
        )
    }

    /// Whether `rotation` already lies within this limit (within a small
    /// tolerance to absorb the Euler round-trip's floating-point error).
    pub(crate) fn contains(self, rotation: Quat) -> bool {
        let e = rotation.to_euler_xyz();
        let tol = 1.0e-4_f32;
        ((e.x >= self.min.x - tol) & (e.x <= self.max.x + tol))
            & ((e.y >= self.min.y - tol) & (e.y <= self.max.y + tol))
            & ((e.z >= self.min.z - tol) & (e.z <= self.max.z + tol))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_error_code::AnimationErrorCode;

    fn bound(v: f32) -> Vec3 {
        Vec3::new(v, v, v)
    }

    #[test]
    fn min_greater_than_max_is_rejected() {
        let err = JointLimit::new(BoneId::from_raw(0), bound(1.0), bound(-1.0)).unwrap_err();
        assert_eq!(err.code(), AnimationErrorCode::InvalidJointLimit);
    }

    #[test]
    fn records_its_bone() {
        let limit = JointLimit::new(BoneId::from_raw(3), bound(-1.0), bound(1.0)).unwrap();
        assert_eq!(limit.bone(), BoneId::from_raw(3));
    }

    #[test]
    fn clamp_pulls_an_out_of_range_rotation_back_in() {
        // Limit the X axis to [0, 0.5] rad; a 1.0-rad X rotation clamps to 0.5.
        let limit = JointLimit::new(
            BoneId::from_raw(0),
            Vec3::new(0.0, -0.1, -0.1),
            Vec3::new(0.5, 0.1, 0.1),
        )
        .unwrap();
        let over = Transform::new(Vec3::ZERO, Quat::from_euler_xyz(1.0, 0.0, 0.0), Vec3::ONE);
        let clamped = limit.clamp_transform(over);
        let e = clamped.rotation.to_euler_xyz();
        assert!((e.x - 0.5).abs() <= 1.0e-3);
        // A rotation already inside the range is left unchanged and reads legal.
        let inside = Transform::from_rotation(Quat::from_euler_xyz(0.3, 0.0, 0.0));
        assert!(limit.contains(inside.rotation));
        assert!(!limit.contains(over.rotation));
    }

    #[test]
    fn clamp_preserves_translation_and_scale() {
        let limit =
            JointLimit::new(BoneId::from_raw(0), bound(-0.1), bound(0.1)).unwrap();
        let t = Transform::new(
            Vec3::new(2.0, 3.0, 4.0),
            Quat::from_euler_xyz(1.0, 0.0, 0.0),
            Vec3::new(5.0, 6.0, 7.0),
        );
        let clamped = limit.clamp_transform(t);
        assert_eq!(clamped.translation, t.translation);
        assert_eq!(clamped.scale, t.scale);
    }
}
