//! The lab scene: load the kicker figure + kick clip from their portable bytes,
//! then pose the figure per frame.
//!
//! This is the data-driven pipeline the game shares. The lab reads the *authored
//! bytes* (which the emit command writes to the shared asset the game embeds),
//! deserializes the figure and clip through the generic facades, builds the
//! animation skeleton from the figure's parts, samples/resolves the clip, and
//! hands the world transforms to the figure to get renderable boxes. Nothing
//! about the kicker is hard-coded here — swap the bytes and this scrubs any
//! figure.

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
    /// World position of the kicking (right) foot joint.
    pub right_foot: Vec3,
    /// World position of the plant (left) foot joint.
    pub plant_foot: Vec3,
    /// Whether the `KickContact` event fires on this frame.
    pub is_contact_frame: bool,
}

/// The lab scene: a kicker figure and the animation registry driving it.
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
    /// Load the kicker from its authored bytes and wire the animation registry.
    pub fn new() -> Self {
        Self::from_bytes(&authoring::figure_bytes(), &authoring::clip_bytes())
    }

    /// Build the scene from figure + clip bytes — the exact data the game loads.
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

    /// Total frames in the kick clip.
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
        let is_contact_frame = self
            .api
            .events_at(self.clip, tick)
            .map(|codes| codes.contains(&authoring::KICK_CONTACT_CODE))
            .unwrap_or(false);
        FrameView {
            frame,
            phase: self.api.phase_at(self.clip, tick).ok().flatten(),
            right_foot: world[authoring::RIGHT_FOOT].translation,
            plant_foot: world[authoring::LEFT_FOOT].translation,
            parts,
            is_contact_frame,
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
    fn contact_frame_is_flagged_only_on_the_strike_frame() {
        let scene = LabScene::new();
        assert!(scene.view(authoring::CONTACT_FRAME).is_contact_frame);
        assert!(!scene.view(0).is_contact_frame);
        assert!(!scene.view(authoring::CONTACT_FRAME - 1).is_contact_frame);
    }

    #[test]
    fn kick_sweeps_the_right_foot_forward_and_is_deterministic() {
        let scene = LabScene::new();
        assert_eq!(scene.view(20).right_foot, scene.view(20).right_foot);
        // The right foot is well behind at backswing and well forward at strike.
        let back = scene.view(24).right_foot.z;
        let strike = scene.view(authoring::CONTACT_FRAME).right_foot.z;
        assert!(strike - back > 0.4, "foot should sweep forward: back={back}, strike={strike}");
    }

    #[test]
    fn plant_foot_stays_roughly_put() {
        let scene = LabScene::new();
        let a = scene.view(0).plant_foot;
        let b = scene.view(authoring::CONTACT_FRAME).plant_foot;
        assert!((a.z - b.z).abs() < 0.2);
    }
}
