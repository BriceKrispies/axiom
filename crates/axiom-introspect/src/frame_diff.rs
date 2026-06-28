//! The structured delta between two recorded frames — the "what changed,
//! exactly" verb that determinism makes precise.

use crate::frame_report::FrameReport;
use crate::system_report::SystemReport;

/// A deterministic, structured delta between two [`FrameReport`]s.
///
/// Built by [`FrameDiff::between`], it records which tracked fields differ (as
/// boolean flags), the signed `runtime_step_count` delta, and the systems that
/// **newly failed** in the second frame — those that failed in `to` but did not
/// fail in `from` (joined by stable `system_id`). The two frames' identities
/// (`from_index` / `to_index`) are carried as context, not counted as "changes".
///
/// Because both inputs are exact, replay-stable reports, the diff is exact and
/// reproducible — "what changed between tick N and N+K, and which system newly
/// failed" is an answer, not an estimate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameDiff {
    from_index: u64,
    to_index: u64,
    step_count_delta: i64,
    lifecycle_changed: bool,
    skipped_changed: bool,
    viewport_changed: bool,
    timing_changed: bool,
    newly_failed_systems: Vec<SystemReport>,
}

impl FrameDiff {
    /// Compute the delta `from` → `to`.
    pub fn between(from: &FrameReport, to: &FrameReport) -> Self {
        // Newly failed = failed in `to`, and not already failing in `from`
        // (absent, or present-and-succeeded). Expressed branchlessly: keep a
        // failed-in-`to` system unless `from` held the same id *failing*.
        let newly_failed_systems = to
            .systems()
            .iter()
            .filter(|system| !system.succeeded())
            .filter(|system| {
                !from
                    .systems()
                    .iter()
                    .any(|prior| (prior.system_id() == system.system_id()) & !prior.succeeded())
            })
            .cloned()
            .collect();
        FrameDiff {
            from_index: from.engine_frame_index(),
            to_index: to.engine_frame_index(),
            step_count_delta: i64::from(to.runtime_step_count())
                - i64::from(from.runtime_step_count()),
            lifecycle_changed: from.lifecycle() != to.lifecycle(),
            skipped_changed: from.skipped() != to.skipped(),
            viewport_changed: (from.viewport_width(), from.viewport_height())
                != (to.viewport_width(), to.viewport_height()),
            timing_changed: (
                from.host_elapsed_nanos(),
                from.consumed_nanos(),
                from.retained_nanos(),
                from.fixed_step_nanos(),
            ) != (
                to.host_elapsed_nanos(),
                to.consumed_nanos(),
                to.retained_nanos(),
                to.fixed_step_nanos(),
            ),
            newly_failed_systems,
        }
    }

    /// The engine frame index of the `from` frame.
    pub const fn from_index(&self) -> u64 {
        self.from_index
    }

    /// The engine frame index of the `to` frame.
    pub const fn to_index(&self) -> u64 {
        self.to_index
    }

    /// `to.runtime_step_count - from.runtime_step_count`.
    pub const fn step_count_delta(&self) -> i64 {
        self.step_count_delta
    }

    /// Whether the lifecycle state differs.
    pub const fn lifecycle_changed(&self) -> bool {
        self.lifecycle_changed
    }

    /// Whether the skip flag differs.
    pub const fn skipped_changed(&self) -> bool {
        self.skipped_changed
    }

    /// Whether the viewport size differs.
    pub const fn viewport_changed(&self) -> bool {
        self.viewport_changed
    }

    /// Whether any of the deterministic timing values differ.
    pub const fn timing_changed(&self) -> bool {
        self.timing_changed
    }

    /// The systems that failed in `to` but did not fail in `from`.
    pub fn newly_failed_systems(&self) -> &[SystemReport] {
        &self.newly_failed_systems
    }

