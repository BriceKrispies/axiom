//! [`RecordingApi`] — the single public facade of `axiom-recording`.
//!
//! It owns a bounded [`FrameTimeline`] of opaque per-frame captures and a small
//! playback cursor (live vs. scrubbing a selected frame). Everything the module
//! offers — recording a frame, querying memory/bounds, fetching a capture,
//! scrubbing/stepping, and proving replay determinism — is reached through this
//! type. The captures, timeline, mode, and report types are returned *opaquely*:
//! callers read them through inference and their public accessors, but only
//! `RecordingApi` is named in the module's public surface.
//!
//! The cursor is stored as a `live` flag plus a `selected` frame index rather
//! than as a [`TimelineMode`] value, so the module never has to destructure the
//! mode enum (which would require a branch) — `mode()` *constructs* the enum on
//! demand instead.

use axiom_kernel::{FrameIndex, KernelResult, Tick};

use crate::determinism_report::{compare_timelines, DeterminismReport};
use crate::frame_capture::FrameCapture;
use crate::frame_timeline::FrameTimeline;
use crate::timeline_mode::TimelineMode;

/// Browser-safe default: at most 3,600 frames (~1 minute at 60 fps).
const BROWSER_MAX_FRAMES: usize = 3_600;
/// Browser-safe default: at most 64 MiB of retained captures.
const BROWSER_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Native default: at most 10,000 frames.
const NATIVE_MAX_FRAMES: usize = 10_000;
/// Native default: at most 128 MiB of retained captures.
const NATIVE_MAX_BYTES: usize = 128 * 1024 * 1024;

/// The deterministic frame recorder + scrubber facade.
#[derive(Debug, Clone)]
pub struct RecordingApi {
    timeline: FrameTimeline,
    live: bool,
    selected: FrameIndex,
}

impl RecordingApi {
    /// A recorder with the browser-safe memory budget (3,600 frames / 64 MiB).
    pub fn browser_safe() -> KernelResult<Self> {
        Self::with_limits(BROWSER_MAX_FRAMES, BROWSER_MAX_BYTES)
    }

    /// A recorder with the larger native memory budget (10,000 frames / 128 MiB).
    pub fn native() -> KernelResult<Self> {
        Self::with_limits(NATIVE_MAX_FRAMES, NATIVE_MAX_BYTES)
    }

    /// A recorder with explicit bounds. Rejects a zero `max_frames`/`max_bytes`
    /// with a deterministic error.
    pub fn with_limits(max_frames: usize, max_bytes: usize) -> KernelResult<Self> {
        FrameTimeline::new(max_frames, max_bytes).map(Self::from_timeline)
    }

    /// Wrap a constructed timeline in a fresh live recorder.
    fn from_timeline(timeline: FrameTimeline) -> Self {
        RecordingApi {
            timeline,
            live: true,
            selected: FrameIndex::new(0),
        }
    }

    /// Record one frame's opaque artifact bytes at the host's `frame`/`tick`
    /// counters (wrapped into kernel [`FrameIndex`]/[`Tick`] identity internally —
    /// the ingestion point takes the raw `u64`s a host already has, so a caller
    /// need not depend on the kernel just to name them). The recorder treats every
    /// payload as undifferentiated bytes (an empty `Vec` means "this artifact was
    /// unavailable this frame"). Enforces both memory bounds, evicting the oldest
    /// frames as needed; rejects a single capture larger than the whole budget.
    pub fn record_frame(
        &mut self,
        frame: u64,
        tick: u64,
        input_bytes: Vec<u8>,
        runtime_bytes: Vec<u8>,
        state_bytes: Vec<u8>,
        render_bytes: Vec<u8>,
    ) -> KernelResult<()> {
        self.timeline.push_frame(FrameCapture::new(
            FrameIndex::new(frame),
            Tick::new(tick),
            input_bytes,
            runtime_bytes,
            state_bytes,
            render_bytes,
        ))
    }

