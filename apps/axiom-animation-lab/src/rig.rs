//! The humanoid rig and its right-foot soccer kick — the *content* (meaning).
//!
//! None of this lives in the engine: `axiom-animation` owns only the mechanism
//! (skeletons, poses, clip sampling, joint limits, events, blending). The
//! humanoid's 18 bones, the kick's authored keyframes, the named kick phases,
//! and the `KickContact` event are all *meaning*, so they live here in the app
//! and are authored through the module's `AnimationApi` facade. Apps may branch,
//! so this file uses ordinary control flow.

use axiom_animation::{AnimationApi, BoneId, ClipId, SkeletonId};
use axiom_kernel::Tick;
use axiom_math::{Quat, Transform, Vec3};

/// The opaque event code the app assigns to "the kicking foot strikes the ball".
pub const KICK_CONTACT: u32 = 1;
/// The opaque event code for "the plant foot lands".
pub const FOOT_PLANT: u32 = 2;

/// Total frames in the kick clip.
pub const KICK_FRAME_COUNT: u32 = 48;
/// The frame the `KickContact` event fires on.
pub const KICK_STRIKE_FRAME: u32 = 33;
/// The plant (left) foot lands on this frame.
pub const FOOT_PLANT_FRAME: u32 = 18;

// Bone indices, parent-before-child.
const SPINE: usize = 2;
const L_UPPER_ARM: usize = 6;
const L_LOWER_ARM: usize = 7;
const R_UPPER_ARM: usize = 9;
const R_LOWER_ARM: usize = 10;
const L_UPPER_LEG: usize = 12;
const L_LOWER_LEG: usize = 13;
const L_FOOT: usize = 14;
const R_UPPER_LEG: usize = 15;
const R_LOWER_LEG: usize = 16;
const R_FOOT: usize = 17;

/// `(name, parent)` for each of the 18 humanoid bones.
const BONES: [(&str, Option<usize>); 18] = [
    ("root", None),
    ("pelvis", Some(0)),
    ("spine", Some(1)),
    ("chest", Some(SPINE)),
    ("neck", Some(3)),
    ("head", Some(4)),
    ("left_upper_arm", Some(3)),
    ("left_lower_arm", Some(L_UPPER_ARM)),
    ("left_hand", Some(L_LOWER_ARM)),
    ("right_upper_arm", Some(3)),
    ("right_lower_arm", Some(R_UPPER_ARM)),
    ("right_hand", Some(R_LOWER_ARM)),
    ("left_upper_leg", Some(1)),
    ("left_lower_leg", Some(L_UPPER_LEG)),
    ("left_foot", Some(L_LOWER_LEG)),
    ("right_upper_leg", Some(1)),
    ("right_lower_leg", Some(R_UPPER_LEG)),
    ("right_foot", Some(R_LOWER_LEG)),
];

/// Parent-relative rest offset (metres) per bone; Y up, +Z forward, +X right.
const BIND_OFFSETS: [Vec3; 18] = [
    Vec3::new(0.0, 0.0, 0.0),
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(0.0, 0.18, 0.0),
    Vec3::new(0.0, 0.22, 0.0),
    Vec3::new(0.0, 0.16, 0.0),
    Vec3::new(0.0, 0.12, 0.0),
    Vec3::new(-0.18, 0.10, 0.0),
    Vec3::new(0.0, -0.27, 0.0),
    Vec3::new(0.0, -0.24, 0.0),
    Vec3::new(0.18, 0.10, 0.0),
    Vec3::new(0.0, -0.27, 0.0),
    Vec3::new(0.0, -0.24, 0.0),
    Vec3::new(-0.10, -0.08, 0.0),
    Vec3::new(0.0, -0.45, 0.0),
    Vec3::new(0.0, -0.45, 0.0),
    Vec3::new(0.10, -0.08, 0.0),
    Vec3::new(0.0, -0.45, 0.0),
    Vec3::new(0.0, -0.45, 0.0),
];

/// A per-bone pitch (rotation-about-X) track: `(frame, radians)` keys.
struct PitchTrack {
    bone: usize,
    keys: &'static [(u32, f32)],
}

