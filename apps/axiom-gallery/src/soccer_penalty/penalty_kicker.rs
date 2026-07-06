//! The articulated, data-driven penalty kicker.
//!
//! The kicker is no longer a frozen box puppet: it is the shared **figure +
//! clip** data (authored in `axiom-animation-lab`, emitted to
//! `assets/soccer/`) posed per frame. This module embeds those exact bytes,
//! rebuilds the animation skeleton, samples the kick, and hands back the
//! kicker's boxes in world space — placed at the kicker's spot and facing the
//! goal. Tuning the kick in the lab and re-emitting the assets updates the game
//! 1-1. The frame that plays is driven by the shot: the strike lands as the ball
//! is struck.

use axiom_animation::{AnimationApi, BoneId, ClipId, SkeletonId};
use axiom_figure::{FigureApi, FigureDefinition};
use axiom_kernel::Tick;
use axiom_math::{Transform, Vec3};

use crate::soccer_penalty::penalty_ball::PenaltyBallState;
use crate::soccer_penalty::penalty_interaction::PenaltyInteractionState;
use crate::soccer_penalty::penalty_materials::PenaltyMaterialId;
use crate::soccer_penalty::penalty_scene::{KICKER_X, KICKER_Z, PENALTY_SPOT_Z};

/// The shared kicker assets, byte-identical to what the lab emits.
const FIGURE_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/soccer/kicker.figure"));
const CLIP_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/soccer/kick_right.clip"));

/// Stable, greppable render labels for the 13 kicker parts, in figure order.
pub const KICKER_LABELS: [&str; 13] = [
    "kicker.pelvis",
    "kicker.chest",
    "kicker.head",
    "kicker.l_thigh",
    "kicker.l_shin",
    "kicker.l_foot",
    "kicker.r_thigh",
    "kicker.r_shin",
    "kicker.r_foot",
    "kicker.l_upper_arm",
    "kicker.l_forearm",
    "kicker.r_upper_arm",
    "kicker.r_forearm",
];

/// The rest/idle frame: the shared figure's authored T-pose rest (arms out
/// horizontal, legs straight). Used by the rig tests as a stable reference frame.
pub const IDLE_FRAME: u32 = 0;

/// The frame the static diorama poses the kicker at. Frame 0 is the figure's
/// limp rest — arms flung straight out to the sides and legs unbent, a scarecrow
/// T-pose that reads dead against a run-up reference. Frame 26 is the
/// planted/cocked run-up pose (the same one the live gameplay holds while the
/// ball is at the spot): support leg planted, kicking leg wound back, weight
/// carried forward — a braced, weighted athlete mid-approach rather than a
/// mannequin. This is a render/display frame choice only; it does not touch the
/// clip data or the gameplay strike timing.
pub const DISPLAY_FRAME: u32 = 26;

/// One posed kicker box, ready for [`crate::soccer_penalty::penalty_scene`] to
/// emit or for the per-frame overlay to reposition.
#[derive(Debug, Clone, Copy)]
pub struct KickerBox {
    /// World-space box center.
    pub center: Vec3,
    /// Full box extents.
    pub size: Vec3,
    /// The material this part is drawn with.
    pub material: PenaltyMaterialId,
    /// The part's stable render label.
    pub label: &'static str,
}

/// The kicker rig: the shared figure plus the animation registry driving it.
#[derive(Debug)]
pub struct KickerRig {
    figure: FigureDefinition,
    api: AnimationApi,
    skeleton: SkeletonId,
    clip: ClipId,
}

impl KickerRig {
    /// Load the kicker from the embedded shared assets.
    pub fn new() -> Self {
        let figure = FigureApi::new().deserialize(FIGURE_BYTES).expect("embedded kicker figure");
        let mut api = AnimationApi::new();
        let skeleton = api.create_skeleton();
        for part in figure.parts() {
            match part.parent {
                None => {
                    api.add_root_bone(skeleton, part.rest).expect("root bone");
                }
                Some(parent) => {
                    api.add_child_bone(skeleton, BoneId::from_raw(u64::from(parent)), part.rest)
                        .expect("child bone");
                }
            }
        }
        let clip = api.deserialize_clip(CLIP_BYTES).expect("embedded kick clip");
        Self { figure, api, skeleton, clip }
    }

