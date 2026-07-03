//! The lab scene: turn a frame number into solved, world-space joint data.
//!
//! This is app composition glue — it drives the `axiom-animation` facade
//! (sample → joint-limit clamp → forward kinematics) and reads the neutral
//! `Transform`/`Vec3` results back out. The determinism is the module's;
//! sampling frame `f` always yields the same pose. Apps may branch.

use axiom_animation::AnimationApi;
use axiom_kernel::Tick;
use axiom_math::Vec3;

use crate::rig::{self, KickPhase, Rig, KICK_CONTACT, KICK_FRAME_COUNT};

/// A bone segment to draw: the world endpoints and whether it is the kick leg.
#[derive(Debug, Clone, Copy)]
pub struct Segment {
    pub from: Vec3,
    pub to: Vec3,
    pub is_kick_leg: bool,
}

/// Everything a view needs for one frame.
#[derive(Debug, Clone)]
pub struct FrameView {
    pub frame: u32,
    pub phase: Option<KickPhase>,
    pub segments: Vec<Segment>,
    pub right_foot: Vec3,
    pub left_foot: Vec3,
    pub is_contact_frame: bool,
}

/// The lab scene: the animation registry and the built humanoid rig.
pub struct LabScene {
    api: AnimationApi,
    rig: Rig,
}

impl Default for LabScene {
    fn default() -> Self {
        Self::new()
    }
}

impl LabScene {
    /// Build the scene by authoring the humanoid + kick through the facade.
    pub fn new() -> Self {
        let mut api = AnimationApi::new();
        let rig = rig::build(&mut api);
        Self { api, rig }
    }

    /// Total frames in the kick clip.
    pub fn frame_count(&self) -> u32 {
        KICK_FRAME_COUNT
    }

    /// The kick phase covering `frame`, if any.
    pub fn phase_of(&self, frame: u32) -> Option<KickPhase> {
        self.api
            .phase_at(self.rig.clip, Tick::new(u64::from(frame)))
            .unwrap()
            .and_then(KickPhase::from_code)
    }

    /// The world-space joint position of every bone at `frame`, after the pose
    /// is sampled, joint-limit-clamped, and forward-kinematics-solved.
    fn joints_at(&self, frame: u32) -> Vec<Vec3> {
        let tick = Tick::new(u64::from(frame));
        let pose = self.api.sample(self.rig.skeleton, self.rig.clip, tick).unwrap();
        // Rebuild the JointLimit values inline (the type is not nameable here).
        let limits: Vec<_> = self
            .rig
            .limit_specs
            .iter()
            .map(|&(bone, min, max)| AnimationApi::joint_limit(bone, min, max).unwrap())
            .collect();
        let clamped = self.api.clamp_pose(&limits, &pose);
        let model = self.api.resolve_model(self.rig.skeleton, &clamped).unwrap();
        (0..self.api.bone_count(self.rig.skeleton).unwrap())
            .map(|i| {
                model
                    .position(axiom_animation::BoneId::from_raw(i as u64))
                    .unwrap_or(Vec3::ZERO)
            })
            .collect()
    }

    /// Assemble the full [`FrameView`] for `frame`.
    pub fn view(&self, frame: u32) -> FrameView {
        let joints = self.joints_at(frame);
        let segments = self
            .rig
            .segments
            .iter()
            .zip(self.rig.is_kick_leg.iter().skip(1))
            .map(|(&(bone, parent), &is_kick_leg)| Segment {
                from: joints[parent.raw() as usize],
                to: joints[bone.raw() as usize],
                is_kick_leg,
            })
            .collect();
        let events = self
            .api
            .events_at(self.rig.clip, Tick::new(u64::from(frame)))
            .unwrap();
        FrameView {
            frame,
            phase: self.phase_of(frame),
            right_foot: joints[self.rig.right_foot.raw() as usize],
            left_foot: joints[self.rig.left_foot.raw() as usize],
            segments,
            is_contact_frame: events.contains(&KICK_CONTACT),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_reports_frame_count_and_phases() {
        let scene = LabScene::new();
        assert_eq!(scene.frame_count(), KICK_FRAME_COUNT);
        assert_eq!(scene.phase_of(0), Some(KickPhase::Ready));
        assert_eq!(scene.phase_of(33), Some(KickPhase::Strike));
    }

    #[test]
    fn contact_frame_is_flagged_only_on_the_strike_frame() {
        let scene = LabScene::new();
        assert!(scene.view(rig::KICK_STRIKE_FRAME).is_contact_frame);
        assert!(!scene.view(rig::KICK_STRIKE_FRAME - 1).is_contact_frame);
        assert!(!scene.view(0).is_contact_frame);
    }

    #[test]
    fn kick_pose_moves_the_right_foot_and_is_deterministic() {
        let scene = LabScene::new();
        let ready = scene.view(0).right_foot;
        let strike = scene.view(rig::KICK_STRIKE_FRAME).right_foot;
        assert!((ready.z - strike.z).abs() > 0.05);
        // Re-sampling the same frame reproduces the same joints byte-for-byte.
        let a = scene.view(20);
        let b = scene.view(20);
        assert_eq!(a.right_foot, b.right_foot);
        assert_eq!(a.segments.len(), 17);
    }

    #[test]
    fn kick_sweeps_the_right_foot_forward_across_the_clip() {
        let scene = LabScene::new();
        let zs: Vec<f32> = (0..scene.frame_count()).map(|f| scene.view(f).right_foot.z).collect();
        // Every frame produces finite joint data (sample → clamp → FK never NaNs)…
        assert!(zs.iter().all(|z| z.is_finite()));
        // …and the kick sweeps the foot through a wide back-to-front arc.
        let min = zs.iter().copied().fold(f32::INFINITY, f32::min);
        let max = zs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(max - min > 1.0, "kick should sweep a wide arc, got {}", max - min);
    }
}
