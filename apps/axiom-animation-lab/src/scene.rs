//! The lab scene: load the sample figure + motion clip from their portable
//! bytes, then pose the figure per frame.
//!
//! This is a generic, data-driven pipeline. The lab reads the *authored bytes*
//! (which the emit command can write out), deserializes the figure and clip
//! through the generic facades, builds the animation skeleton from the figure's
//! parts, samples/resolves the clip, and hands the world transforms to the
//! figure to get renderable boxes. Nothing about the specific figure is
//! hard-coded here — swap the bytes and this scrubs any figure.

use axiom_animation::{AnimationApi, BoneId, ClipId, SkeletonId};
use axiom_figure::{FigureApi, FigureDefinition, PosedPart};
use axiom_kernel::Tick;
use axiom_math::{Transform, Vec3};

use crate::authoring;

/// Everything the debug view needs for one frame.
#[derive(Debug, Clone)]
pub struct FrameView {
    /// The frame index.
    pub frame: u32,
    /// The phase code covering this frame, if any.
    pub phase: Option<u32>,
    /// The figure's renderable posed boxes.
    pub parts: Vec<PosedPart>,
    /// World position of the highlighted swinging joint.
    pub swing_joint: Vec3,
    /// World position of the highlighted anchored joint.
    pub anchor_joint: Vec3,
    /// Whether the sample event fires on this frame.
    pub is_event_frame: bool,
}

/// The lab scene: a sample figure and the animation registry driving it.
#[derive(Debug)]
pub struct LabScene {
    figure: FigureDefinition,
    api: AnimationApi,
    skeleton: SkeletonId,
    clip: ClipId,
}

impl Default for LabScene {
    fn default() -> Self {
        Self::new()
    }
}

impl LabScene {
    /// Load the sample figure from its authored bytes and wire the animation
    /// registry.
    pub fn new() -> Self {
        Self::from_bytes(&authoring::figure_bytes(), &authoring::clip_bytes())
    }

    /// Build the scene from figure + clip bytes — any portable figure/clip pair.
    pub fn from_bytes(figure_bytes: &[u8], clip_bytes: &[u8]) -> Self {
        let figure = FigureApi::new().deserialize(figure_bytes).expect("valid figure bytes");
        let mut api = AnimationApi::new();
        let skeleton = api.create_skeleton();
        figure.parts().iter().for_each(|part| {
            match part.parent {
                None => api.add_root_bone(skeleton, part.rest).map(|_| ()),
                Some(parent) => api
                    .add_child_bone(skeleton, BoneId::from_raw(u64::from(parent)), part.rest)
                    .map(|_| ()),
            }
            .expect("figure parts are parent-before-child");
        });
        let clip = api.deserialize_clip(clip_bytes).expect("valid clip bytes");
        Self { figure, api, skeleton, clip }
    }

    /// Total frames in the motion clip.
    pub fn frame_count(&self) -> u32 {
        authoring::FRAME_COUNT
    }

    /// The number of figure parts.
    pub fn part_count(&self) -> usize {
        self.figure.part_count()
    }

    /// The phase code covering `frame`, if any (cheap: no pose evaluation).
    pub fn phase_of(&self, frame: u32) -> Option<u32> {
        self.api.phase_at(self.clip, Tick::new(u64::from(frame))).ok().flatten()
    }

    /// Each part's world transform at `frame` (sampled + resolved).
    fn world_transforms(&self, frame: u32) -> Vec<Transform> {
        let tick = Tick::new(u64::from(frame));
        let pose = self.api.sample(self.skeleton, self.clip, tick).expect("sample");
        let model = self.api.resolve_model(self.skeleton, &pose).expect("resolve");
        (0..self.figure.part_count() as u64)
            .map(|i| model.transform(BoneId::from_raw(i)).unwrap_or(Transform::IDENTITY))
            .collect()
    }

    /// Assemble the full [`FrameView`] for `frame`.
    pub fn view(&self, frame: u32) -> FrameView {
        let world = self.world_transforms(frame);
        let parts = FigureApi::new().posed_parts(&self.figure, &world).expect("posed parts");
        let tick = Tick::new(u64::from(frame));
        let is_event_frame = self
            .api
            .events_at(self.clip, tick)
            .map(|codes| codes.contains(&authoring::EVENT_CODE))
            .unwrap_or(false);
        FrameView {
            frame,
            phase: self.api.phase_at(self.clip, tick).ok().flatten(),
            swing_joint: world[authoring::SWING_JOINT].translation,
            anchor_joint: world[authoring::ANCHOR_JOINT].translation,
            parts,
            is_event_frame,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_loads_figure_and_reports_counts() {
        let scene = LabScene::new();
        assert_eq!(scene.frame_count(), authoring::FRAME_COUNT);
        assert_eq!(scene.part_count(), 13);
        assert_eq!(scene.view(0).parts.len(), 13);
    }

    #[test]
    fn event_frame_is_flagged_only_on_the_event_frame() {
        let scene = LabScene::new();
        assert!(scene.view(authoring::EVENT_FRAME).is_event_frame);
        assert!(!scene.view(0).is_event_frame);
        assert!(!scene.view(authoring::EVENT_FRAME - 1).is_event_frame);
    }

    #[test]
    fn motion_sweeps_the_swing_joint_forward_and_is_deterministic() {
        let scene = LabScene::new();
        assert_eq!(scene.view(20).swing_joint, scene.view(20).swing_joint);
        // The swing joint is well behind at windup and well forward at the event.
        let back = scene.view(24).swing_joint.z;
        let peak = scene.view(authoring::EVENT_FRAME).swing_joint.z;
        assert!(peak - back > 0.4, "joint should sweep forward: back={back}, peak={peak}");
    }

    #[test]
    fn anchor_joint_stays_roughly_put() {
        let scene = LabScene::new();
        let a = scene.view(0).anchor_joint;
        let b = scene.view(authoring::EVENT_FRAME).anchor_joint;
        assert!((a.z - b.z).abs() < 0.2);
    }
}
