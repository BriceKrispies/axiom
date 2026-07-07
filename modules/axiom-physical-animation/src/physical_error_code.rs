//! Stable numeric codes for the physical-animation bridge's deterministic errors.

/// A machine-stable classification of a bridge failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalErrorCode {
    /// A call into the `axiom-physics` facade failed (e.g. body capacity exceeded,
    /// invalid configuration, or a command on a non-dynamic body).
    PhysicsFailed,
    /// A call into the `axiom-animation-authoring` facade failed (e.g. an unknown
    /// plan, or sampling a missing plan).
    AuthoringFailed,
    /// An operation needed a humanoid binding that has not been built yet.
    NotBound,
    /// An operation needed the ball body that has not been attached yet.
    NoBall,
}

impl PhysicalErrorCode {
    /// The stable numeric discriminant.
    pub const fn raw(self) -> u16 {
        [
            (PhysicalErrorCode::PhysicsFailed, 1_u16),
            (PhysicalErrorCode::AuthoringFailed, 2),
            (PhysicalErrorCode::NotBound, 3),
            (PhysicalErrorCode::NoBall, 4),
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
            PhysicalErrorCode::PhysicsFailed,
            PhysicalErrorCode::AuthoringFailed,
            PhysicalErrorCode::NotBound,
            PhysicalErrorCode::NoBall,
        ];
        assert_eq!(all.map(PhysicalErrorCode::raw), [1, 2, 3, 4]);
        assert_eq!(PhysicalErrorCode::NotBound, PhysicalErrorCode::NotBound);
        assert_ne!(PhysicalErrorCode::NotBound, PhysicalErrorCode::NoBall);
    }
}
