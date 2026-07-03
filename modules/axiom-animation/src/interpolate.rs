//! Deterministic transform interpolation shared by clip sampling and blending.
//!
//! Translation and scale interpolate componentwise; rotation uses the math
//! layer's shortest-arc [`axiom_math::Quat::nlerp`]. The only failure is a
//! non-finite / zero-length rotation, surfaced as a deterministic
//! [`AnimationError`] (never a panic).

use axiom_math::{Transform, Vec3};

use crate::animation_error::AnimationError;
use crate::animation_result::AnimationResult;

/// Linearly interpolate `a -> b` at `factor` (callers pass a value already
/// clamped to `[0, 1]`). Exact at the endpoints: `factor == 0.0` returns `a`,
/// `factor == 1.0` returns `b`.
pub(crate) fn lerp_transform(
    a: Transform,
    b: Transform,
    factor: f32,
) -> AnimationResult<Transform> {
    let translation = lerp_vec3(a.translation, b.translation, factor);
    let scale = lerp_vec3(a.scale, b.scale, factor);
    a.rotation
        .nlerp(b.rotation, factor)
        .map(|rotation| Transform::new(translation, rotation, scale))
        .map_err(|cause| {
            AnimationError::non_finite_interpolation(
                "rotation interpolation produced a non-finite quaternion",
                cause,
            )
        })
}

/// Componentwise `a + (b - a) * factor`.
fn lerp_vec3(a: Vec3, b: Vec3, factor: f32) -> Vec3 {
    a.add(b.subtract(a).mul_scalar(factor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{ApproxEq, Epsilon, Quat};

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    fn xf(x: f32) -> Transform {
        Transform::from_translation(Vec3::new(x, 0.0, 0.0))
    }

    #[test]
    fn endpoints_are_exact() {
        let a = xf(0.0);
        let b = xf(10.0);
        assert!(lerp_transform(a, b, 0.0)
            .unwrap()
            .translation
            .approx_eq(&a.translation, eps()));
        assert!(lerp_transform(a, b, 1.0)
            .unwrap()
            .translation
            .approx_eq(&b.translation, eps()));
    }

    #[test]
    fn midpoint_interpolates_translation_and_scale() {
        let a = Transform::new(Vec3::new(0.0, 0.0, 0.0), Quat::IDENTITY, Vec3::new(1.0, 1.0, 1.0));
        let b = Transform::new(Vec3::new(4.0, 0.0, 0.0), Quat::IDENTITY, Vec3::new(3.0, 3.0, 3.0));
        let mid = lerp_transform(a, b, 0.5).unwrap();
        assert!(mid.translation.approx_eq(&Vec3::new(2.0, 0.0, 0.0), eps()));
        assert!(mid.scale.approx_eq(&Vec3::new(2.0, 2.0, 2.0), eps()));
    }

    #[test]
    fn degenerate_rotation_fails_deterministically() {
        let a = Transform::new(Vec3::ZERO, Quat::new(0.0, 0.0, 0.0, 0.0), Vec3::ONE);
        let b = Transform::new(Vec3::ZERO, Quat::new(0.0, 0.0, 0.0, 0.0), Vec3::ONE);
        assert!(lerp_transform(a, b, 0.5).is_err());
    }
}
