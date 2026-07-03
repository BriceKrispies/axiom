//! [`AnimationApi`]: the module's single behavioral facade.
//!
//! Every capability the animation core offers is reached through this one
//! stateless entry point — build the default humanoid, validate a skeleton,
//! sample a clip at a frame, clamp a pose to its joint limits, query phases and
//! events, and run forward kinematics to world-space joints. The vocabulary
//! types the facade traffics in (skeletons, poses, clips, limits, the prefab)
//! are re-exported alongside it from `lib.rs`; the behavior lives here.

use axiom_math::{Transform, Vec3};

use crate::clip::{AnimationClip, ClipPhase};
use crate::events::AnimationEvent;
use crate::pose::{BindPose, Pose};
use crate::prefab::HumanoidPrefab;
use crate::sampler::ClipSampler;
use crate::skeleton::{SkeletonDefinition, SkeletonError};
use crate::solver::{JointLimit, PoseSolver};

/// The one behavioral facade over the animation core. Stateless: construct one
/// and call through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnimationApi;

impl AnimationApi {
    /// Construct the facade.
    pub const fn new() -> Self {
        Self
    }

    /// The default editable low-poly humanoid rig with its `kick_right` clip.
    pub fn default_humanoid(&self) -> HumanoidPrefab {
        HumanoidPrefab::default_humanoid()
    }

    /// Validate a skeleton's topology (non-empty, rooted, parent-before-child).
    pub fn validate_skeleton(
        &self,
        skeleton: &SkeletonDefinition,
    ) -> Result<(), SkeletonError> {
        skeleton.validate()
    }

    /// Sample `clip` at `frame` into a pose for a `bone_count`-bone rig.
    pub fn sample(&self, clip: &AnimationClip, bone_count: usize, frame: u32) -> Pose {
        ClipSampler::new(bone_count).sample(clip, frame)
    }

    /// Clamp `pose` into `limits` (one limit per bone), preventing illegal bends.
    pub fn solve(&self, limits: &[JointLimit], pose: &Pose) -> Pose {
        PoseSolver::new(limits.to_vec()).solve(pose)
    }

    /// Whether `pose` is already within `limits` on every joint.
    pub fn is_pose_legal(&self, limits: &[JointLimit], pose: &Pose) -> bool {
        PoseSolver::new(limits.to_vec()).is_legal(pose)
    }

    /// The phase covering `frame`, if any.
    pub fn phase_at(&self, clip: &AnimationClip, frame: u32) -> Option<ClipPhase> {
        clip.phase_at(frame)
    }

    /// Every event firing on `frame`.
    pub fn events_at(&self, clip: &AnimationClip, frame: u32) -> Vec<AnimationEvent> {
        clip.events.at(frame)
    }

    /// Forward kinematics: each bone's world transform for `pose` on the rig.
    pub fn world_transforms(
        &self,
        skeleton: &SkeletonDefinition,
        bind: &BindPose,
        pose: &Pose,
    ) -> Vec<Transform> {
        pose.world_transforms(skeleton, bind)
    }

    /// World-space joint positions for `pose` — what a debug view draws.
    pub fn world_joint_positions(
        &self,
        skeleton: &SkeletonDefinition,
        bind: &BindPose,
        pose: &Pose,
    ) -> Vec<Vec3> {
        pose.world_joint_positions(skeleton, bind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::ApproxEq;

    fn eps() -> axiom_math::Epsilon {
        axiom_math::Epsilon::new(1.0e-4).unwrap()
    }

    /// Construct a `T` via `Default` from a generic context, so the call is on a
    /// type parameter — exercising the derived `Default` without clippy reading it
    /// as a redundant unit-struct construction.
    fn defaulted<T: Default>() -> T {
        T::default()
    }

    #[test]
    fn new_and_default_agree() {
        assert_eq!(AnimationApi::new(), AnimationApi);
        assert_eq!(defaulted::<AnimationApi>(), AnimationApi::new());
    }

    #[test]
    fn facade_builds_validates_samples_and_solves() {
        let api = AnimationApi::new();
        let prefab = api.default_humanoid();
        assert_eq!(api.validate_skeleton(&prefab.skeleton), Ok(()));

        let clip = &prefab.clips[0];
        let n = prefab.skeleton.bone_count();
        let pose = api.sample(clip, n, HumanoidPrefab::KICK_STRIKE_FRAME);
        assert_eq!(pose.bone_count(), n);

        // The authored kick is already legal; solving preserves it.
        assert!(api.is_pose_legal(&prefab.joint_limits, &pose));
        let solved = api.solve(&prefab.joint_limits, &pose);
        assert_eq!(solved, pose);
    }

    #[test]
    fn facade_reports_phase_and_events() {
        let api = AnimationApi::new();
        let clip = HumanoidPrefab::kick_right_clip();
        assert!(api.phase_at(&clip, HumanoidPrefab::KICK_STRIKE_FRAME).is_some());
        let events = api.events_at(&clip, HumanoidPrefab::KICK_STRIKE_FRAME);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].target_bone, HumanoidPrefab::RIGHT_FOOT_BONE);
    }

    #[test]
    fn facade_runs_forward_kinematics() {
        let api = AnimationApi::new();
        let prefab = api.default_humanoid();
        let pose = Pose::rest(prefab.skeleton.bone_count());
        let transforms = api.world_transforms(&prefab.skeleton, &prefab.bind_pose, &pose);
        let joints = api.world_joint_positions(&prefab.skeleton, &prefab.bind_pose, &pose);
        assert_eq!(transforms.len(), prefab.skeleton.bone_count());
        assert_eq!(joints.len(), prefab.skeleton.bone_count());
        // Root sits at the pelvis-anchored origin; joint 0 is the rig root.
        assert!(joints[0].approx_eq(&transforms[0].translation, eps()));
    }
}