    /// The number of retained captures.
    pub fn frame_count(&self) -> usize {
        self.timeline.len()
    }

    /// Whether nothing has been recorded (or everything was cleared).
    pub fn is_empty(&self) -> bool {
        self.timeline.is_empty()
    }

    /// The bytes currently retained against the budget.
    pub fn current_bytes(&self) -> usize {
        self.timeline.current_bytes()
    }

    /// The configured memory budget in bytes.
    pub fn max_bytes(&self) -> usize {
        self.timeline.max_bytes()
    }

    /// The configured maximum retained frame count.
    pub fn max_frames(&self) -> usize {
        self.timeline.max_frames()
    }

    /// The oldest retained frame index, or a deterministic error if empty.
    pub fn oldest_frame_index(&self) -> KernelResult<FrameIndex> {
        self.timeline.oldest_frame_index()
    }

    /// The newest retained frame index, or a deterministic error if empty.
    pub fn latest_frame_index(&self) -> KernelResult<FrameIndex> {
        self.timeline.latest_frame_index()
    }

    /// The capture at `frame_index`, returned opaquely. The caller reads its
    /// bytes/hashes through inference; the type is not part of the public surface.
    pub fn frame(&self, frame_index: FrameIndex) -> KernelResult<&FrameCapture> {
        self.timeline.get_frame(frame_index)
    }

    /// The current playback mode, constructed from the cursor on demand.
    pub fn mode(&self) -> TimelineMode {
        [
            TimelineMode::scrubbing(self.selected),
            TimelineMode::live(),
        ][usize::from(self.live)]
    }

    /// Whether the recorder is following live frames.
    pub fn is_live(&self) -> bool {
        self.live
    }

    /// The frame currently being scrubbed, or `None` while live.
    pub fn selected_frame(&self) -> Option<FrameIndex> {
        (!self.live).then_some(self.selected)
    }

    /// Begin scrubbing on the host frame number `frame`. Fails (without changing
    /// the cursor) if that frame is not retained. Does not mutate the timeline.
    pub fn enter_scrub(&mut self, frame: u64) -> KernelResult<()> {
        let frame_index = FrameIndex::new(frame);
        let present = self.timeline.get_frame(frame_index).map(|_| ());
        present.map(|()| self.select(frame_index))
    }

    /// Step the scrub selection to the previous retained frame (entering scrub
    /// from live steps back from the latest frame). Fails at the oldest edge or
    /// on an empty timeline. Does not mutate the timeline.
    pub fn step_back(&mut self) -> KernelResult<()> {
        let target = self.previous_selection();
        target.map(|prev| self.select(prev))
    }

    /// Step the scrub selection to the next retained frame. Fails at the latest
    /// edge or on an empty timeline. Does not mutate the timeline.
    pub fn step_forward(&mut self) -> KernelResult<()> {
        let target = self.next_selection();
        target.map(|next| self.select(next))
    }

    /// Resume following live frames.
    pub fn resume(&mut self) {
        self.live = true;
    }

    /// Compare this recording against a replay recording and report the first
    /// divergence (or a match). A length / frame-index shape mismatch is a
    /// deterministic structural error rather than an artifact report.
    pub fn compare_with(&self, other: &RecordingApi) -> KernelResult<DeterminismReport> {
        compare_timelines(&self.timeline, &other.timeline)
    }

    /// Discard every retained capture and reset memory accounting. Returns to
    /// live mode.
    pub fn clear(&mut self) {
        self.timeline.clear();
        self.live = true;
        self.selected = FrameIndex::new(0);
    }

    /// Move the cursor onto `frame` and pause live following.
    fn select(&mut self, frame: FrameIndex) {
        self.selected = frame;
        self.live = false;
    }

    /// The frame the cursor currently points at: the selected frame while
    /// scrubbing, or the latest frame while live. Errors on an empty timeline.
    fn current_selection(&self) -> KernelResult<FrameIndex> {
        self.timeline
            .latest_frame_index()
            .map(|latest| [self.selected, latest][usize::from(self.live)])
    }

