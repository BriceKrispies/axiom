//! Deterministic blending of two poses.

use axiom_math::Transform;

use crate::animation_error::AnimationError;
use crate::animation_result::AnimationResult;
use crate::interpolate::lerp_transform;
use crate::pose::Pose;

/// Blend `a -> b` per bone at `factor` (callers pass a value already clamped to
/// `[0, 1]`). Fails with `PoseLengthMismatch` if the poses cover different bone
/// counts, or propagates an interpolation failure. Exact at the endpoints.
pub(crate) fn blend_poses(a: &Pose, b: &Pose, factor: f32) -> AnimationResult<Pose> {
    (a.bone_count() == b.bone_count())
        .then_some(())
        .ok_or_else(|| {
            AnimationError::pose_length_mismatch("blended poses cover different bone counts")
        })
        .and_then(|()| {
            a.locals()
                .iter()
                .zip(b.locals().iter())
                .map(|(&la, &lb)| lerp_transform(la, lb, factor))
                .collect::<AnimationResult<Vec<Transform>>>()
        })
        .map(Pose::from_locals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_error_code::AnimationErrorCode;
    use crate::ids::BoneId;
    use axiom_math::{ApproxEq, Epsilon, Vec3};

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    fn pose(x: f32) -> Pose {
        Pose::from_locals(vec![Transform::from_translation(Vec3::new(x, 0.0, 0.0))])
    }

    #[test]
    fn blend_midpoint_averages_and_is_deterministic() {
        let mid = blend_poses(&pose(0.0), &pose(4.0), 0.5).unwrap();
        assert!(mid
            .local(BoneId::from_raw(0))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(2.0, 0.0, 0.0), eps()));
        assert_eq!(blend_poses(&pose(0.0), &pose(4.0), 0.5).unwrap(), mid);
    }

    #[test]
    fn blend_endpoints_return_inputs() {
        assert_eq!(blend_poses(&pose(1.0), &pose(9.0), 0.0).unwrap(), pose(1.0));
        assert_eq!(blend_poses(&pose(1.0), &pose(9.0), 1.0).unwrap(), pose(9.0));
    }

    #[test]
    fn blend_length_mismatch_is_rejected() {
        let a = pose(0.0);
        let b = Pose::from_locals(vec![Transform::IDENTITY, Transform::IDENTITY]);
        assert_eq!(
            blend_poses(&a, &b, 0.5).unwrap_err().code(),
            AnimationErrorCode::PoseLengthMismatch
        );
    }
}
