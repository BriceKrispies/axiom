//! [`ClipSampler`]: turn an [`AnimationClip`] plus a frame into a [`Pose`].
//!
//! The sampler is bound to a bone count (a rig size) and is otherwise pure: it
//! starts from a rest pose and lays each track's sampled rotation onto its
//! target bone. Sampling the same clip at the same frame always yields an
//! identical pose — the determinism the lab's scrubber relies on.

use axiom_math::Vec3;

use crate::clip::AnimationClip;
use crate::pose::Pose;

/// Samples clips against a fixed rig size. Bones no track touches keep their
/// rest (zero) rotation; a track targeting a bone outside the rig is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipSampler {
    bone_count: usize,
}

impl ClipSampler {
    /// A sampler for a rig with `bone_count` bones.
    pub const fn new(bone_count: usize) -> Self {
        Self { bone_count }
    }

    /// The rig size this sampler produces poses for.
    pub const fn bone_count(&self) -> usize {
        self.bone_count
    }

    /// Sample `clip` at `frame` into a full [`Pose`].
    pub fn sample(&self, clip: &AnimationClip, frame: u32) -> Pose {
        let mut eulers = vec![Vec3::ZERO; self.bone_count];
        clip.tracks.iter().for_each(|track| {
            (track.bone < self.bone_count).then(|| {
                eulers[track.bone] = track.sample(frame);
            });
        });
        Pose::new(eulers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::{BoneTrack, ClipPhase, Keyframe, PhaseKind};
    use crate::events::EventTrack;
    use axiom_math::{ApproxEq, Epsilon};

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    fn clip() -> AnimationClip {
        AnimationClip::new(
            "t",
            10,
            vec![
                BoneTrack::new(
                    1,
                    vec![
                        Keyframe::new(0, Vec3::ZERO),
                        Keyframe::new(10, Vec3::new(1.0, 0.0, 0.0)),
                    ],
                ),
                // A track targeting a bone outside a 3-bone rig: ignored.
                BoneTrack::new(9, vec![Keyframe::new(0, Vec3::new(5.0, 5.0, 5.0))]),
            ],
            vec![ClipPhase::new(PhaseKind::Ready, 0, 10)],
            EventTrack::new(vec![]),
        )
    }

    #[test]
    fn sample_lays_track_on_target_bone_and_rests_others() {
        let sampler = ClipSampler::new(3);
        assert_eq!(sampler.bone_count(), 3);
        let pose = sampler.sample(&clip(), 5);
        assert_eq!(pose.bone_count(), 3);
        assert!(pose.joint_eulers[0].approx_eq(&Vec3::ZERO, eps()));
        assert!(pose.joint_eulers[1].approx_eq(&Vec3::new(0.5, 0.0, 0.0), eps()));
        // Bone 2 untouched; the out-of-range track (bone 9) had no effect.
        assert!(pose.joint_eulers[2].approx_eq(&Vec3::ZERO, eps()));
    }

    #[test]
    fn sampling_same_frame_is_identical() {
        let sampler = ClipSampler::new(3);
        let c = clip();
        (0..10u32).for_each(|f| assert_eq!(sampler.sample(&c, f), sampler.sample(&c, f)));
    }
}
