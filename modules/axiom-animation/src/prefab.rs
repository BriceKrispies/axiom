//! [`HumanoidPrefab`]: an ordinary, editable low-poly humanoid rig plus an
//! authored right-foot soccer kick.
//!
//! Everything here is *emitted data*, not a hardcoded character: the returned
//! prefab is a plain `SkeletonDefinition` / `BindPose` / limit list / clip list
//! the caller can mutate, extend, or replace. The default rig is an 18-bone
//! humanoid (root → pelvis → spine → chest → neck/head, two arms, two legs), and
//! the `kick_right` clip drives it through the eight-phase kick with a
//! `KickContact` event on the strike frame targeting the right foot.

use axiom_math::Vec3;

use crate::clip::{AnimationClip, BoneTrack, ClipPhase, Keyframe, PhaseKind};
use crate::events::{AnimationEvent, EventKind, EventTrack};
use crate::pose::{BindPose, LocalBoneTransform};
use crate::skeleton::{BoneDefinition, SkeletonDefinition};
use crate::solver::JointLimit;

// Bone indices, in parent-before-child order.
const PELVIS: usize = 1;
const SPINE: usize = 2;
const L_UPPER_ARM: usize = 6;
const L_LOWER_ARM: usize = 7;
const R_UPPER_ARM: usize = 9;
const R_LOWER_ARM: usize = 10;
const L_UPPER_LEG: usize = 12;
const L_LOWER_LEG: usize = 13;
const R_UPPER_LEG: usize = 15;
const R_LOWER_LEG: usize = 16;
const R_FOOT: usize = 17;
const L_FOOT: usize = 14;

/// `(name, parent)` for each of the 18 default bones.
const BONES: [(&str, Option<usize>); 18] = [
    ("root", None),
    ("pelvis", Some(0)),
    ("spine", Some(PELVIS)),
    ("chest", Some(SPINE)),
    ("neck", Some(3)),
    ("head", Some(4)),
    ("left_upper_arm", Some(3)),
    ("left_lower_arm", Some(L_UPPER_ARM)),
    ("left_hand", Some(L_LOWER_ARM)),
    ("right_upper_arm", Some(3)),
    ("right_lower_arm", Some(R_UPPER_ARM)),
    ("right_hand", Some(R_LOWER_ARM)),
    ("left_upper_leg", Some(PELVIS)),
    ("left_lower_leg", Some(L_UPPER_LEG)),
    ("left_foot", Some(L_LOWER_LEG)),
    ("right_upper_leg", Some(PELVIS)),
    ("right_lower_leg", Some(R_UPPER_LEG)),
    ("right_foot", Some(R_LOWER_LEG)),
];

/// Parent-relative rest offset (metres) for each bone, index-aligned with
/// [`BONES`]. Y is up, +Z is the facing/kick direction, +X is the rig's right.
const BIND_OFFSETS: [Vec3; 18] = [
    Vec3::new(0.0, 0.0, 0.0),   // root
    Vec3::new(0.0, 1.0, 0.0),   // pelvis
    Vec3::new(0.0, 0.18, 0.0),  // spine
    Vec3::new(0.0, 0.22, 0.0),  // chest
    Vec3::new(0.0, 0.16, 0.0),  // neck
    Vec3::new(0.0, 0.12, 0.0),  // head
    Vec3::new(-0.18, 0.10, 0.0), // left_upper_arm
    Vec3::new(0.0, -0.27, 0.0), // left_lower_arm
    Vec3::new(0.0, -0.24, 0.0), // left_hand
    Vec3::new(0.18, 0.10, 0.0), // right_upper_arm
    Vec3::new(0.0, -0.27, 0.0), // right_lower_arm
    Vec3::new(0.0, -0.24, 0.0), // right_hand
    Vec3::new(-0.10, -0.08, 0.0), // left_upper_leg
    Vec3::new(0.0, -0.45, 0.0), // left_lower_leg
    Vec3::new(0.0, -0.45, 0.0), // left_foot
    Vec3::new(0.10, -0.08, 0.0), // right_upper_leg
    Vec3::new(0.0, -0.45, 0.0), // right_lower_leg
    Vec3::new(0.0, -0.45, 0.0), // right_foot
];

