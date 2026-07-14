//! An animation clip: per-bone keyframe tracks, sampled against a skeleton.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, Tick};
use axiom_math::Transform;

use crate::animation_error::AnimationError;
use crate::animation_result::AnimationResult;
use crate::clip_event::ClipEvent;
use crate::clip_phase::ClipPhase;
use crate::ids::BoneId;
use crate::keyframe::Keyframe;
use crate::pose::Pose;
use crate::skeleton::Skeleton;
use crate::track::Track;

/// A set of per-bone [`Track`]s. Sampling a clip at a [`Tick`] against a
/// [`Skeleton`] yields a full [`Pose`]: bones the clip animates take their
/// sampled transform, and bones it does not touch fall back to their skeleton
/// rest local. A clip is built through [`crate::AnimationApi`] (`create_clip` +
/// `add_track`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimationClip {
    tracks: Vec<Track>,
    events: Vec<ClipEvent>,
    phases: Vec<ClipPhase>,
}

impl AnimationClip {
    /// An empty clip that animates nothing.
    pub(crate) fn new() -> Self {
        AnimationClip {
            tracks: Vec::new(),
            events: Vec::new(),
            phases: Vec::new(),
        }
    }

    /// Attach an opaque-coded event at `at`.
    pub(crate) fn add_event(&mut self, at: Tick, code: u32) {
        self.events.push(ClipEvent::new(at, code));
    }

    /// Attach an opaque-coded phase spanning `[start, end)`.
    pub(crate) fn add_phase(&mut self, start: Tick, end: Tick, code: u32) {
        self.phases.push(ClipPhase::new(start, end, code));
    }

    /// The codes of every event that fires exactly at `tick`, in insertion
    /// order.
    pub(crate) fn events_at(&self, tick: Tick) -> Vec<u32> {
        self.events
            .iter()
            .filter(|e| e.at() == tick)
            .map(|e| e.code())
            .collect()
    }

    /// The code of the first phase whose span contains `tick`, if any.
    pub(crate) fn phase_at(&self, tick: Tick) -> Option<u32> {
        self.phases
            .iter()
            .find(|p| p.contains(tick))
            .map(|p| p.code())
    }

    /// Add a validated track for `bone` from `keys`. Fails with `EmptyTrack`
    /// (no keyframes) or `NonMonotonicKeyframes` (times not strictly
    /// increasing).
    pub(crate) fn add_track(&mut self, bone: BoneId, keys: Vec<Keyframe>) -> AnimationResult<()> {
        Track::new(bone, keys).map(|track| self.tracks.push(track))
    }

    /// The track driving `bone`, if the clip animates it.
    fn track_for(&self, bone: BoneId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.bone() == bone)
    }

    /// Sample the whole clip at `tick` against `skeleton`, producing a pose over
    /// every bone. Fails with `BoneNotFound` if any track references a bone
    /// outside the skeleton, or propagates an interpolation failure.
    pub(crate) fn sample(&self, skeleton: &Skeleton, tick: Tick) -> AnimationResult<Pose> {
        let n = skeleton.bone_count();
        self.tracks
            .iter()
            .all(|t| t.bone().raw() < n as u64)
            .then_some(())
            .ok_or_else(|| {
                AnimationError::bone_not_found("clip track references a bone outside the skeleton")
            })
            .and_then(|()| {
                (0..n)
                    .map(|i| {
                        let bone = BoneId::from_raw(i as u64);
                        let rest = skeleton
                            .bone(bone)
                            .map(|b| b.rest())
                            .unwrap_or(Transform::IDENTITY);
                        self.track_for(bone)
                            .map(|t| t.sample(tick))
                            .unwrap_or_else(|| Ok(rest))
                    })
                    .collect::<AnimationResult<Vec<Transform>>>()
            })
            .map(Pose::from_locals)
    }

    /// Append the clip's bytes: each of tracks, events, and phases as a `u64`
    /// count followed by that many encoded items, in that order.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_u64(self.tracks.len() as u64);
        self.tracks.iter().for_each(|track| track.write_to(writer));
        writer.write_u64(self.events.len() as u64);
        self.events.iter().for_each(|event| event.write_to(writer));
        writer.write_u64(self.phases.len() as u64);
        self.phases.iter().for_each(|phase| phase.write_to(writer));
    }

    /// Read a clip written by [`AnimationClip::write_to`].
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<AnimationClip> {
        read_counted(reader, Track::read_from).and_then(|tracks| {
            read_counted(reader, ClipEvent::read_from).and_then(|events| {
                read_counted(reader, ClipPhase::read_from).map(|phases| AnimationClip {
                    tracks,
                    events,
                    phases,
                })
            })
        })
    }
}