    /// The kicker's posed boxes at `frame`, in world space at the kicker's spot
    /// and facing the goal (`+Z` figure-forward mapped to `-Z` world-forward).
    pub fn boxes_at(&self, frame: u32) -> Vec<KickerBox> {
        let pose = self.api.sample(self.skeleton, self.clip, Tick::new(u64::from(frame))).expect("sample");
        let model = self.api.resolve_model(self.skeleton, &pose).expect("resolve");
        let world: Vec<Transform> = (0..self.figure.part_count() as u64)
            .map(|i| model.transform(BoneId::from_raw(i)).unwrap_or(Transform::IDENTITY))
            .collect();
        let posed = FigureApi::new().posed_parts(&self.figure, &world).expect("posed parts");
        posed
            .iter()
            .enumerate()
            .map(|(i, pp)| {
                let p = pp.transform.translation;
                KickerBox {
                    center: Vec3::new(KICKER_X + p.x, p.y, KICKER_Z - p.z),
                    size: pp.box_size,
                    material: material_for_part(i, pp.tag),
                    label: KICKER_LABELS[i],
                }
            })
            .collect()
    }
}

impl Default for KickerRig {
    fn default() -> Self {
        Self::new()
    }
}

/// Map an opaque figure tag to a soccer material.
fn material_for(tag: u32) -> PenaltyMaterialId {
    match tag {
        0 => PenaltyMaterialId::KickerJerseyBlue,
        1 => PenaltyMaterialId::KickerShortsWhite,
        2 => PenaltyMaterialId::KickerSkin,
        _ => PenaltyMaterialId::KickerSocksDark, // socks + boots
    }
}

/// Map a kicker *part* to its material.
///
/// The opaque figure tag alone can't tell three kit surfaces apart: the head
/// (index 2) shares `TAG_SKIN` with the thighs and forearms, and the shins
/// (indices 4, 7) and feet (indices 5, 8) share `TAG_LIMB`/`TAG_END` and both
/// fell to one near-black `KickerSocksDark`. The reference kit, seen from
/// behind, is dark hair on the head, royal-blue socks below the knee, and black
/// boots — so those three parts are resolved by index here; everything else
/// (jersey, shorts, skin) still falls back to the tag-based [`material_for`].
///
/// The **upper arms** (indices 9, 11) are also resolved by index to the jersey
/// blue: the reference #10 wears a short-sleeve kit whose blue sleeve covers the
/// upper arm (bare skin begins at the forearm). This is not only on-model — it is
/// what re-attaches the arms to the body. The athletes bake as one continuous
/// `MetaSurface` **per kit material** (see `body_groups`), so a skin-tagged upper
/// arm baked in the *skin* group could never fuse to the *jersey* torso surface
/// and read as a detached capsule floating at the shoulder. Grouping the upper
/// arm with the jersey torso lets the smooth-union weld the shoulder into one
/// surface, moving the only kit seam down to the elbow — a natural sleeve edge.
fn material_for_part(index: usize, tag: u32) -> PenaltyMaterialId {
    match index {
        2 => PenaltyMaterialId::KickerHair,            // head: dark hair, not bald skin
        4 | 7 => PenaltyMaterialId::KickerSocksBlue,   // shins: royal-blue socks
        5 | 8 => PenaltyMaterialId::KickerShoes,       // feet: black boots
        9 | 11 => PenaltyMaterialId::KickerJerseyBlue, // upper arms: blue sleeve, fuses to torso
        _ => material_for(tag),
    }
}

