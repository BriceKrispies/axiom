//! Authored clips: keyframed per-bone rotation tracks, a phase timeline, and an
//! event track, all indexed by an integer frame.
//!
//! Time here is an integer **frame**, not wall-clock — a clip is sampled at
//! frame `f`, and sampling at the same `f` always yields the same value
//! (determinism §17.5). A [`BoneTrack`] interpolates its keyframes with a
//! branchless piecewise-linear lookup: a single fold finds the surrounding
//! keyframes and a clamped ratio blends them, so a frame before the first key or
//! after the last simply holds the endpoint.

use axiom_math::Vec3;

use crate::events::EventTrack;

/// One authored sample on a [`BoneTrack`]: an Euler rotation (radians) at a
/// specific integer frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyframe {
    /// The frame this key sits on.
    pub frame: u32,
    /// The bone's Euler rotation (radians) at this frame.
    pub euler: Vec3,
}

impl Keyframe {
    /// The zero key at frame 0 — the fallback for a track with no keys.
    pub const ZERO: Keyframe = Keyframe {
        frame: 0,
        euler: Vec3::ZERO,
    };

    /// Construct a key.
    pub const fn new(frame: u32, euler: Vec3) -> Self {
        Self { frame, euler }
    }
}

/// A per-bone rotation track: the bone it drives and its keyframes, ordered by
/// frame.
#[derive(Debug, Clone, PartialEq)]
pub struct BoneTrack {
    /// Index of the bone this track rotates.
    pub bone: usize,
    /// Keyframes in ascending frame order.
    pub keys: Vec<Keyframe>,
}

impl BoneTrack {
    /// Construct a track.
    pub fn new(bone: usize, keys: Vec<Keyframe>) -> Self {
        Self { bone, keys }
    }

    /// The interpolated Euler rotation at `frame`. Branchless piecewise-linear:
    /// one fold tightens `(lo, hi)` to the keys bracketing `frame`, then a
    /// clamped ratio blends them. A frame outside the key range holds the
    /// nearest endpoint.
    pub fn sample(&self, frame: u32) -> Vec3 {
        let first = self.keys.first().copied().unwrap_or(Keyframe::ZERO);
        let last = self.keys.last().copied().unwrap_or(Keyframe::ZERO);
        let (lo, hi) = self.keys.iter().fold((first, last), |(lo, hi), k| {
            let take_lo = (k.frame <= frame) & (k.frame >= lo.frame);
            let take_hi = (k.frame >= frame) & (k.frame <= hi.frame);
            (
                [lo, *k][usize::from(take_lo)],
                [hi, *k][usize::from(take_hi)],
            )
        });
        let span = (hi.frame - lo.frame).max(1) as f32;
        let t = ((frame as f32 - lo.frame as f32) / span).clamp(0.0, 1.0);
        lo.euler.mul_scalar(1.0 - t).add(hi.euler.mul_scalar(t))
    }
}

/// The named stages of a clip's timeline. For the humanoid kick these run
/// `ready → lean_forward → approach → plant → backswing → strike →
/// follow_through → recover`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseKind {
    /// Standing, weight settled, before any motion.
    Ready,
    /// The torso tips forward to begin the approach.
    LeanForward,
    /// Steps in toward the ball.
    Approach,
    /// The support (plant) foot is set beside the ball.
    Plant,
    /// The kicking leg cocks back.
    Backswing,
    /// The kicking leg drives through; contact happens here.
    Strike,
    /// The leg continues past the contact point.
    FollowThrough,
    /// Motion settles back toward a neutral stance.
    Recover,
}

/// One phase span on a clip's timeline: a [`PhaseKind`] over `[start_frame,
/// end_frame)` (start inclusive, end exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipPhase {
    /// Which stage this span is.
    pub kind: PhaseKind,
    /// First frame of the phase (inclusive).
    pub start_frame: u32,
    /// One past the last frame of the phase (exclusive).
    pub end_frame: u32,
}

impl ClipPhase {
    /// Construct a phase span.
    pub const fn new(kind: PhaseKind, start_frame: u32, end_frame: u32) -> Self {
        Self {
            kind,
            start_frame,
            end_frame,
        }
    }

    /// Whether `frame` falls in `[start_frame, end_frame)`.
    pub fn contains(&self, frame: u32) -> bool {
        (frame >= self.start_frame) & (frame < self.end_frame)
    }
}