    /// Whether no tracked field differs between the two frames (identity indices
    /// aside) — a `from`/`to` pair with the same execution shape.
    pub fn is_unchanged(&self) -> bool {
        !self.lifecycle_changed
            & !self.skipped_changed
            & !self.viewport_changed
            & !self.timing_changed
            & (self.step_count_delta == 0)
            & self.newly_failed_systems.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    /// A copy of `base` with one serialized byte overwritten, decoded back —
    /// the test-only way to vary a single private field of a `FrameReport`.
    /// Buffer layout (see `frame_report`): schema(4) + index(8) + sequence(8) +
    /// step_count(4) + skipped(1)@24 + lifecycle(1)@25 + viewport_w(4)@26 +
    /// viewport_h(4)@30 + host_elapsed(8)@34 + ...
    fn patched(base: &FrameReport, offset: usize, value: u8) -> FrameReport {
        let mut bytes = base.to_bytes();
        bytes[offset] = value;
        FrameReport::from_bytes(&bytes).unwrap()
    }

    #[test]
    fn equal_frames_produce_an_unchanged_diff() {
        let report = FrameReport::from_frame(&fixtures::active_engine_frames(1)[0]);
        let diff = FrameDiff::between(&report, &report);
        assert!(diff.is_unchanged());
        assert_eq!(diff.step_count_delta(), 0);
        assert!(!diff.lifecycle_changed());
        assert!(!diff.skipped_changed());
        assert!(!diff.viewport_changed());
        assert!(!diff.timing_changed());
        assert!(diff.newly_failed_systems().is_empty());
        assert_eq!(diff.from_index(), diff.to_index());
    }

    #[test]
    fn different_frames_carry_distinct_identities() {
        let frames = fixtures::active_engine_frames(2);
        let from = FrameReport::from_frame(&frames[0]);
        let to = FrameReport::from_frame(&frames[1]);
        let diff = FrameDiff::between(&from, &to);
        assert!(diff.from_index() < diff.to_index());
        assert!(diff.newly_failed_systems().is_empty());
    }

    #[test]
    fn a_system_that_newly_fails_is_reported() {
        let from = FrameReport::from_frame(&fixtures::active_engine_frames(1)[0]);
        let to = FrameReport::from_frame(&fixtures::failing_engine_frame());
        let diff = FrameDiff::between(&from, &to);
        assert_eq!(diff.newly_failed_systems().len(), 1);
        assert_eq!(diff.newly_failed_systems()[0].name(), "fail");
        assert!(!diff.is_unchanged());
    }

    #[test]
    fn a_system_that_recovers_is_not_newly_failed() {
        // from failed, to (active) has no failing systems ⇒ nothing *newly*
        // failed (exercises the success-filter rejecting `to`'s clean systems
        // and the absence of a newly-failed entry).
        let from = FrameReport::from_frame(&fixtures::failing_engine_frame());
        let to = FrameReport::from_frame(&fixtures::active_engine_frames(1)[0]);
        let diff = FrameDiff::between(&from, &to);
        assert!(diff.newly_failed_systems().is_empty());
    }

    #[test]
    fn a_system_already_failing_in_both_is_not_newly_failed() {
        // Same failing frame on both sides: the system fails in `to` but also
        // failed in `from`, so it is not *newly* failed (the join's
        // already-failed arm).
        let report = FrameReport::from_frame(&fixtures::failing_engine_frame());
        let diff = FrameDiff::between(&report, &report);
        assert!(diff.newly_failed_systems().is_empty());
        assert!(diff.is_unchanged());
    }

    #[test]
    fn scalar_field_changes_are_detected() {
        let base = FrameReport::from_frame(&fixtures::active_engine_frames(1)[0]);
        // Lifecycle byte @25: Active(0) -> Hidden(1).
        let life = FrameDiff::between(&base, &patched(&base, 25, 1));
        assert!(life.lifecycle_changed());
        // Skip byte @24: false(0) -> true(1).
        let skip = FrameDiff::between(&base, &patched(&base, 24, 1));
        assert!(skip.skipped_changed());
        // Viewport width LSB @26.
        let view = FrameDiff::between(&base, &patched(&base, 26, 1));
        assert!(view.viewport_changed());
        // host_elapsed_nanos LSB @34.
        let time = FrameDiff::between(&base, &patched(&base, 34, 1));
        assert!(time.timing_changed());
    }

    #[test]
    fn step_count_delta_is_signed() {
        // Patch the step-count u32 LSB @20 to differ, both directions.
        let base = FrameReport::from_frame(&fixtures::active_engine_frames(1)[0]);
        let more = patched(&base, 20, base.runtime_step_count() as u8 + 5);
        assert!(FrameDiff::between(&base, &more).step_count_delta() > 0);
        assert!(FrameDiff::between(&more, &base).step_count_delta() < 0);
    }
}
