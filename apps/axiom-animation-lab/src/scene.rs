//! The lab scene: the humanoid rig plus everything a frame needs computed.
//!
//! This is the app's composition glue — it reads the `axiom-animation` module's
//! facade and turns a frame number into the concrete, solved, world-space data a
//! debug view draws. Being an app, it is free to branch; the determinism comes
//! from the module underneath (sampling frame `f` always yields the same pose).

use axiom_animation::{
    AnimationApi, AnimationClip, AnimationEvent, HumanoidPrefab, PhaseKind, SkeletonDefinition,
};
use axiom_math::Vec3;

/// A single bone segment to draw: the world positions of its endpoints and
/// whether it is part of the (right) kicking leg.
#[derive(Debug, Clone, Copy)]
pub struct BoneSegment {
    /// World position of the parent (proximal) joint.
    pub from: Vec3,
    /// World position of this (distal) joint.
    pub to: Vec3,
    /// Whether this bone belongs to the kicking (right) leg.
    pub is_kick_leg: bool,
}

/// Everything the debug view needs for one frame.
#[derive(Debug, Clone)]
pub struct FrameView {
    /// The frame index.
    pub frame: u32,
    /// The phase covering this frame, if any.
    pub phase: Option<PhaseKind>,
    /// World-space joint positions, one per bone.
    pub joints: Vec<Vec3>,
    /// Bone segments to draw.
    pub bones: Vec<BoneSegment>,
    /// World position of the kicking (right) foot.
    pub right_foot: Vec3,
    /// World position of the plant (left) foot.
    pub plant_foot: Vec3,
    /// Events firing on this frame.
    pub events: Vec<AnimationEvent>,
    /// Whether the KickContact event fires on this frame.
    pub is_contact_frame: bool,
}

/// The lab scene: a default humanoid rig and the animation facade.
#[derive(Debug)]
pub struct LabScene {
    api: AnimationApi,
    prefab: HumanoidPrefab,
}

impl Default for LabScene {
    fn default() -> Self {
        Self::new()
    }
}

impl LabScene {
    /// Build the lab scene from the default humanoid prefab.
    pub fn new() -> Self {
        let api = AnimationApi::new();
        let prefab = api.default_humanoid();
        Self { api, prefab }
    }

    /// The rig's skeleton.
    pub fn skeleton(&self) -> &SkeletonDefinition {
        &self.prefab.skeleton
    }

    /// The `kick_right` clip.
    pub fn clip(&self) -> &AnimationClip {
        &self.prefab.clips[0]
    }

    /// Total frames in the kick clip.
    pub fn frame_count(&self) -> u32 {
        self.clip().frame_count
    }

    /// The phase kind covering `frame`, if any (cheap: no pose evaluation).
    pub fn phase_of(&self, frame: u32) -> Option<PhaseKind> {
        self.api.phase_at(self.clip(), frame).map(|p| p.kind)
    }

    /// The solved (limit-clamped) pose sampled at `frame`, as world joints.
    pub fn joints_at(&self, frame: u32) -> Vec<Vec3> {
        let n = self.prefab.skeleton.bone_count();
        let raw = self.api.sample(self.clip(), n, frame);
        let solved = self.api.solve(&self.prefab.joint_limits, &raw);
        self.api
            .world_joint_positions(&self.prefab.skeleton, &self.prefab.bind_pose, &solved)
    }

    /// Assemble the full [`FrameView`] for `frame`.
    pub fn view(&self, frame: u32) -> FrameView {
        let joints = self.joints_at(frame);
        let right = HumanoidPrefab::RIGHT_FOOT_BONE;
        let plant = HumanoidPrefab::LEFT_FOOT_BONE;
        let bones = self
            .prefab
            .skeleton
            .bones
            .iter()
            .enumerate()
            .filter_map(|(i, bone)| {
                bone.parent.map(|p| BoneSegment {
                    from: joints[p],
                    to: joints[i],
                    is_kick_leg: matches!(i, x if x == right || x == 16 || x == 15),
                })
            })
            .collect();
        let events = self.api.events_at(self.clip(), frame);
        let is_contact_frame = events
            .iter()
            .any(|e| matches!(e.kind, axiom_animation::EventKind::KickContact));
        FrameView {
            frame,
            phase: self.api.phase_at(self.clip(), frame).map(|p| p.kind),
            right_foot: joints[right],
            plant_foot: joints[plant],
            joints,
            bones,
            events,
            is_contact_frame,
        }
    }
}

/// The human-readable name of a phase.
pub fn phase_name(phase: Option<PhaseKind>) -> &'static str {
    match phase {
        Some(PhaseKind::Ready) => "ready",
        Some(PhaseKind::LeanForward) => "lean_forward",
        Some(PhaseKind::Approach) => "approach",
        Some(PhaseKind::Plant) => "plant",
        Some(PhaseKind::Backswing) => "backswing",
        Some(PhaseKind::Strike) => "strike",
        Some(PhaseKind::FollowThrough) => "follow_through",
        Some(PhaseKind::Recover) => "recover",
        None => "-",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_count_matches_prefab() {
        let scene = LabScene::new();
        assert_eq!(scene.frame_count(), HumanoidPrefab::KICK_FRAME_COUNT);
        assert_eq!(scene.skeleton().bone_count(), 18);
    }

    #[test]
    fn contact_frame_flagged_only_on_strike_frame() {
        let scene = LabScene::new();
        let strike = HumanoidPrefab::KICK_STRIKE_FRAME;
        assert!(scene.view(strike).is_contact_frame);
        assert!(!scene.view(strike - 1).is_contact_frame);
        assert!(!scene.view(0).is_contact_frame);
    }

    #[test]
    fn rig_pose_changes_across_frames() {
        let scene = LabScene::new();
        // The right foot is in a different place at ready vs strike.
        let ready = scene.view(0).right_foot;
        let strike = scene.view(HumanoidPrefab::KICK_STRIKE_FRAME).right_foot;
        assert!((ready.z - strike.z).abs() > 0.05);
    }

    #[test]
    fn view_is_deterministic() {
        let scene = LabScene::new();
        assert_eq!(scene.joints_at(20), scene.joints_at(20));
    }

    #[test]
    fn phase_names_cover_the_timeline() {
        let scene = LabScene::new();
        assert_eq!(phase_name(scene.view(0).phase), "ready");
        assert_eq!(phase_name(scene.view(33).phase), "strike");
        assert_eq!(phase_name(None), "-");
    }
}
