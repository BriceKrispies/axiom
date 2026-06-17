//! Deterministic report of one GPU submission.

use crate::gpu_command::GpuCommand;
use crate::gpu_submission_status::GpuSubmissionStatus;

/// The deterministic record `WebGpuApi::submit` returns.
///
/// Every command the app pushed into the submission is captured here, plus a
/// per-kind counter that lets test code assert on submission shape without
/// walking the command list. These describe *what the submission contained*
/// and are identical regardless of backend, because `GpuSubmission` is the
/// stable input contract.
///
/// The [`GpuSubmissionStatus`] describes *what the backend did with it*: the
/// recording backend reports `Recorded`; a live backend reports a
/// deterministic not-bound / not-initialized status. No status claims pixels
/// were presented in this pass — see `ARCHITECTURE.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuSubmissionReport {
    submitted_commands: Vec<GpuCommand>,
    target_width: u32,
    target_height: u32,
    clear_count: u32,
    draw_count: u32,
    present_count: u32,
    status: GpuSubmissionStatus,
}

impl GpuSubmissionReport {
    pub(crate) fn new(
        submitted_commands: Vec<GpuCommand>,
        target_width: u32,
        target_height: u32,
        status: GpuSubmissionStatus,
    ) -> Self {
        // Branchless tally: map each command to its kind code, then count the
        // occurrences of each counted kind. Equality on the stable kind codes
        // replaces the per-variant `match` arms, and `filter().count()` walks
        // the sequence without an explicit `for`/`if`.
        let kinds = || submitted_commands.iter().map(GpuCommand::kind_code);
        let clear_count = kinds()
            .filter(|code| *code == GpuCommand::KIND_CLEAR_FRAME)
            .count() as u32;
        let draw_count = kinds()
            .filter(|code| *code == GpuCommand::KIND_DRAW_INDEXED)
            .count() as u32;
        let present_count = kinds()
            .filter(|code| *code == GpuCommand::KIND_PRESENT)
            .count() as u32;
        GpuSubmissionReport {
            submitted_commands,
            target_width,
            target_height,
            clear_count,
            draw_count,
            present_count,
            status,
        }
    }

    pub fn submitted_commands(&self) -> &[GpuCommand] {
        &self.submitted_commands
    }

    pub const fn submitted_command_count(&self) -> usize {
        self.submitted_commands.len()
    }

    pub const fn target_width(&self) -> u32 {
        self.target_width
    }

    pub const fn target_height(&self) -> u32 {
        self.target_height
    }

    pub const fn clear_count(&self) -> u32 {
        self.clear_count
    }

    pub const fn draw_count(&self) -> u32 {
        self.draw_count
    }

    pub const fn present_count(&self) -> u32 {
        self.present_count
    }

    /// The deterministic backend outcome for this submission.
    pub const fn status(&self) -> GpuSubmissionStatus {
        self.status
    }

    /// Whether real, visible presentation occurred. Always `false` in this
    /// pass — neither backend touches a GPU. Exposed as a primitive so
    /// downstream crates can assert "no pixels were claimed" without naming
    /// the module-internal status enum.
    pub const fn presented(&self) -> bool {
        self.status.presented()
    }

    /// Whether this report came from the deterministic recording backend.
    pub const fn is_recorded(&self) -> bool {
        // Fieldless-enum predicate over the carried status: compare integer
        // discriminants directly rather than branching on a `matches!` arm.
        (self.status as u8) == (GpuSubmissionStatus::Recorded as u8)
    }

    /// Whether a live backend accepted the submission but had no
    /// target/surface bound.
    pub const fn is_live_not_bound(&self) -> bool {
        (self.status as u8) == (GpuSubmissionStatus::LiveNotBound as u8)
    }

