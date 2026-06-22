//! [`DeterminismReport`] and the deterministic timeline comparison that produces
//! it.
//!
//! Comparison is **byte equality first, hashes second**. Two captures are
//! compared in a fixed order — identity (`tick`), then the four opaque artifact
//! byte arrays (input → runtime → state → render), then the combined
//! `final_hash` — and the *first* divergence in that order is reported: which
//! frame, which artifact, and (for a byte artifact) the first differing byte
//! index. The comparison is pure and allocation-light, and never interprets the
//! bytes it diffs.

use axiom_kernel::{FrameIndex, KernelResult};

use crate::artifact_kind::ArtifactKind;
use crate::error::{timeline_frame_index_mismatch, timeline_length_mismatch};
use crate::frame_capture::FrameCapture;
use crate::frame_timeline::FrameTimeline;

/// The deterministic outcome of comparing an original timeline against a replay.
///
/// When `matched` is true every other field is empty/zero. When it is false the
/// `first_mismatching_*` fields locate the first divergence and the two hashes
/// are the diverging frame's `final_hash` from each timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterminismReport {
    matched: bool,
    first_mismatching_frame: Option<FrameIndex>,
    first_mismatching_artifact: Option<ArtifactKind>,
    first_mismatching_byte_index: Option<usize>,
    original_hash: u64,
    replay_hash: u64,
}

impl DeterminismReport {
    /// A report stating the two timelines were byte-identical.
    fn all_matched() -> Self {
        DeterminismReport {
            matched: true,
            first_mismatching_frame: None,
            first_mismatching_artifact: None,
            first_mismatching_byte_index: None,
            original_hash: 0,
            replay_hash: 0,
        }
    }

    /// A report locating the first divergence between the timelines.
    fn mismatch(
        frame: FrameIndex,
        artifact: ArtifactKind,
        byte_index: Option<usize>,
        original_hash: u64,
        replay_hash: u64,
    ) -> Self {
        DeterminismReport {
            matched: false,
            first_mismatching_frame: Some(frame),
            first_mismatching_artifact: Some(artifact),
            first_mismatching_byte_index: byte_index,
            original_hash,
            replay_hash,
        }
    }

    /// Whether the two timelines were byte-identical.
    pub fn matched(&self) -> bool {
        self.matched
    }

    /// The first frame at which the timelines diverged, if any.
    pub fn first_mismatching_frame(&self) -> Option<FrameIndex> {
        self.first_mismatching_frame
    }

    /// The first artifact within that frame that diverged, if any.
    pub fn first_mismatching_artifact(&self) -> Option<ArtifactKind> {
        self.first_mismatching_artifact
    }

    /// The first differing byte index within the diverging artifact, if the
    /// divergence was a byte-array difference (hash-only divergences report none).
    pub fn first_mismatching_byte_index(&self) -> Option<usize> {
        self.first_mismatching_byte_index
    }

    /// The diverging frame's `final_hash` in the original timeline.
    pub fn original_hash(&self) -> u64 {
        self.original_hash
    }

    /// The diverging frame's `final_hash` in the replay timeline.
    pub fn replay_hash(&self) -> u64 {
        self.replay_hash
    }
}

/// The first index at which two byte slices differ. If all shared-length bytes
/// match but the lengths differ, the shorter length is the first divergence
/// (the index where one slice ran out). Returns `None` when the slices are equal.
fn first_byte_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter()
        .zip(b.iter())
        .position(|(x, y)| x != y)
        .or_else(|| (a.len() != b.len()).then_some(a.len().min(b.len())))
}