/// Read a `u64` length prefix then that many `T`s with `read_one`, into a `Vec`.
fn read_counted<T>(
    reader: &mut BinaryReader<'_>,
    read_one: fn(&mut BinaryReader<'_>) -> KernelResult<T>,
) -> KernelResult<Vec<T>> {
    reader.read_u64().and_then(|count| {
        (0..count).try_fold(Vec::new(), |mut acc, _| {
            read_one(reader).map(|item| {
                acc.push(item);
                acc
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_error_code::AnimationErrorCode;
    use axiom_math::{ApproxEq, Epsilon, Quat, Vec3};

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    fn key(t: u64, x: f32) -> Keyframe {
        Keyframe::new(
            Tick::new(t),
            Transform::from_translation(Vec3::new(x, 0.0, 0.0)),
        )
    }

    /// A two-bone skeleton (root + child) where only the child is animated, so
    /// sampling exercises both the tracked and the untracked (rest) paths.
    fn skeleton_two_bones() -> Skeleton {
        let mut skel = Skeleton::new();
        let root = skel.push_root(Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)));
        skel.add_child(root, Transform::IDENTITY).unwrap();
        skel
    }

    #[test]
    fn clip_round_trips_through_bytes() {
        let mut clip = AnimationClip::new();
        clip.add_track(BoneId::from_raw(1), vec![key(0, 0.0), key(10, 10.0)])
            .unwrap();
        clip.add_track(BoneId::from_raw(0), vec![key(0, 1.0)])
            .unwrap();
        clip.add_event(Tick::new(5), 42);
        clip.add_phase(Tick::new(0), Tick::new(6), 1);
        clip.add_phase(Tick::new(6), Tick::new(10), 2);
        let mut w = BinaryWriter::new();
        clip.write_to(&mut w);
        let bytes = w.into_bytes();
        let back = AnimationClip::read_from(&mut BinaryReader::new(&bytes)).unwrap();
        assert_eq!(back, clip);
    }

    #[test]
    fn empty_clip_round_trips_and_truncation_fails() {
        let clip = AnimationClip::new();
        let mut w = BinaryWriter::new();
        clip.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            AnimationClip::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            clip
        );
        assert!(AnimationClip::read_from(&mut BinaryReader::new(&bytes[..4])).is_err());
    }

    #[test]
    fn sample_fills_tracked_and_rest_bones() {
        let skel = skeleton_two_bones();
        let mut clip = AnimationClip::new();
        clip.add_track(BoneId::from_raw(1), vec![key(0, 0.0), key(10, 10.0)])
            .unwrap();
        let pose = clip.sample(&skel, Tick::new(5)).unwrap();
        // Untracked root holds its rest (x = 5); tracked child interpolates to x = 5.
        assert!(pose
            .local(BoneId::from_raw(0))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(5.0, 0.0, 0.0), eps()));
        assert!(pose
            .local(BoneId::from_raw(1))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(5.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn sample_is_deterministic() {
        let skel = skeleton_two_bones();
        let mut clip = AnimationClip::new();
        clip.add_track(BoneId::from_raw(1), vec![key(0, 0.0), key(10, 10.0)])
            .unwrap();
        assert_eq!(
            clip.sample(&skel, Tick::new(4)).unwrap(),
            clip.sample(&skel, Tick::new(4)).unwrap()
        );
    }

    #[test]
    fn track_bone_out_of_range_is_rejected() {
        let skel = skeleton_two_bones();
        let mut clip = AnimationClip::new();
        clip.add_track(BoneId::from_raw(9), vec![key(0, 0.0), key(10, 10.0)])
            .unwrap();
        assert_eq!(
            clip.sample(&skel, Tick::new(0)).unwrap_err().code(),
            AnimationErrorCode::BoneNotFound
        );
    }

    #[test]
    fn add_track_validates_keys() {
        let mut clip = AnimationClip::new();
        assert_eq!(
            clip.add_track(BoneId::from_raw(0), Vec::new())
                .unwrap_err()
                .code(),
            AnimationErrorCode::EmptyTrack
        );
    }

    #[test]
    fn events_fire_at_their_exact_tick() {
        let mut clip = AnimationClip::new();
        clip.add_event(Tick::new(5), 42);
        clip.add_event(Tick::new(5), 7);
        clip.add_event(Tick::new(6), 99);
        assert_eq!(clip.events_at(Tick::new(5)), vec![42, 7]);
        assert_eq!(clip.events_at(Tick::new(6)), vec![99]);
        assert!(clip.events_at(Tick::new(4)).is_empty());
    }

    #[test]
    fn phase_at_reports_the_covering_span() {
        let mut clip = AnimationClip::new();
        clip.add_phase(Tick::new(0), Tick::new(4), 1);
        clip.add_phase(Tick::new(4), Tick::new(8), 2);
        assert_eq!(clip.phase_at(Tick::new(2)), Some(1));
        assert_eq!(clip.phase_at(Tick::new(4)), Some(2));
        assert_eq!(clip.phase_at(Tick::new(9)), None);
    }

    #[test]
    fn interpolation_failure_propagates() {
        // A keyframe with a zero-length rotation makes nlerp fail; the clip
        // surfaces it rather than panicking.
        let mut skel = Skeleton::new();
        skel.push_root(Transform::IDENTITY);
        let bad = Transform::new(Vec3::ZERO, Quat::new(0.0, 0.0, 0.0, 0.0), Vec3::ONE);
        let mut clip = AnimationClip::new();
        clip.add_track(
            BoneId::from_raw(0),
            vec![
                Keyframe::new(Tick::new(0), bad),
                Keyframe::new(Tick::new(4), bad),
            ],
        )
        .unwrap();
        assert_eq!(
            clip.sample(&skel, Tick::new(2)).unwrap_err().code(),
            AnimationErrorCode::NonFiniteInterpolation
        );
    }
}
