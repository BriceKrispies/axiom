//! Stable numeric codes for the animation module's deterministic errors.

/// A machine-stable classification of an animation failure. Each variant has a
/// fixed `u16` discriminant ([`AnimationErrorCode::raw`]) so external callers
/// and replay logs can assert on *which* failure occurred without depending on
/// the human-readable message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationErrorCode {
    /// A referenced [`crate::SkeletonId`] does not exist.
    SkeletonNotFound,
    /// A referenced [`crate::ClipId`] does not exist.
    ClipNotFound,
    /// A referenced [`crate::BoneId`] is out of range for its skeleton.
    BoneNotFound,
    /// A clip track was given with no keyframes.
    EmptyTrack,
    /// A clip track's keyframe times were not strictly increasing.
    NonMonotonicKeyframes,
    /// Two poses being blended cover a different number of bones.
    PoseLengthMismatch,
    /// An interpolation produced a non-finite / zero-length rotation.
    NonFiniteInterpolation,
    /// A joint limit was given a min bound greater than its max bound.
    InvalidJointLimit,
    /// A serialized skeleton or clip could not be decoded from its bytes.
    MalformedData,
}

impl AnimationErrorCode {
    /// The stable numeric discriminant.
    pub const fn raw(self) -> u16 {
        // Table-indexed to keep the mapping explicit and branch-free.
        [
            (AnimationErrorCode::SkeletonNotFound, 1_u16),
            (AnimationErrorCode::ClipNotFound, 2),
            (AnimationErrorCode::BoneNotFound, 3),
            (AnimationErrorCode::EmptyTrack, 4),
            (AnimationErrorCode::NonMonotonicKeyframes, 5),
            (AnimationErrorCode::PoseLengthMismatch, 6),
            (AnimationErrorCode::NonFiniteInterpolation, 7),
            (AnimationErrorCode::InvalidJointLimit, 8),
            (AnimationErrorCode::MalformedData, 9),
        ][self as usize]
            .1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_codes_are_stable_and_distinct() {
        let all = [
            AnimationErrorCode::SkeletonNotFound,
            AnimationErrorCode::ClipNotFound,
            AnimationErrorCode::BoneNotFound,
            AnimationErrorCode::EmptyTrack,
            AnimationErrorCode::NonMonotonicKeyframes,
            AnimationErrorCode::PoseLengthMismatch,
            AnimationErrorCode::NonFiniteInterpolation,
            AnimationErrorCode::InvalidJointLimit,
            AnimationErrorCode::MalformedData,
        ];
        // Each maps to its documented discriminant, in order 1..=9.
        assert_eq!(
            all.map(AnimationErrorCode::raw),
            [1, 2, 3, 4, 5, 6, 7, 8, 9]
        );
        // Distinct codes and matching equality.
        assert_eq!(
            AnimationErrorCode::BoneNotFound,
            AnimationErrorCode::BoneNotFound
        );
        assert_ne!(
            AnimationErrorCode::BoneNotFound,
            AnimationErrorCode::ClipNotFound
        );
    }
}