/// Compare two aligned captures (same frame index, already verified by the
/// caller). Returns the first divergence in the fixed order, or `None` if the
/// captures are byte-identical.
///
/// There is deliberately no `final_hash` comparison arm: `final_hash` is a pure
/// function of the frame index (verified equal by the caller), the tick, and the
/// four artifact hashes — all of which are compared above. If every one of those
/// matches, `final_hash` is necessarily equal, so a `final_hash` arm could never
/// be the first divergence. The hashes survive only as the report's diagnostic
/// `original_hash` / `replay_hash`.
fn compare_captures(orig: &FrameCapture, rep: &FrameCapture) -> Option<DeterminismReport> {
    let divergence = [
        (orig.tick() != rep.tick()).then_some((ArtifactKind::Final, None)),
        first_byte_diff(orig.input_bytes(), rep.input_bytes())
            .map(|i| (ArtifactKind::Input, Some(i))),
        first_byte_diff(orig.runtime_bytes(), rep.runtime_bytes())
            .map(|i| (ArtifactKind::Runtime, Some(i))),
        first_byte_diff(orig.state_bytes(), rep.state_bytes())
            .map(|i| (ArtifactKind::State, Some(i))),
        first_byte_diff(orig.render_bytes(), rep.render_bytes())
            .map(|i| (ArtifactKind::Render, Some(i))),
    ]
    .into_iter()
    .flatten()
    .next();
    divergence.map(|(artifact, byte_index)| {
        DeterminismReport::mismatch(
            orig.frame_index(),
            artifact,
            byte_index,
            orig.final_hash(),
            rep.final_hash(),
        )
    })
}