    /// Whether a live backend had a validated presentation request but no
    /// initialized device/surface.
    pub const fn is_live_not_initialized(&self) -> bool {
        (self.status as u8) == (GpuSubmissionStatus::LiveNotInitialized as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_records_per_kind_counts() {
        let r = GpuSubmissionReport::new(
            vec![
                GpuCommand::ClearFrame {
                    color: [0.0, 0.0, 0.0, 1.0],
                },
                GpuCommand::DrawIndexed {
                    index_count: 36,
                    world: axiom_math::Mat4::IDENTITY,
                },
                GpuCommand::DrawIndexed {
                    index_count: 6,
                    world: axiom_math::Mat4::IDENTITY,
                },
                GpuCommand::Present,
            ],
            800,
            600,
            GpuSubmissionStatus::Recorded,
        );
        assert_eq!(r.clear_count(), 1);
        assert_eq!(r.draw_count(), 2);
        assert_eq!(r.present_count(), 1);
        assert_eq!(r.submitted_command_count(), 4);
    }

    #[test]
    fn report_counts_distinguish_from_zero_and_one() {
        // Two of every counted kind so each count is 2 — distinct from the
        // mutant constants 0 and 1 for clear_count / present_count (and
        // draw_count for good measure).
        let r = GpuSubmissionReport::new(
            vec![
                GpuCommand::ClearFrame {
                    color: [0.0, 0.0, 0.0, 1.0],
                },
                GpuCommand::ClearFrame {
                    color: [1.0, 1.0, 1.0, 1.0],
                },
                GpuCommand::DrawIndexed {
                    index_count: 36,
                    world: axiom_math::Mat4::IDENTITY,
                },
                GpuCommand::DrawIndexed {
                    index_count: 6,
                    world: axiom_math::Mat4::IDENTITY,
                },
                GpuCommand::Present,
                GpuCommand::Present,
            ],
            1,
            1,
            GpuSubmissionStatus::Recorded,
        );
        assert_eq!(r.clear_count(), 2);
        assert_eq!(r.draw_count(), 2);
        assert_eq!(r.present_count(), 2);
    }

    #[test]
    fn report_round_trips_target_dimensions() {
        let r = GpuSubmissionReport::new(vec![], 1920, 1080, GpuSubmissionStatus::Recorded);
        assert_eq!(r.target_width(), 1920);
        assert_eq!(r.target_height(), 1080);
    }

    #[test]
    fn equal_inputs_produce_equal_reports() {
        let a = GpuSubmissionReport::new(
            vec![GpuCommand::Present],
            1,
            1,
            GpuSubmissionStatus::Recorded,
        );
        let b = GpuSubmissionReport::new(
            vec![GpuCommand::Present],
            1,
            1,
            GpuSubmissionStatus::Recorded,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn status_is_carried_and_recording_claims_no_pixels() {
        let r = GpuSubmissionReport::new(vec![], 1, 1, GpuSubmissionStatus::Recorded);
        assert_eq!(r.status(), GpuSubmissionStatus::Recorded);
        assert!(r.is_recorded());
        assert!(!r.presented());
    }

    #[test]
    fn live_status_reports_are_not_presented() {
        let unbound = GpuSubmissionReport::new(vec![], 1, 1, GpuSubmissionStatus::LiveNotBound);
        assert!(unbound.is_live_not_bound());
        assert!(!unbound.presented());
        let pending =
            GpuSubmissionReport::new(vec![], 1, 1, GpuSubmissionStatus::LiveNotInitialized);
        assert!(pending.is_live_not_initialized());
        assert!(!pending.presented());
    }
}

#[cfg(test)]
mod cov {
    use super::*;

    #[test]
    fn status_predicates_return_false_on_mismatch() {
        // A `LiveNotBound` report is none of the other classifications, so
        // each `matches!` predicate exercises its non-matching arm.
        let r = GpuSubmissionReport::new(vec![], 1, 1, GpuSubmissionStatus::LiveNotBound);
        assert!(!r.is_recorded());
        assert!(!r.is_live_not_initialized());

        // A `Recorded` report exercises the non-matching arm of the live
        // predicates.
        let rec = GpuSubmissionReport::new(vec![], 1, 1, GpuSubmissionStatus::Recorded);
        assert!(!rec.is_live_not_bound());
        assert!(!rec.is_live_not_initialized());
    }
}
