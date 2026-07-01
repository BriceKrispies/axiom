//! The deterministic outcome status attached to a [`crate::GpuSubmissionReport`].

/// What actually happened to a submission, beyond the recorded command shape.
///
/// The per-kind counts in a [`crate::GpuSubmissionReport`] describe *what the
/// submission contained* (identical for both backends, because
/// `GpuSubmission` is the stable contract). This status describes *what the
/// backend did with it* — which is the only place the recording and live
/// backends differ today.
///
/// No variant claims pixels were presented. A future live pass will add a
/// `Presented`-style variant once a real surface/device binding exists; until
/// then [`GpuSubmissionStatus::presented`] is always `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuSubmissionStatus {
    /// The recording backend deterministically captured the submission.
    Recorded,
    /// A live backend accepted the submission, but no presentation target /
    /// surface is bound, so nothing was presented.
    LiveNotBound,
    /// A live backend has a validated host presentation request, but no real
    /// device/surface is initialized, so nothing was presented.
    LiveNotInitialized,
}

impl GpuSubmissionStatus {
    /// Whether real, visible presentation occurred. Always `false` in this
    /// pass — neither backend touches a GPU.
    pub const fn presented(self) -> bool {
        false
    }

    /// Whether this status came from the deterministic recording backend.
    pub const fn is_recorded(self) -> bool {
        (self as u8) == (GpuSubmissionStatus::Recorded as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(
            GpuSubmissionStatus::Recorded,
            GpuSubmissionStatus::LiveNotBound
        );
        assert_ne!(
            GpuSubmissionStatus::LiveNotBound,
            GpuSubmissionStatus::LiveNotInitialized
        );
        assert_ne!(
            GpuSubmissionStatus::Recorded,
            GpuSubmissionStatus::LiveNotInitialized
        );
    }

    #[test]
    fn no_status_claims_presentation() {
        assert!(!GpuSubmissionStatus::Recorded.presented());
        assert!(!GpuSubmissionStatus::LiveNotBound.presented());
        assert!(!GpuSubmissionStatus::LiveNotInitialized.presented());
    }

    #[test]
    fn only_recorded_is_recorded() {
        assert!(GpuSubmissionStatus::Recorded.is_recorded());
        assert!(!GpuSubmissionStatus::LiveNotBound.is_recorded());
    }
}