/// Compare an original timeline against a replayed one and report the first
/// divergence. The timelines must have equal length and equal aligned frame
/// indices; otherwise a deterministic structural error is returned (rather than a
/// report), because a frame-shape mismatch is not an artifact divergence.
pub(crate) fn compare_timelines(
    original: &FrameTimeline,
    replay: &FrameTimeline,
) -> KernelResult<DeterminismReport> {
    (original.len() == replay.len())
        .then_some(())
        .ok_or_else(timeline_length_mismatch)
        .and_then(|()| {
            original
                .captures()
                .iter()
                .zip(replay.captures().iter())
                .all(|(a, b)| a.frame_index() == b.frame_index())
                .then_some(())
                .ok_or_else(timeline_frame_index_mismatch)
        })
        .map(|()| {
            original
                .captures()
                .iter()
                .zip(replay.captures().iter())
                .filter_map(|(a, b)| compare_captures(a, b))
                .next()
                .unwrap_or_else(DeterminismReport::all_matched)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Tick;

    fn cap(frame: u64, tick: u64, input: &[u8], render: &[u8]) -> FrameCapture {
        FrameCapture::new(
            FrameIndex::new(frame),
            Tick::new(tick),
            input.to_vec(),
            Vec::new(),
            Vec::new(),
            render.to_vec(),
        )
    }

    fn timeline_of(caps: &[FrameCapture]) -> FrameTimeline {
        let mut t = FrameTimeline::new(100, 1 << 20).unwrap();
        caps.iter().for_each(|c| t.push_frame(c.clone()).unwrap());
        t
    }

    #[test]
    fn matched_report_has_no_divergence() {
        let r = DeterminismReport::all_matched();
        assert!(r.matched());
        assert_eq!(r.first_mismatching_frame(), None);
        assert_eq!(r.first_mismatching_artifact(), None);
        assert_eq!(r.first_mismatching_byte_index(), None);
        assert_eq!(r.original_hash(), 0);
        assert_eq!(r.replay_hash(), 0);
    }

    #[test]
    fn identical_timelines_match() {
        let a = timeline_of(&[cap(0, 0, b"x", b"y"), cap(1, 10, b"a", b"b")]);
        let b = a.clone();
        let r = compare_timelines(&a, &b).unwrap();
        assert!(r.matched());
    }

    #[test]
    fn first_byte_diff_locates_byte_or_length() {
        assert_eq!(first_byte_diff(b"abc", b"abc"), None);
        assert_eq!(first_byte_diff(b"abc", b"aXc"), Some(1));
        // Shared bytes match, lengths differ → shorter length is the divergence.
        assert_eq!(first_byte_diff(b"ab", b"abc"), Some(2));
        assert_eq!(first_byte_diff(b"abc", b"ab"), Some(2));
    }

    #[test]
    fn input_byte_divergence_is_reported_with_index() {
        let a = timeline_of(&[cap(0, 0, b"hello", b"r")]);
        let b = timeline_of(&[cap(0, 0, b"heLlo", b"r")]);
        let r = compare_timelines(&a, &b).unwrap();
        assert!(!r.matched());
        assert_eq!(r.first_mismatching_frame(), Some(FrameIndex::new(0)));
        assert_eq!(r.first_mismatching_artifact(), Some(ArtifactKind::Input));
        assert_eq!(r.first_mismatching_byte_index(), Some(2));
        assert_ne!(r.original_hash(), r.replay_hash());
    }

    #[test]
    fn render_divergence_is_reported_after_matching_earlier_artifacts() {
        let a = timeline_of(&[cap(0, 0, b"same", b"render-a")]);
        let b = timeline_of(&[cap(0, 0, b"same", b"render-b")]);
        let r = compare_timelines(&a, &b).unwrap();
        assert_eq!(r.first_mismatching_artifact(), Some(ArtifactKind::Render));
        assert_eq!(r.first_mismatching_byte_index(), Some(7));
    }

    #[test]
    fn tick_divergence_is_reported_as_final_with_no_byte_index() {
        let a = timeline_of(&[cap(0, 10, b"x", b"y")]);
        let b = timeline_of(&[cap(0, 11, b"x", b"y")]);
        let r = compare_timelines(&a, &b).unwrap();
        assert_eq!(r.first_mismatching_artifact(), Some(ArtifactKind::Final));
        assert_eq!(r.first_mismatching_byte_index(), None);
        assert_eq!(r.first_mismatching_frame(), Some(FrameIndex::new(0)));
    }

    #[test]
    fn runtime_and_state_byte_divergences_are_located() {
        let base = FrameCapture::new(
            FrameIndex::new(0),
            Tick::new(0),
            b"i".to_vec(),
            b"run".to_vec(),
            b"st".to_vec(),
            b"r".to_vec(),
        );
        let diff_runtime = FrameCapture::new(
            FrameIndex::new(0),
            Tick::new(0),
            b"i".to_vec(),
            b"rXn".to_vec(),
            b"st".to_vec(),
            b"r".to_vec(),
        );
        let diff_state = FrameCapture::new(
            FrameIndex::new(0),
            Tick::new(0),
            b"i".to_vec(),
            b"run".to_vec(),
            b"sY".to_vec(),
            b"r".to_vec(),
        );
        let original = timeline_of(&[base]);
        let r = compare_timelines(&original, &timeline_of(&[diff_runtime])).unwrap();
        assert_eq!(r.first_mismatching_artifact(), Some(ArtifactKind::Runtime));
        assert_eq!(r.first_mismatching_byte_index(), Some(1));
        let s = compare_timelines(&original, &timeline_of(&[diff_state])).unwrap();
        assert_eq!(s.first_mismatching_artifact(), Some(ArtifactKind::State));
        assert_eq!(s.first_mismatching_byte_index(), Some(1));
    }

    #[test]
    fn divergence_in_a_later_frame_is_found() {
        let a = timeline_of(&[cap(0, 0, b"x", b"y"), cap(1, 10, b"a", b"b")]);
        let b = timeline_of(&[cap(0, 0, b"x", b"y"), cap(1, 10, b"a", b"B")]);
        let r = compare_timelines(&a, &b).unwrap();
        assert_eq!(r.first_mismatching_frame(), Some(FrameIndex::new(1)));
        assert_eq!(r.first_mismatching_artifact(), Some(ArtifactKind::Render));
    }

    #[test]
    fn length_mismatch_is_a_structural_error() {
        let a = timeline_of(&[cap(0, 0, b"x", b"y")]);
        let b = timeline_of(&[cap(0, 0, b"x", b"y"), cap(1, 10, b"a", b"b")]);
        assert!(compare_timelines(&a, &b).is_err());
    }

    #[test]
    fn aligned_frame_index_mismatch_is_a_structural_error() {
        let a = timeline_of(&[cap(0, 0, b"x", b"y")]);
        let b = timeline_of(&[cap(5, 0, b"x", b"y")]);
        assert!(compare_timelines(&a, &b).is_err());
    }

    #[test]
    fn report_is_copy_and_debug() {
        let r = DeterminismReport::all_matched();
        let c = r;
        assert_eq!(r, c);
        assert!(format!("{r:?}").contains("DeterminismReport"));
    }
}
