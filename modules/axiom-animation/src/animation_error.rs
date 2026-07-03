//! The animation module's deterministic error value.

use axiom_math::MathError;

use crate::animation_error_code::AnimationErrorCode;

/// A deterministic animation-module error. Identity is `(code, math-cause)`:
/// two errors with the same [`AnimationErrorCode`] and the same optionally-
/// wrapped [`MathError`] compare equal regardless of the static human message,
/// so assertions stay machine-stable across builds and replays. No animation
/// operation panics for a validation failure — it returns one of these instead.
#[derive(Debug, Clone, Copy)]
pub struct AnimationError {
    code: AnimationErrorCode,
    message: &'static str,
    math: Option<MathError>,
}

impl AnimationError {
    /// An animation-only error without a wrapped math cause.
    pub const fn new(code: AnimationErrorCode, message: &'static str) -> Self {
        AnimationError {
            code,
            message,
            math: None,
        }
    }

    /// An animation error wrapping a math failure (e.g. a rotation that could
    /// not be normalized during interpolation).
    pub const fn with_math(
        code: AnimationErrorCode,
        message: &'static str,
        cause: MathError,
    ) -> Self {
        AnimationError {
            code,
            message,
            math: Some(cause),
        }
    }

    /// A referenced skeleton id does not exist.
    pub const fn skeleton_not_found(message: &'static str) -> Self {
        AnimationError::new(AnimationErrorCode::SkeletonNotFound, message)
    }

    /// A referenced clip id does not exist.
    pub const fn clip_not_found(message: &'static str) -> Self {
        AnimationError::new(AnimationErrorCode::ClipNotFound, message)
    }

    /// A referenced bone id is out of range for its skeleton.
    pub const fn bone_not_found(message: &'static str) -> Self {
        AnimationError::new(AnimationErrorCode::BoneNotFound, message)
    }

    /// A clip track was supplied with no keyframes.
    pub const fn empty_track(message: &'static str) -> Self {
        AnimationError::new(AnimationErrorCode::EmptyTrack, message)
    }

    /// A clip track's keyframe times were not strictly increasing.
    pub const fn non_monotonic_keyframes(message: &'static str) -> Self {
        AnimationError::new(AnimationErrorCode::NonMonotonicKeyframes, message)
    }

    /// Two poses being blended cover a different number of bones.
    pub const fn pose_length_mismatch(message: &'static str) -> Self {
        AnimationError::new(AnimationErrorCode::PoseLengthMismatch, message)
    }

    /// An interpolation produced a non-finite / zero-length rotation. Wraps the
    /// underlying [`MathError`] from the failed quaternion normalization.
    pub const fn non_finite_interpolation(message: &'static str, cause: MathError) -> Self {
        AnimationError::with_math(AnimationErrorCode::NonFiniteInterpolation, message, cause)
    }

    /// A joint limit had a min bound greater than its max bound on some axis.
    pub const fn invalid_joint_limit(message: &'static str) -> Self {
        AnimationError::new(AnimationErrorCode::InvalidJointLimit, message)
    }

    /// The stable error classification.
    pub const fn code(&self) -> AnimationErrorCode {
        self.code
    }

    /// The static human-readable message (never part of identity).
    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// The wrapped math cause, if any.
    pub const fn math(&self) -> Option<MathError> {
        self.math
    }

    /// The stable numeric error code — inspect *which* error occurred without
    /// naming the internal code enum.
    pub const fn raw_code(&self) -> u16 {
        self.code.raw()
    }
}

/// Equality on machine identity only (code + math cause), never the message.
impl PartialEq for AnimationError {
    fn eq(&self, other: &Self) -> bool {
        (self.code == other.code) & (self.math == other.math)
    }
}

impl Eq for AnimationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::MathErrorCode;

    fn math_cause() -> MathError {
        MathError::normalize_zero_length("synthetic")
    }

    #[test]
    fn identity_ignores_message_but_keeps_code() {
        let a = AnimationError::bone_not_found("x");
        let b = AnimationError::bone_not_found("totally different");
        assert_eq!(a, b);
        assert_eq!(a.code(), AnimationErrorCode::BoneNotFound);
        assert_eq!(a.message(), "x");
    }

    #[test]
    fn different_codes_are_not_equal() {
        assert_ne!(
            AnimationError::skeleton_not_found(""),
            AnimationError::clip_not_found("")
        );
    }

    #[test]
    fn shorthand_constructors_use_their_codes() {
        assert_eq!(
            AnimationError::skeleton_not_found("").code(),
            AnimationErrorCode::SkeletonNotFound
        );
        assert_eq!(
            AnimationError::clip_not_found("").code(),
            AnimationErrorCode::ClipNotFound
        );
        assert_eq!(
            AnimationError::bone_not_found("").code(),
            AnimationErrorCode::BoneNotFound
        );
        assert_eq!(
            AnimationError::empty_track("").code(),
            AnimationErrorCode::EmptyTrack
        );
        assert_eq!(
            AnimationError::non_monotonic_keyframes("").code(),
            AnimationErrorCode::NonMonotonicKeyframes
        );
        assert_eq!(
            AnimationError::pose_length_mismatch("").code(),
            AnimationErrorCode::PoseLengthMismatch
        );
        assert_eq!(
            AnimationError::invalid_joint_limit("").code(),
            AnimationErrorCode::InvalidJointLimit
        );
        assert_eq!(AnimationError::skeleton_not_found("").raw_code(), 1);
    }

    #[test]
    fn wraps_math_cause_and_distinguishes_from_bare() {
        let wrapped =
            AnimationError::non_finite_interpolation("bad rotation", math_cause());
        assert_eq!(wrapped.code(), AnimationErrorCode::NonFiniteInterpolation);
        assert_eq!(wrapped.math().unwrap().code(), MathErrorCode::NormalizeZeroLength);
        let bare = AnimationError::new(AnimationErrorCode::NonFiniteInterpolation, "bad rotation");
        assert_ne!(bare, wrapped);
        assert_eq!(bare.math(), None);
    }
}