/// The kick frame to show for the current shot: planted/cocked while aiming, the
/// strike frame (33) as the ball is struck, then follow-through as the ball
/// travels toward the goal.
pub fn kicker_frame(state: &PenaltyInteractionState) -> u32 {
    match state.ball_state() {
        PenaltyBallState::AtPenaltySpot => 26,
        _ => {
            let ball_z = state.ball_pose().position.z;
            let progress = ((PENALTY_SPOT_Z - ball_z) / PENALTY_SPOT_Z).clamp(0.0, 1.0);
            33 + (progress * 14.0) as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soccer_penalty::penalty_input::PenaltyInputIntent;

    #[test]
    fn rig_loads_and_poses_thirteen_boxes() {
        let rig = KickerRig::new();
        let boxes = rig.boxes_at(IDLE_FRAME);
        assert_eq!(boxes.len(), 13);
        // All parts sit at/near the kicker's spot in Z (a metre or so of rig depth).
        assert!(boxes.iter().all(|b| (b.center.z - KICKER_Z).abs() < 2.0));
        assert_eq!(boxes[0].label, "kicker.pelvis");
    }

    #[test]
    fn right_foot_sweeps_toward_the_goal_across_the_kick() {
        let rig = KickerRig::new();
        // Right foot is part index 8; world -Z is toward the goal.
        let back = rig.boxes_at(24)[8].center.z;
        let strike = rig.boxes_at(33)[8].center.z;
        assert!(back - strike > 0.4, "foot should move toward goal (-Z): back={back}, strike={strike}");
    }

    #[test]
    fn kick_frame_is_cocked_while_aiming_and_strikes_in_flight() {
        // At rest / aiming the kicker is cocked (frame 26), never past the strike.
        let aiming = PenaltyInteractionState::start();
        assert_eq!(kicker_frame(&aiming), 26);
        // Drive a full shot; once airborne the frame is at/after the strike.
        let intents = vec![PenaltyInputIntent::charging(0, 0); 40];
        let launched = PenaltyInteractionState::run(&intents);
        assert!(kicker_frame(&launched) >= 33 || launched.ball_state() == PenaltyBallState::AtPenaltySpot);
    }

    #[test]
    fn material_mapping_covers_every_tag() {
        assert_eq!(material_for(0), PenaltyMaterialId::KickerJerseyBlue);
        assert_eq!(material_for(1), PenaltyMaterialId::KickerShortsWhite);
        assert_eq!(material_for(2), PenaltyMaterialId::KickerSkin);
        assert_eq!(material_for(4), PenaltyMaterialId::KickerSocksDark);
    }

    #[test]
    fn head_shins_and_feet_are_resolved_by_part_index() {
        // Head (index 2) is dark hair, not the skin its tag would give.
        assert_eq!(material_for_part(2, 2), PenaltyMaterialId::KickerHair);
        // Shins (4, 7) are royal-blue socks; feet (5, 8) are black boots.
        assert_eq!(material_for_part(4, 3), PenaltyMaterialId::KickerSocksBlue);
        assert_eq!(material_for_part(7, 3), PenaltyMaterialId::KickerSocksBlue);
        assert_eq!(material_for_part(5, 4), PenaltyMaterialId::KickerShoes);
        assert_eq!(material_for_part(8, 4), PenaltyMaterialId::KickerShoes);
        // Upper arms (9, 11) are the blue jersey sleeve, so they bake into the
        // torso's MetaSurface group and fuse at the shoulder (bare-skin tag notwithstanding).
        assert_eq!(material_for_part(9, 2), PenaltyMaterialId::KickerJerseyBlue);
        assert_eq!(material_for_part(11, 2), PenaltyMaterialId::KickerJerseyBlue);
        // Everything else falls back to the tag: jersey, shorts, skin (thigh/forearm).
        assert_eq!(material_for_part(1, 0), PenaltyMaterialId::KickerJerseyBlue);
        assert_eq!(material_for_part(0, 1), PenaltyMaterialId::KickerShortsWhite);
        assert_eq!(material_for_part(3, 2), PenaltyMaterialId::KickerSkin);
        assert_eq!(material_for_part(10, 2), PenaltyMaterialId::KickerSkin);
    }

    #[test]
    fn head_reads_as_hair_and_hands_stay_skin_in_world() {
        let boxes = KickerRig::new().boxes_at(IDLE_FRAME);
        assert_eq!(boxes[2].label, "kicker.head");
        assert_eq!(boxes[2].material, PenaltyMaterialId::KickerHair);
        // Forearm/hand tips keep skin so only the head turns to hair.
        assert_eq!(boxes[10].material, PenaltyMaterialId::KickerSkin);
        assert_eq!(boxes[12].material, PenaltyMaterialId::KickerSkin);
    }
}
