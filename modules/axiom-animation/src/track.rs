//! A per-bone keyframe track and its deterministic sampler.

use axiom_kernel::Tick;
use axiom_math::Transform;

use crate::animation_error::AnimationError;
use crate::animation_result::AnimationResult;
use crate::ids::BoneId;
use crate::interpolate::lerp_transform;
use crate::keyframe::Keyframe;

/// The keyframe track for one bone: which [`BoneId`] it drives and its ordered
/// [`Keyframe`]s. A valid track has at least one keyframe and strictly
/// increasing times, both enforced at construction.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Track {
    bone: BoneId,
    keys: Vec<Keyframe>,
}

impl Track {
    /// Build a validated track. Fails with `EmptyTrack` if `keys` is empty, or
    /// `NonMonotonicKeyframes` if the keyframe times are not strictly
    /// increasing.
    pub(crate) fn new(bone: BoneId, keys: Vec<Keyframe>) -> AnimationResult<Track> {
        (!keys.is_empty())
            .then_some(())
            .ok_or_else(|| AnimationError::empty_track("animation track needs at least one keyframe"))
            .and_then(|()| {
                keys.windows(2)
                    .all(|w| w[0].time().raw() < w[1].time().raw())
                    .then_some(())
                    .ok_or_else(|| {
                        AnimationError::non_monotonic_keyframes(
                            "keyframe times must be strictly increasing",
                        )
                    })
            })
            .map(|()| Track { bone, keys })
    }

    /// The bone this track drives.
    pub(crate) fn bone(&self) -> BoneId {
        self.bone
    }

    /// Sample the track at `tick`. Before the first key the first pose is held;
    /// after the last key the last pose is held; between two keys the transform
    /// is interpolated by the fraction of the tick span elapsed. Deterministic
    /// for any tick.
    pub(crate) fn sample(&self, tick: Tick) -> AnimationResult<Transform> {
        let len = self.keys.len();
        // Number of keys at or before `tick`; the upper neighbour is the next
        // key, both clamped into range so out-of-range ticks hold an endpoint.
        let idx = self.keys.partition_point(|k| k.time().raw() <= tick.raw());
        let hi = idx.min(len - 1);
        let lo = hi.saturating_sub(1);
        let lo_key = self.keys[lo];
        let hi_key = self.keys[hi];
        let lo_time = lo_key.time().raw();
        let hi_time = hi_key.time().raw();
        let span = hi_time - lo_time;
        // Clamp the query into `[lo_time, hi_time]` so the fraction stays in
        // `[0, 1]`; `span.max(1)` keeps the divisor non-zero when lo == hi.
        let elapsed = tick.raw().max(lo_time).min(hi_time) - lo_time;
        let factor = (elapsed as f32 / (span as f32).max(1.0)).clamp(0.0, 1.0);
        lerp_transform(lo_key.transform(), hi_key.transform(), factor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation_error_code::AnimationErrorCode;
    use axiom_math::{ApproxEq, Epsilon, Vec3};

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    fn key(t: u64, x: f32) -> Keyframe {
        Keyframe::new(Tick::new(t), Transform::from_translation(Vec3::new(x, 0.0, 0.0)))
    }

    fn two_key_track() -> Track {
        Track::new(BoneId::from_raw(0), vec![key(0, 0.0), key(10, 10.0)]).unwrap()
    }

    #[test]
    fn empty_track_is_rejected() {
        let err = Track::new(BoneId::from_raw(0), Vec::new()).unwrap_err();
        assert_eq!(err.code(), AnimationErrorCode::EmptyTrack);
    }

    #[test]
    fn non_increasing_times_are_rejected() {
        let err = Track::new(BoneId::from_raw(0), vec![key(5, 0.0), key(5, 1.0)]).unwrap_err();
        assert_eq!(err.code(), AnimationErrorCode::NonMonotonicKeyframes);
    }

    #[test]
    fn records_its_bone() {
        assert_eq!(two_key_track().bone(), BoneId::from_raw(0));
    }

    #[test]
    fn sample_at_exact_keyframe_is_exact() {
        let track = two_key_track();
        assert!(track
            .sample(Tick::new(0))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(0.0, 0.0, 0.0), eps()));
        assert!(track
            .sample(Tick::new(10))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(10.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn sample_between_keyframes_interpolates() {
        let track = two_key_track();
        assert!(track
            .sample(Tick::new(3))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(3.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn ticks_outside_range_hold_the_endpoints() {
        // A track that does not start at tick 0 exercises the before-first clamp.
        let track = Track::new(BoneId::from_raw(0), vec![key(4, 4.0), key(8, 8.0)]).unwrap();
        assert!(track
            .sample(Tick::new(0))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(4.0, 0.0, 0.0), eps()));
        assert!(track
            .sample(Tick::new(99))
            .unwrap()
            .translation
            .approx_eq(&Vec3::new(8.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn single_key_track_holds_constant() {
        let track = Track::new(BoneId::from_raw(0), vec![key(5, 7.0)]).unwrap();
        for tick in [0_u64, 5, 100] {
            assert!(track
                .sample(Tick::new(tick))
                .unwrap()
                .translation
                .approx_eq(&Vec3::new(7.0, 0.0, 0.0), eps()));
        }
    }
}