    /// The retained frame before the current cursor position.
    fn previous_selection(&self) -> KernelResult<FrameIndex> {
        self.current_selection()
            .and_then(|cur| self.timeline.previous_frame_index(cur))
    }

    /// The retained frame after the current cursor position.
    fn next_selection(&self) -> KernelResult<FrameIndex> {
        self.current_selection()
            .and_then(|cur| self.timeline.next_frame_index(cur))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> RecordingApi {
        RecordingApi::with_limits(8, 1 << 20).unwrap()
    }

    fn push(r: &mut RecordingApi, frame: u64) {
        r.record_frame(
            frame,
            frame * 10,
            vec![frame as u8],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
    }

    #[test]
    fn default_recorders_have_the_documented_budgets() {
        let b = RecordingApi::browser_safe().unwrap();
        assert_eq!(b.max_frames(), 3_600);
        assert_eq!(b.max_bytes(), 64 * 1024 * 1024);
        let n = RecordingApi::native().unwrap();
        assert_eq!(n.max_frames(), 10_000);
        assert_eq!(n.max_bytes(), 128 * 1024 * 1024);
    }

    #[test]
    fn with_limits_rejects_zero_bounds() {
        assert!(RecordingApi::with_limits(0, 1024).is_err());
        assert!(RecordingApi::with_limits(8, 0).is_err());
    }

    #[test]
    fn a_fresh_recorder_is_empty_and_live() {
        let r = rec();
        assert!(r.is_empty());
        assert_eq!(r.frame_count(), 0);
        assert!(r.is_live());
        assert_eq!(r.selected_frame(), None);
        assert!(r.mode().is_live());
    }

    #[test]
    fn recording_frames_grows_the_timeline_and_tracks_bytes() {
        let mut r = rec();
        push(&mut r, 0);
        push(&mut r, 1);
        assert_eq!(r.frame_count(), 2);
        assert!(!r.is_empty());
        assert!(r.current_bytes() > 0);
        assert_eq!(r.oldest_frame_index().unwrap(), FrameIndex::new(0));
        assert_eq!(r.latest_frame_index().unwrap(), FrameIndex::new(1));
    }

    #[test]
    fn recording_too_large_a_capture_is_rejected() {
        let mut r = RecordingApi::with_limits(8, 16).unwrap();
        let err = r.record_frame(0, 0, vec![0_u8; 4096], Vec::new(), Vec::new(), Vec::new());
        assert!(err.is_err());
        assert!(r.is_empty());
    }

    #[test]
    fn fetching_a_recorded_frame_exposes_its_opaque_payload() {
        let mut r = rec();
        push(&mut r, 3);
        let c = r.frame(FrameIndex::new(3)).unwrap();
        assert_eq!(c.frame_index(), FrameIndex::new(3));
        assert_eq!(c.tick(), Tick::new(30));
        assert_eq!(c.input_bytes(), &[3_u8]);
        assert_ne!(c.final_hash(), 0);
        // A missing frame is a deterministic error.
        assert!(r.frame(FrameIndex::new(99)).is_err());
    }

    #[test]
    fn entering_scrub_selects_a_present_frame_and_pauses_live() {
        let mut r = rec();
        push(&mut r, 0);
        push(&mut r, 1);
        push(&mut r, 2);
        r.enter_scrub(1).unwrap();
        assert!(!r.is_live());
        assert_eq!(r.selected_frame(), Some(FrameIndex::new(1)));
        assert!(!r.mode().is_live());
        assert_eq!(r.mode(), TimelineMode::scrubbing(FrameIndex::new(1)));
    }

    #[test]
    fn entering_scrub_on_a_missing_frame_fails_and_keeps_live() {
        let mut r = rec();
        push(&mut r, 0);
        assert!(r.enter_scrub(42).is_err());
        assert!(r.is_live());
    }

    #[test]
    fn scrubbing_does_not_mutate_the_timeline() {
        let mut r = rec();
        (0..4).for_each(|f| push(&mut r, f));
        let before = (r.frame_count(), r.current_bytes());
        r.enter_scrub(2).unwrap();
        r.step_back().unwrap();
        r.step_forward().unwrap();
        assert_eq!((r.frame_count(), r.current_bytes()), before);
    }

    #[test]
    fn step_back_from_live_walks_back_from_the_latest_frame() {
        let mut r = rec();
        (0..3).for_each(|f| push(&mut r, f)); // 0,1,2
        r.step_back().unwrap();
        assert_eq!(r.selected_frame(), Some(FrameIndex::new(1)));
        r.step_back().unwrap();
        assert_eq!(r.selected_frame(), Some(FrameIndex::new(0)));
        // At the oldest edge stepping back fails.
        assert!(r.step_back().is_err());
        assert_eq!(r.selected_frame(), Some(FrameIndex::new(0)));
    }

    #[test]
    fn step_forward_walks_toward_the_latest_and_stops_at_the_edge() {
        let mut r = rec();
        (0..3).for_each(|f| push(&mut r, f));
        r.enter_scrub(0).unwrap();
        r.step_forward().unwrap();
        assert_eq!(r.selected_frame(), Some(FrameIndex::new(1)));
        r.step_forward().unwrap();
        assert_eq!(r.selected_frame(), Some(FrameIndex::new(2)));
        assert!(r.step_forward().is_err());
    }

    #[test]
    fn stepping_an_empty_timeline_is_a_deterministic_error() {
        let mut r = rec();
        assert!(r.step_back().is_err());
        assert!(r.step_forward().is_err());
    }

    #[test]
    fn resume_returns_to_live_following() {
        let mut r = rec();
        (0..2).for_each(|f| push(&mut r, f));
        r.enter_scrub(0).unwrap();
        assert!(!r.is_live());
        r.resume();
        assert!(r.is_live());
        assert_eq!(r.selected_frame(), None);
    }

    #[test]
    fn clear_empties_and_returns_to_live() {
        let mut r = rec();
        (0..3).for_each(|f| push(&mut r, f));
        r.enter_scrub(1).unwrap();
        r.clear();
        assert!(r.is_empty());
        assert!(r.is_live());
        assert_eq!(r.selected_frame(), None);
    }

    #[test]
    fn comparing_identical_recordings_matches() {
        let mut a = rec();
        let mut b = rec();
        (0..3).for_each(|f| {
            push(&mut a, f);
            push(&mut b, f);
        });
        assert!(a.compare_with(&b).unwrap().matched());
    }

    #[test]
    fn comparing_divergent_recordings_reports_the_first_difference() {
        let mut a = rec();
        let mut b = rec();
        push(&mut a, 0);
        push(&mut b, 0);
        a.record_frame(1, 10, vec![1], Vec::new(), Vec::new(), Vec::new())
            .unwrap();
        b.record_frame(1, 10, vec![9], Vec::new(), Vec::new(), Vec::new())
            .unwrap();
        let report = a.compare_with(&b).unwrap();
        assert!(!report.matched());
        assert_eq!(report.first_mismatching_frame(), Some(FrameIndex::new(1)));
    }

    #[test]
    fn comparing_different_length_recordings_is_a_structural_error() {
        let mut a = rec();
        let mut b = rec();
        push(&mut a, 0);
        push(&mut a, 1);
        push(&mut b, 0);
        assert!(a.compare_with(&b).is_err());
    }

    #[test]
    fn recorder_is_debug_and_clone() {
        let mut r = rec();
        push(&mut r, 0);
        let c = r.clone();
        assert_eq!(c.frame_count(), 1);
        assert!(format!("{r:?}").contains("RecordingApi"));
    }
}