const KICK_TRACKS: &[PitchTrack] = &[
    PitchTrack { bone: SPINE, keys: &[(0, 0.0), (9, 0.18), (33, 0.12), (47, 0.05)] },
    PitchTrack {
        bone: R_UPPER_LEG,
        keys: &[(0, 0.0), (15, -0.15), (21, 0.10), (27, 0.70), (33, -0.90), (39, -0.50), (47, 0.0)],
    },
    PitchTrack { bone: R_LOWER_LEG, keys: &[(0, 0.15), (27, 1.20), (33, 0.10), (39, 0.50), (47, 0.20)] },
    PitchTrack { bone: R_UPPER_ARM, keys: &[(0, 0.0), (27, -0.40), (33, 0.50), (47, 0.0)] },
    PitchTrack { bone: L_UPPER_ARM, keys: &[(0, 0.0), (27, 0.40), (33, -0.50), (47, 0.0)] },
    PitchTrack { bone: L_UPPER_LEG, keys: &[(0, 0.0), (21, -0.10), (47, 0.0)] },
    PitchTrack { bone: L_LOWER_LEG, keys: &[(0, 0.10), (21, 0.30), (47, 0.10)] },
];

/// The eight kick phases, in order, as `(phase, start, end)` frame spans.
pub const KICK_PHASES: [(KickPhase, u32, u32); 8] = [
    (KickPhase::Ready, 0, 6),
    (KickPhase::LeanForward, 6, 12),
    (KickPhase::Approach, 12, 18),
    (KickPhase::Plant, 18, 24),
    (KickPhase::Backswing, 24, 30),
    (KickPhase::Strike, 30, 36),
    (KickPhase::FollowThrough, 36, 42),
    (KickPhase::Recover, 42, 48),
];

/// A named phase of the kick. This is *meaning* — the module only sees the
/// opaque `u32` code each phase maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KickPhase {
    Ready,
    LeanForward,
    Approach,
    Plant,
    Backswing,
    Strike,
    FollowThrough,
    Recover,
}

impl KickPhase {
    /// The opaque code this phase is stored under in the clip.
    pub fn code(self) -> u32 {
        match self {
            KickPhase::Ready => 0,
            KickPhase::LeanForward => 1,
            KickPhase::Approach => 2,
            KickPhase::Plant => 3,
            KickPhase::Backswing => 4,
            KickPhase::Strike => 5,
            KickPhase::FollowThrough => 6,
            KickPhase::Recover => 7,
        }
    }

    /// Recover the phase from its stored code.
    pub fn from_code(code: u32) -> Option<KickPhase> {
        KICK_PHASES.iter().map(|&(p, _, _)| p).find(|p| p.code() == code)
    }

    /// A short human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            KickPhase::Ready => "ready",
            KickPhase::LeanForward => "lean_forward",
            KickPhase::Approach => "approach",
            KickPhase::Plant => "plant",
            KickPhase::Backswing => "backswing",
            KickPhase::Strike => "strike",
            KickPhase::FollowThrough => "follow_through",
            KickPhase::Recover => "recover",
        }
    }
}

/// The authored rig: the handles and metadata the scene needs after the humanoid
/// and its kick have been built through the facade.
pub struct Rig {
    /// The humanoid skeleton handle.
    pub skeleton: SkeletonId,
    /// The `kick_right` clip handle.
    pub clip: ClipId,
    /// Anatomical joint-limit specs `(bone, euler-min, euler-max)`. The module's
    /// `JointLimit` value type is not nameable from an app (it is not part of the
    /// facade's id vocabulary), so the app holds the raw specs and rebuilds the
    /// `JointLimit`s inline via `AnimationApi::joint_limit` where they are used.
    pub limit_specs: Vec<(BoneId, Vec3, Vec3)>,
    /// `(bone, parent-bone)` pairs for drawing segments.
    pub segments: Vec<(BoneId, BoneId)>,
    /// Whether each bone belongs to the kicking (right) leg.
    pub is_kick_leg: Vec<bool>,
    /// The right (kicking) foot bone.
    pub right_foot: BoneId,
    /// The left (plant) foot bone.
    pub left_foot: BoneId,
}

/// The local rest transform of bone `i` (bind offset, identity rotation).
fn rest_local(i: usize) -> Transform {
    Transform::from_translation(BIND_OFFSETS[i])
}