/// A one-axis hinge about X that cannot bend past straight (`min.x = 0`), with
/// the near-locked cross axes a knee/elbow has.
fn hinge_x(max_bend: f32) -> JointLimit {
    JointLimit::new(
        Vec3::new(0.0, -0.03, -0.03),
        Vec3::new(max_bend, 0.03, 0.03),
    )
}

/// An editable humanoid rig plus its authored clips. Pure data: every field is
/// public and may be mutated or replaced by the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct HumanoidPrefab {
    /// The bone topology.
    pub skeleton: SkeletonDefinition,
    /// The rest pose (one local transform per bone).
    pub bind_pose: BindPose,
    /// Anatomical joint limits (one per bone).
    pub joint_limits: Vec<JointLimit>,
    /// Authored clips; the first is `kick_right`.
    pub clips: Vec<AnimationClip>,
}

impl HumanoidPrefab {
    /// Total frames in the `kick_right` clip.
    pub const KICK_FRAME_COUNT: u32 = 48;
    /// The frame the `KickContact` event fires on.
    pub const KICK_STRIKE_FRAME: u32 = 33;
    /// Bone index of the kicking (right) foot — the plant is the left foot.
    pub const RIGHT_FOOT_BONE: usize = R_FOOT;
    /// Bone index of the support (plant) foot.
    pub const LEFT_FOOT_BONE: usize = L_FOOT;

    /// Build the default 18-bone humanoid with its `kick_right` clip.
    pub fn default_humanoid() -> Self {
        Self {
            skeleton: Self::default_skeleton(),
            bind_pose: Self::default_bind_pose(),
            joint_limits: Self::default_joint_limits(),
            clips: vec![Self::kick_right_clip()],
        }
    }

    /// The default bone topology (root, pelvis, spine, chest, neck, head, both
    /// arms, both legs).
    pub fn default_skeleton() -> SkeletonDefinition {
        SkeletonDefinition::new(
            BONES
                .iter()
                .map(|(name, parent)| BoneDefinition {
                    name: (*name).to_string(),
                    parent: *parent,
                })
                .collect(),
        )
    }

    /// The default rest pose: each bone at its fixed parent-relative offset with
    /// identity rotation.
    pub fn default_bind_pose() -> BindPose {
        BindPose::new(
            BIND_OFFSETS
                .iter()
                .map(|&o| LocalBoneTransform::offset(o))
                .collect(),
        )
    }

    /// The default per-bone joint limits: free everywhere except the knees and
    /// elbows, which are one-sided hinges that cannot bend backward.
    pub fn default_joint_limits() -> Vec<JointLimit> {
        let mut limits = vec![JointLimit::free(); BONES.len()];
        [L_LOWER_ARM, R_LOWER_ARM]
            .iter()
            .for_each(|&i| limits[i] = hinge_x(2.6));
        [L_LOWER_LEG, R_LOWER_LEG]
            .iter()
            .for_each(|&i| limits[i] = hinge_x(2.4));
        limits
    }

    /// The authored right-foot kick: eight phases from ready to recover, with a
    /// `KickContact` event on the strike frame targeting the right foot and a
    /// `FootPlant` on the plant frame targeting the left foot.
    pub fn kick_right_clip() -> AnimationClip {
        AnimationClip::new(
            "kick_right",
            Self::KICK_FRAME_COUNT,
            Self::kick_tracks(),
            Self::kick_phases(),
            EventTrack::new(vec![
                AnimationEvent::new(18, EventKind::FootPlant, L_FOOT),
                AnimationEvent::new(Self::KICK_STRIKE_FRAME, EventKind::KickContact, R_FOOT),
            ]),
        )
    }

    /// The eight ordered phases of the kick.
    fn kick_phases() -> Vec<ClipPhase> {
        vec![
            ClipPhase::new(PhaseKind::Ready, 0, 6),
            ClipPhase::new(PhaseKind::LeanForward, 6, 12),
            ClipPhase::new(PhaseKind::Approach, 12, 18),
            ClipPhase::new(PhaseKind::Plant, 18, 24),
            ClipPhase::new(PhaseKind::Backswing, 24, 30),
            ClipPhase::new(PhaseKind::Strike, 30, 36),
            ClipPhase::new(PhaseKind::FollowThrough, 36, 42),
            ClipPhase::new(PhaseKind::Recover, 42, 48),
        ]
    }