/// A complete authored clip: a name, a frame count, per-bone rotation tracks, a
/// phase timeline, and an event track.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationClip {
    /// The clip's name, e.g. `"kick_right"`.
    pub name: String,
    /// Total frame count; valid sample frames are `0..frame_count`.
    pub frame_count: u32,
    /// Per-bone rotation tracks.
    pub tracks: Vec<BoneTrack>,
    /// The ordered phase timeline.
    pub phases: Vec<ClipPhase>,
    /// Discrete events fired on specific frames.
    pub events: EventTrack,
}

impl AnimationClip {
    /// Construct a clip.
    pub fn new(
        name: &str,
        frame_count: u32,
        tracks: Vec<BoneTrack>,
        phases: Vec<ClipPhase>,
        events: EventTrack,
    ) -> Self {
        Self {
            name: name.to_string(),
            frame_count,
            tracks,
            phases,
            events,
        }
    }

    /// The phase covering `frame`, or `None` if no phase spans it.
    pub fn phase_at(&self, frame: u32) -> Option<ClipPhase> {
        self.phases.iter().copied().find(|p| p.contains(frame))
    }

    /// The clip's phase kinds in timeline order — the sequence a phase-order
    /// check compares against.
    pub fn phase_kinds(&self) -> Vec<PhaseKind> {
        self.phases.iter().map(|p| p.kind).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AnimationEvent, EventKind};
    use axiom_math::{ApproxEq, Epsilon};

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    #[test]
    fn track_interpolates_between_keys() {
        let track = BoneTrack::new(
            0,
            vec![
                Keyframe::new(0, Vec3::ZERO),
                Keyframe::new(10, Vec3::new(1.0, 0.0, 0.0)),
            ],
        );
        assert!(track.sample(0).approx_eq(&Vec3::ZERO, eps()));
        assert!(track.sample(5).approx_eq(&Vec3::new(0.5, 0.0, 0.0), eps()));
        assert!(track.sample(10).approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn track_holds_endpoints_outside_range() {
        let track = BoneTrack::new(
            0,
            vec![
                Keyframe::new(4, Vec3::new(2.0, 0.0, 0.0)),
                Keyframe::new(8, Vec3::new(3.0, 0.0, 0.0)),
            ],
        );
        // Before the first key holds the first value; after the last holds the last.
        assert!(track.sample(0).approx_eq(&Vec3::new(2.0, 0.0, 0.0), eps()));
        assert!(track.sample(20).approx_eq(&Vec3::new(3.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn track_with_no_keys_samples_zero() {
        let track = BoneTrack::new(0, vec![]);
        assert!(track.sample(3).approx_eq(&Vec3::ZERO, eps()));
    }

    #[test]
    fn track_with_single_key_holds_it() {
        let track = BoneTrack::new(0, vec![Keyframe::new(5, Vec3::new(7.0, 0.0, 0.0))]);
        assert!(track.sample(0).approx_eq(&Vec3::new(7.0, 0.0, 0.0), eps()));
        assert!(track.sample(9).approx_eq(&Vec3::new(7.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn track_sampling_is_deterministic() {
        let track = BoneTrack::new(
            0,
            vec![
                Keyframe::new(0, Vec3::ZERO),
                Keyframe::new(6, Vec3::new(0.4, -0.2, 0.1)),
                Keyframe::new(12, Vec3::new(1.0, 0.0, 0.0)),
            ],
        );
        (0..12u32).for_each(|f| assert_eq!(track.sample(f), track.sample(f)));
    }

    #[test]
    fn phase_contains_is_half_open() {
        let p = ClipPhase::new(PhaseKind::Strike, 4, 8);
        assert!(!p.contains(3));
        assert!(p.contains(4));
        assert!(p.contains(7));
        assert!(!p.contains(8));
    }

    #[test]
    fn clip_reports_phase_and_kinds() {
        let clip = AnimationClip::new(
            "t",
            8,
            vec![],
            vec![
                ClipPhase::new(PhaseKind::Ready, 0, 4),
                ClipPhase::new(PhaseKind::Strike, 4, 8),
            ],
            EventTrack::new(vec![AnimationEvent::new(5, EventKind::KickContact, 0)]),
        );
        assert_eq!(clip.phase_at(2).map(|p| p.kind), Some(PhaseKind::Ready));
        assert_eq!(clip.phase_at(6).map(|p| p.kind), Some(PhaseKind::Strike));
        assert_eq!(clip.phase_at(99), None);
        assert_eq!(clip.phase_kinds(), vec![PhaseKind::Ready, PhaseKind::Strike]);
        assert_eq!(clip.name, "t");
        assert_eq!(clip.frame_count, 8);
    }
}
