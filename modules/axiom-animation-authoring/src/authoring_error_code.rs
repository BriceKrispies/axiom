//! Stable numeric codes for the authoring module's deterministic errors.

/// A machine-stable classification of an authoring failure. Each variant has a
/// fixed `u16` discriminant ([`AuthoringErrorCode::raw`]) so callers and replay
/// logs can assert on *which* failure occurred without depending on the
/// human-readable message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthoringErrorCode {
    /// A referenced [`crate::RigId`] does not exist.
    RigNotFound,
    /// A referenced [`crate::MotionId`] does not exist.
    MotionNotFound,
    /// A referenced [`crate::PhaseId`] does not exist for its motion.
    PhaseNotFound,
    /// A referenced [`crate::PlanId`] does not exist.
    PlanNotFound,
    /// A pose goal or constraint referenced a joint name absent from the rig.
    UnknownJoint,
    /// A pose goal or constraint referenced an effector name absent from the rig.
    UnknownEffector,
    /// A pose goal, constraint, contact, or event referenced a target name that
    /// the motion never declared.
    UnknownTarget,
    /// A phase or event carried a tick range that is empty or inverted
    /// (`end <= start`), or a tick beyond the motion's duration.
    InvalidTickRange,
    /// Two phases cover overlapping tick ranges — a phase timeline must be a set
    /// of disjoint, ordered spans.
    OverlappingPhases,
    /// An authored position or transform carried a non-finite (NaN / ±infinity)
    /// component.
    NonFiniteValue,
}

impl AuthoringErrorCode {
    /// The stable numeric discriminant.
    pub const fn raw(self) -> u16 {
        // Table-indexed to keep the mapping explicit and branch-free.
        [
            (AuthoringErrorCode::RigNotFound, 1_u16),
            (AuthoringErrorCode::MotionNotFound, 2),
            (AuthoringErrorCode::PhaseNotFound, 3),
            (AuthoringErrorCode::PlanNotFound, 4),
            (AuthoringErrorCode::UnknownJoint, 5),
            (AuthoringErrorCode::UnknownEffector, 6),
            (AuthoringErrorCode::UnknownTarget, 7),
            (AuthoringErrorCode::InvalidTickRange, 8),
            (AuthoringErrorCode::OverlappingPhases, 9),
            (AuthoringErrorCode::NonFiniteValue, 10),
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
            AuthoringErrorCode::RigNotFound,
            AuthoringErrorCode::MotionNotFound,
            AuthoringErrorCode::PhaseNotFound,
            AuthoringErrorCode::PlanNotFound,
            AuthoringErrorCode::UnknownJoint,
            AuthoringErrorCode::UnknownEffector,
            AuthoringErrorCode::UnknownTarget,
            AuthoringErrorCode::InvalidTickRange,
            AuthoringErrorCode::OverlappingPhases,
            AuthoringErrorCode::NonFiniteValue,
        ];
        assert_eq!(all.map(AuthoringErrorCode::raw), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(AuthoringErrorCode::UnknownJoint, AuthoringErrorCode::UnknownJoint);
        assert_ne!(AuthoringErrorCode::UnknownJoint, AuthoringErrorCode::UnknownEffector);
    }
}