    /// The per-bone rotation tracks driving the kick. Angles are radians about X
    /// (forward/back swing and hinge bend); the right leg cocks back through
    /// backswing and drives forward through strike while the knee extends.
    fn kick_tracks() -> Vec<BoneTrack> {
        vec![
            pitch_track(SPINE, &[(0, 0.0), (9, 0.18), (33, 0.12), (47, 0.05)]),
            pitch_track(
                R_UPPER_LEG,
                &[
                    (0, 0.0),
                    (15, -0.15),
                    (21, 0.10),
                    (27, 0.70),
                    (33, -0.90),
                    (39, -0.50),
                    (47, 0.0),
                ],
            ),
            pitch_track(
                R_LOWER_LEG,
                &[(0, 0.15), (27, 1.20), (33, 0.10), (39, 0.50), (47, 0.20)],
            ),
            pitch_track(R_UPPER_ARM, &[(0, 0.0), (27, -0.40), (33, 0.50), (47, 0.0)]),
            pitch_track(L_UPPER_ARM, &[(0, 0.0), (27, 0.40), (33, -0.50), (47, 0.0)]),
            pitch_track(L_UPPER_LEG, &[(0, 0.0), (21, -0.10), (47, 0.0)]),
            pitch_track(L_LOWER_LEG, &[(0, 0.10), (21, 0.30), (47, 0.10)]),
        ]
    }
}

/// Build a track that rotates `bone` about X, from `(frame, radians)` pairs.
fn pitch_track(bone: usize, keys: &[(u32, f32)]) -> BoneTrack {
    BoneTrack::new(
        bone,
        keys.iter()
            .map(|&(frame, x)| Keyframe::new(frame, Vec3::new(x, 0.0, 0.0)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_humanoid_is_well_formed() {
        let p = HumanoidPrefab::default_humanoid();
        assert_eq!(p.skeleton.bone_count(), 18);
        assert_eq!(p.bind_pose.bone_count(), 18);
        assert_eq!(p.joint_limits.len(), 18);
        assert_eq!(p.clips.len(), 1);
        assert_eq!(p.skeleton.validate(), Ok(()));
        assert_eq!(p.skeleton.bone_index("right_foot"), Some(R_FOOT));
        assert_eq!(p.skeleton.bone_index("left_foot"), Some(L_FOOT));
    }

    #[test]
    fn knees_and_elbows_are_one_sided_hinges() {
        let limits = HumanoidPrefab::default_joint_limits();
        [L_LOWER_LEG, R_LOWER_LEG, L_LOWER_ARM, R_LOWER_ARM]
            .iter()
            .for_each(|&i| assert_eq!(limits[i].min.x, 0.0));
        // A non-hinge bone stays free.
        assert!(limits[SPINE].min.x < 0.0);
    }

    #[test]
    fn kick_clip_has_ordered_phases_and_contact_event() {
        let clip = HumanoidPrefab::kick_right_clip();
        assert_eq!(clip.name, "kick_right");
        assert_eq!(clip.frame_count, HumanoidPrefab::KICK_FRAME_COUNT);
        assert_eq!(
            clip.phase_kinds(),
            vec![
                PhaseKind::Ready,
                PhaseKind::LeanForward,
                PhaseKind::Approach,
                PhaseKind::Plant,
                PhaseKind::Backswing,
                PhaseKind::Strike,
                PhaseKind::FollowThrough,
                PhaseKind::Recover,
            ]
        );
        let contact = clip.events.at(HumanoidPrefab::KICK_STRIKE_FRAME);
        assert_eq!(
            contact,
            vec![AnimationEvent::new(
                HumanoidPrefab::KICK_STRIKE_FRAME,
                EventKind::KickContact,
                R_FOOT
            )]
        );
    }

    #[test]
    fn pitch_track_places_angle_on_x_axis() {
        let t = pitch_track(3, &[(0, 0.0), (4, 0.5)]);
        assert_eq!(t.bone, 3);
        assert_eq!(t.keys[1].euler, Vec3::new(0.5, 0.0, 0.0));
    }
}