/// The animated local transform for bone `i` at pitch `x` (radians about X):
/// the bind offset plus the joint rotation.
fn posed_local(i: usize, x: f32) -> Transform {
    Transform::new(BIND_OFFSETS[i], Quat::from_euler_xyz(x, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0))
}

/// Build the humanoid and its `kick_right` clip through the `AnimationApi`
/// facade, returning the handles + drawing metadata.
pub fn build(api: &mut AnimationApi) -> Rig {
    // 1. Skeleton: add each bone in parent-before-child order.
    let skeleton = api.create_skeleton();
    let mut bones: Vec<BoneId> = Vec::with_capacity(BONES.len());
    for (i, (_, parent)) in BONES.iter().enumerate() {
        let bone = match parent {
            None => api.add_root_bone(skeleton, rest_local(i)).unwrap(),
            Some(p) => api.add_child_bone(skeleton, bones[*p], rest_local(i)).unwrap(),
        };
        bones.push(bone);
    }

    // 2. Clip: one pitch track per animated bone.
    let clip = api.create_clip();
    for track in KICK_TRACKS {
        let keys: Vec<(Tick, Transform)> = track
            .keys
            .iter()
            .map(|&(frame, x)| (Tick::new(u64::from(frame)), posed_local(track.bone, x)))
            .collect();
        api.add_track(clip, bones[track.bone], &keys).unwrap();
    }

    // 3. Phases + events (opaque codes; meaning stays in this app).
    for &(phase, start, end) in &KICK_PHASES {
        api.add_phase(clip, Tick::new(u64::from(start)), Tick::new(u64::from(end)), phase.code())
            .unwrap();
    }
    api.add_event(clip, Tick::new(u64::from(FOOT_PLANT_FRAME)), FOOT_PLANT).unwrap();
    api.add_event(clip, Tick::new(u64::from(KICK_STRIKE_FRAME)), KICK_CONTACT).unwrap();

    // 4. Joint-limit specs: one-sided hinges on knees and elbows, free elsewhere.
    let limit_specs = vec![
        hinge_spec(bones[L_LOWER_ARM], 2.6),
        hinge_spec(bones[R_LOWER_ARM], 2.6),
        hinge_spec(bones[L_LOWER_LEG], 2.4),
        hinge_spec(bones[R_LOWER_LEG], 2.4),
    ];

    // 5. Drawing metadata.
    let segments = BONES
        .iter()
        .enumerate()
        .filter_map(|(i, (_, parent))| parent.map(|p| (bones[i], bones[p])))
        .collect();
    let is_kick_leg = (0..BONES.len())
        .map(|i| i == R_UPPER_LEG || i == R_LOWER_LEG || i == R_FOOT)
        .collect();

    Rig {
        skeleton,
        clip,
        limit_specs,
        segments,
        is_kick_leg,
        right_foot: bones[R_FOOT],
        left_foot: bones[L_FOOT],
    }
}

/// The spec for a one-axis hinge about X that cannot bend past straight
/// (`min.x = 0`), with the near-locked cross axes a knee/elbow has.
fn hinge_spec(bone: BoneId, max_bend: f32) -> (BoneId, Vec3, Vec3) {
    (bone, Vec3::new(0.0, -0.03, -0.03), Vec3::new(max_bend, 0.03, 0.03))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_codes_round_trip() {
        for &(phase, _, _) in &KICK_PHASES {
            assert_eq!(KickPhase::from_code(phase.code()), Some(phase));
        }
        assert_eq!(KickPhase::from_code(99), None);
        assert_eq!(KickPhase::Strike.name(), "strike");
    }

    #[test]
    fn build_produces_an_18_bone_rig_with_a_kick_clip() {
        let mut api = AnimationApi::new();
        let rig = build(&mut api);
        assert_eq!(api.bone_count(rig.skeleton).unwrap(), 18);
        assert_eq!(rig.segments.len(), 17);
        assert_eq!(rig.limit_specs.len(), 4);
        // The KickContact event is on the strike frame.
        assert_eq!(
            api.events_at(rig.clip, Tick::new(u64::from(KICK_STRIKE_FRAME))).unwrap(),
            vec![KICK_CONTACT]
        );
        assert_eq!(
            api.phase_at(rig.clip, Tick::new(33)).unwrap().and_then(KickPhase::from_code),
            Some(KickPhase::Strike)
        );
    }
}
