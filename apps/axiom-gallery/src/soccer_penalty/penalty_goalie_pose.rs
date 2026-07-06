//! Pass 7 — the goalie as a deterministic articulated primitive puppet, with
//! authored dive pose clips and animated save volumes.
//!
//! The goalie is a fixed 16-part hierarchy of primitive boxes (no skeleton, no
//! skinning, no IK, no blend trees). Each part has a local [`Transform`] and a
//! world transform composed from its parent in stable ordinal order via
//! [`Transform::combine`]. Five authored dive clips move the parts; the Pass 6
//! save volumes ride along on the animated hands / torso / pelvis.
//!
//! Everything is pure and deterministic: fixed constants, explicit ordered
//! arrays, nearest-frame clip sampling, and no wall-clock time, randomness, or
//! maps.

use axiom_math::{Quat, Transform, Vec3};

use crate::soccer_penalty::penalty_goalie::PenaltyGoalieVolumeSet;
use crate::soccer_penalty::penalty_materials::PenaltyMaterialId;
use crate::soccer_penalty::penalty_scene::{GOALIE_X, GOALIE_Z};

/// The 16 goalie parts, in stable priority/ordinal order. `#[repr(u8)]` so the
/// discriminant is the ordinal; access is by enum, never by string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PenaltyGoaliePartKind {
    Root,
    Pelvis,
    Torso,
    Head,
    LeftUpperArm,
    LeftForearm,
    LeftHand,
    RightUpperArm,
    RightForearm,
    RightHand,
    LeftThigh,
    LeftShin,
    LeftFoot,
    RightThigh,
    RightShin,
    RightFoot,
}

impl PenaltyGoaliePartKind {
    /// Every part in ordinal order.
    pub const ALL: [PenaltyGoaliePartKind; 16] = [
        PenaltyGoaliePartKind::Root,
        PenaltyGoaliePartKind::Pelvis,
        PenaltyGoaliePartKind::Torso,
        PenaltyGoaliePartKind::Head,
        PenaltyGoaliePartKind::LeftUpperArm,
        PenaltyGoaliePartKind::LeftForearm,
        PenaltyGoaliePartKind::LeftHand,
        PenaltyGoaliePartKind::RightUpperArm,
        PenaltyGoaliePartKind::RightForearm,
        PenaltyGoaliePartKind::RightHand,
        PenaltyGoaliePartKind::LeftThigh,
        PenaltyGoaliePartKind::LeftShin,
        PenaltyGoaliePartKind::LeftFoot,
        PenaltyGoaliePartKind::RightThigh,
        PenaltyGoaliePartKind::RightShin,
        PenaltyGoaliePartKind::RightFoot,
    ];

    /// This part's stable ordinal (`0..16`).
    pub fn ordinal(self) -> u32 {
        self as u32
    }

    /// The parent part, or `None` for the root.
    pub fn parent(self) -> Option<PenaltyGoaliePartKind> {
        use PenaltyGoaliePartKind::*;
        match self {
            Root => None,
            Pelvis => Some(Root),
            Torso => Some(Pelvis),
            Head => Some(Torso),
            LeftUpperArm => Some(Torso),
            LeftForearm => Some(LeftUpperArm),
            LeftHand => Some(LeftForearm),
            RightUpperArm => Some(Torso),
            RightForearm => Some(RightUpperArm),
            RightHand => Some(RightForearm),
            LeftThigh => Some(Pelvis),
            LeftShin => Some(LeftThigh),
            LeftFoot => Some(LeftShin),
            RightThigh => Some(Pelvis),
            RightShin => Some(RightThigh),
            RightFoot => Some(RightShin),
        }
    }

    /// The stable, greppable render label for this part.
    pub fn label(self) -> &'static str {
        PART_LABELS[self as usize]
    }

    /// The part with this render label, if any (used to overlay a sampled pose
    /// onto the emitted goalie objects — never for runtime hot-path access).
    pub fn from_label(label: &str) -> Option<PenaltyGoaliePartKind> {
        PenaltyGoaliePartKind::ALL.iter().copied().find(|k| k.label() == label)
    }
}

/// The rest-pose local offsets (translation-only), goalie-local.
const IDLE_LOCAL: [Vec3; 16] = [
    Vec3::new(GOALIE_X, 0.0, GOALIE_Z), // Root
    Vec3::new(0.0, 0.92, 0.0),          // Pelvis
    Vec3::new(0.0, 0.40, 0.0),          // Torso
    Vec3::new(0.0, 0.52, 0.0),          // Head
    Vec3::new(-0.36, 0.28, 0.0),        // LeftUpperArm
    Vec3::new(-0.12, -0.28, 0.0),       // LeftForearm
    Vec3::new(-0.10, -0.28, 0.0),       // LeftHand
    Vec3::new(0.36, 0.28, 0.0),         // RightUpperArm
    Vec3::new(0.12, -0.28, 0.0),        // RightForearm
    Vec3::new(0.10, -0.28, 0.0),        // RightHand
    Vec3::new(-0.14, -0.10, 0.0),       // LeftThigh
    Vec3::new(0.0, -0.42, 0.0),         // LeftShin
    Vec3::new(0.0, -0.36, 0.06),        // LeftFoot
    Vec3::new(0.14, -0.10, 0.0),        // RightThigh
    Vec3::new(0.0, -0.42, 0.0),         // RightShin
    Vec3::new(0.0, -0.36, 0.06),        // RightFoot
];

/// Full box extents per part (for rendering). Root is invisible (zero size).
const PART_SIZE: [Vec3; 16] = [
    Vec3::new(0.0, 0.0, 0.0),    // Root (invisible)
    Vec3::new(0.34, 0.26, 0.26), // Pelvis
    Vec3::new(0.5, 0.6, 0.3),    // Torso
    Vec3::new(0.26, 0.28, 0.26), // Head
    Vec3::new(0.16, 0.34, 0.16), // LeftUpperArm
    Vec3::new(0.14, 0.3, 0.14),  // LeftForearm
    Vec3::new(0.18, 0.18, 0.18), // LeftHand
    Vec3::new(0.16, 0.34, 0.16), // RightUpperArm
    Vec3::new(0.14, 0.3, 0.14),  // RightForearm
    Vec3::new(0.18, 0.18, 0.18), // RightHand
    Vec3::new(0.2, 0.44, 0.2),   // LeftThigh
    Vec3::new(0.18, 0.4, 0.18),  // LeftShin
    Vec3::new(0.18, 0.12, 0.28), // LeftFoot
    Vec3::new(0.2, 0.44, 0.2),   // RightThigh
    Vec3::new(0.18, 0.4, 0.18),  // RightShin
    Vec3::new(0.18, 0.12, 0.28), // RightFoot
];

const PART_MATERIAL: [PenaltyMaterialId; 16] = [
    PenaltyMaterialId::GoalieShortsBlack,  // Root
    PenaltyMaterialId::GoalieShortsBlack,  // Pelvis
    PenaltyMaterialId::GoalieJerseyYellow, // Torso
    PenaltyMaterialId::GoalieSkin,         // Head
    PenaltyMaterialId::GoalieJerseyYellow, // LeftUpperArm
    PenaltyMaterialId::GoalieJerseyYellow, // LeftForearm
    PenaltyMaterialId::GoalieGloves,       // LeftHand
    PenaltyMaterialId::GoalieJerseyYellow, // RightUpperArm
    PenaltyMaterialId::GoalieJerseyYellow, // RightForearm
    PenaltyMaterialId::GoalieGloves,       // RightHand
    PenaltyMaterialId::GoalieShortsBlack,  // LeftThigh
    PenaltyMaterialId::GoalieSocks,        // LeftShin
    PenaltyMaterialId::GoalieShoes,        // LeftFoot
    PenaltyMaterialId::GoalieShortsBlack,  // RightThigh
    PenaltyMaterialId::GoalieSocks,        // RightShin
    PenaltyMaterialId::GoalieShoes,        // RightFoot
];

const PART_LABELS: [&str; 16] = [
    "goalie.root",
    "goalie.pelvis",
    "goalie.torso",
    "goalie.head",
    "goalie.upperarm.left",
    "goalie.forearm.left",
    "goalie.hand.left",
    "goalie.upperarm.right",
    "goalie.forearm.right",
    "goalie.hand.right",
    "goalie.thigh.left",
    "goalie.shin.left",
    "goalie.foot.left",
    "goalie.thigh.right",
    "goalie.shin.right",
    "goalie.foot.right",
];

/// One resolved goalie part: its kind, ordinal, parent, local + world
/// transforms, and its visual box + material descriptor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoaliePart {
    pub kind: PenaltyGoaliePartKind,
    pub ordinal: u32,
    pub parent_ordinal: Option<u32>,
    pub local: Transform,
    pub world: Transform,
    pub size: Vec3,
    pub material: PenaltyMaterialId,
}

/// A pose: the local transform of every part (authoring data).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoaliePose {
    pub local: [Transform; 16],
}

impl PenaltyGoaliePose {
    /// The idle / ready rest pose (translation-only locals). The idle pose is left
    /// un-rotated on purpose: the Pass-6 save volumes ride these part world
    /// positions, so re-posing the idle would move the deterministic save geometry
    /// — a gameplay change out of scope for a visual-silhouette pass. The goalie's
    /// silhouette still improves via angular box meshes + sock/shoe materials +
    /// hair, none of which move a joint. A render-only posed overlay (decoupled
    /// from the collision rig) is the correct future path to an arms-out stance.
    pub fn idle() -> Self {
        let mut local = [Transform::IDENTITY; 16];
        (0..16).for_each(|i| local[i] = Transform::from_translation(IDLE_LOCAL[i]));
        Self { local }
    }

    /// A **render-only** braced ready stance: the keeper "makes himself big" —
    /// arms spread WIDE and near-horizontal (the gloves flung out to the sides,
    /// forearms continuing the outward line rather than curling up into a Y) and
    /// legs planted in a wide braced crouch, matching the reference's large set
    /// keeper. This is deliberately decoupled from [`Self::idle`] — the Pass-6
    /// save volumes keep riding the un-rotated `idle` rig, so this pose changes
    /// only the visual silhouette, never the deterministic save geometry (the
    /// dive clips are separate). Used by the static diorama emit.
    pub fn idle_display() -> Self {
        let mut local = [Transform::IDENTITY; 16];
        (0..16).for_each(|i| local[i] = Transform::from_translation(IDLE_LOCAL[i]));
        // (part-ordinal, euler x, y, z) — upper arms flung wide & near-horizontal,
        // but the elbows BREAK so the gloves drop down-and-forward into a catching
        // set (the reference keeper's hands sit ~waist height, forward of the body,
        // not a stiff scarecrow-T). Legs planted in a wide brace.
        [
            (4_usize, 0.1_f32, 0.0_f32, -1.45_f32), // left upper arm — wide, near-horizontal
            (7, 0.1, 0.0, 1.45),                    // right upper arm — wide, near-horizontal
            (5, 0.35, 0.0, 0.72),                   // left forearm — elbow breaks: glove drops down-&-forward
            (8, 0.35, 0.0, -0.72),                  // right forearm — elbow breaks: glove drops down-&-forward
            (10, 0.18, 0.0, -0.55),                 // left thigh — WIDE plant + weight forward
            (13, 0.18, 0.0, 0.55),                  // right thigh
            (11, -0.62, 0.0, 0.0),                  // left shin — deep braced knee bend (low set crouch)
            (14, -0.62, 0.0, 0.0),                  // right shin
        ]
        .iter()
        .for_each(|&(i, x, y, z)| {
            local[i] = Transform::new(
                IDLE_LOCAL[i],
                Quat::from_euler_xyz(x, y, z),
                Vec3::new(1.0, 1.0, 1.0),
            );
        });
        Self { local }
    }

    /// Resolve world transforms by composing each part onto its parent in
    /// ordinal order (parents always precede children).
    pub fn resolve(&self) -> PenaltyGoaliePoseDescriptor {
        let mut world = [Transform::IDENTITY; 16];
        PenaltyGoaliePartKind::ALL.iter().for_each(|&kind| {
            let i = kind.ordinal() as usize;
            world[i] = match kind.parent() {
                Some(parent) => Transform::combine(world[parent.ordinal() as usize], self.local[i]),
                None => self.local[i],
            };
        });
        PenaltyGoaliePoseDescriptor { local: self.local, world }
    }
}

/// A resolved pose: local + world transforms, and per-part descriptors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoaliePoseDescriptor {
    local: [Transform; 16],
    world: [Transform; 16],
}

impl PenaltyGoaliePoseDescriptor {
    /// The world position of a part (its world-transform translation).
    pub fn world_position(&self, kind: PenaltyGoaliePartKind) -> Vec3 {
        self.world[kind.ordinal() as usize].translation
    }

    /// The world transform of a part.
    pub fn world_transform(&self, kind: PenaltyGoaliePartKind) -> Transform {
        self.world[kind.ordinal() as usize]
    }

    /// Every resolved part, in ordinal order.
    pub fn parts(&self) -> [PenaltyGoaliePart; 16] {
        let mut parts = [self.part(PenaltyGoaliePartKind::Root); 16];
        PenaltyGoaliePartKind::ALL.iter().for_each(|&kind| {
            parts[kind.ordinal() as usize] = self.part(kind);
        });
        parts
    }

    /// One resolved part.
    pub fn part(&self, kind: PenaltyGoaliePartKind) -> PenaltyGoaliePart {
        let i = kind.ordinal() as usize;
        PenaltyGoaliePart {
            kind,
            ordinal: kind.ordinal(),
            parent_ordinal: kind.parent().map(|p| p.ordinal()),
            local: self.local[i],
            world: self.world[i],
            size: PART_SIZE[i],
            material: PART_MATERIAL[i],
        }
    }
}

/// One authored keyframe of a clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoaliePoseFrame {
    pub tick: u32,
    pub pose: PenaltyGoaliePose,
    pub label: &'static str,
}

/// The five deterministic dive lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyGoalieDiveLane {
    DiveLeftLow,
    DiveLeftHigh,
    DiveRightLow,
    DiveRightHigh,
    DiveCenter,
}

impl PenaltyGoalieDiveLane {
    pub const ALL: [PenaltyGoalieDiveLane; 5] = [
        PenaltyGoalieDiveLane::DiveLeftLow,
        PenaltyGoalieDiveLane::DiveLeftHigh,
        PenaltyGoalieDiveLane::DiveRightLow,
        PenaltyGoalieDiveLane::DiveRightHigh,
        PenaltyGoalieDiveLane::DiveCenter,
    ];

    /// Deterministic dive-lane selection from a normalized shot target. No
    /// randomness, no prediction, no difficulty — a fixed table.
    pub fn select(target_x: i32, target_y: i32) -> Self {
        match (target_x, target_y) {
            (x, y) if x < -35 && y < 50 => PenaltyGoalieDiveLane::DiveLeftLow,
            (x, _) if x < -35 => PenaltyGoalieDiveLane::DiveLeftHigh,
            (x, y) if x > 35 && y < 50 => PenaltyGoalieDiveLane::DiveRightLow,
            (x, _) if x > 35 => PenaltyGoalieDiveLane::DiveRightHigh,
            _ => PenaltyGoalieDiveLane::DiveCenter,
        }
    }

    /// `(side, vert)`: `side` = -1 left / +1 right / 0 center; `vert` = +1 high
    /// / -1 low / 0 center.
    fn params(self) -> (f32, f32) {
        match self {
            PenaltyGoalieDiveLane::DiveLeftLow => (-1.0, -1.0),
            PenaltyGoalieDiveLane::DiveLeftHigh => (-1.0, 1.0),
            PenaltyGoalieDiveLane::DiveRightLow => (1.0, -1.0),
            PenaltyGoalieDiveLane::DiveRightHigh => (1.0, 1.0),
            PenaltyGoalieDiveLane::DiveCenter => (0.0, 0.0),
        }
    }
}

/// The clip's fixed duration in ticks.
pub const CLIP_DURATION_TICKS: u32 = 24;

/// An authored dive pose clip: a lane, a duration, and ordered keyframes.
#[derive(Debug, Clone, PartialEq)]
pub struct PenaltyGoaliePoseClip {
    pub lane: PenaltyGoalieDiveLane,
    pub duration_ticks: u32,
    pub frames: Vec<PenaltyGoaliePoseFrame>,
}

impl PenaltyGoaliePoseClip {
    /// Build the authored clip for a lane: five keyframes (idle, anticipation,
    /// launch, full extension, settle) built from the idle pose plus fixed
    /// per-part offsets.
    pub fn for_lane(lane: PenaltyGoalieDiveLane) -> Self {
        let frames = vec![
            frame(lane, 0, 0.0, 0.0, "idle"),
            frame(lane, 4, -0.10, -0.20, "anticipation"),
            frame(lane, 9, 0.45, -0.05, "launch"),
            frame(lane, 16, 1.0, 0.0, "extension"),
            frame(lane, CLIP_DURATION_TICKS, 0.82, -0.10, "settle"),
        ];
        Self { lane, duration_ticks: CLIP_DURATION_TICKS, frames }
    }
}

fn vert_root_y(vert: f32) -> f32 {
    // High dives drop the body a little; low dives drop it a lot; center barely.
    if vert > 0.0 {
        -0.05
    } else if vert < 0.0 {
        -0.55
    } else {
        -0.10
    }
}

fn vert_hand_y(vert: f32) -> f32 {
    // High/center dives raise the hands; low dives lower them.
    if vert < 0.0 {
        -0.20
    } else if vert > 0.0 {
        0.80
    } else {
        0.75
    }
}

/// Build one keyframe: idle + `m`-scaled dive offsets + a pelvis `crouch`.
fn frame(
    lane: PenaltyGoalieDiveLane,
    tick: u32,
    m: f32,
    crouch: f32,
    label: &'static str,
) -> PenaltyGoaliePoseFrame {
    let (side, vert) = lane.params();
    let mut pose = PenaltyGoaliePose::idle();

    let add = |pose: &mut PenaltyGoaliePose, kind: PenaltyGoaliePartKind, off: Vec3| {
        let i = kind.ordinal() as usize;
        pose.local[i].translation = pose.local[i].translation.add(off);
    };

    // Root shifts sideways (the dive) and settles vertically.
    add(&mut pose, PenaltyGoaliePartKind::Root, Vec3::new(side * 0.8 * m, vert_root_y(vert) * m, 0.0));
    // Anticipation / settle crouch at the pelvis.
    add(&mut pose, PenaltyGoaliePartKind::Pelvis, Vec3::new(0.0, crouch, 0.0));

    // The lead hand(s) + upper arm(s) reach toward the lane.
    let hand_off = Vec3::new(side * 0.5 * m, vert_hand_y(vert) * m, 0.0);
    let arm_off = Vec3::new(side * 0.15 * m, vert_hand_y(vert) * 0.3 * m, 0.0);
    let leads: &[(PenaltyGoaliePartKind, PenaltyGoaliePartKind)] = if side > 0.0 {
        &[(PenaltyGoaliePartKind::RightHand, PenaltyGoaliePartKind::RightUpperArm)]
    } else if side < 0.0 {
        &[(PenaltyGoaliePartKind::LeftHand, PenaltyGoaliePartKind::LeftUpperArm)]
    } else {
        &[
            (PenaltyGoaliePartKind::LeftHand, PenaltyGoaliePartKind::LeftUpperArm),
            (PenaltyGoaliePartKind::RightHand, PenaltyGoaliePartKind::RightUpperArm),
        ]
    };
    leads.iter().for_each(|&(hand, arm)| {
        add(&mut pose, hand, hand_off);
        add(&mut pose, arm, arm_off);
    });

    PenaltyGoaliePoseFrame { tick, pose, label }
}

/// Deterministic nearest-(previous-)frame clip sampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyGoaliePoseSampler;

impl PenaltyGoaliePoseSampler {
    /// Sample a clip at a local tick: clamped before the first frame to the
    /// first pose and after the duration to the final pose. Holds the last
    /// keyframe whose tick `<=` the (clamped) sample tick. Always the same pose
    /// for the same `(clip, tick)`.
    pub fn sample(clip: &PenaltyGoaliePoseClip, tick: u32) -> PenaltyGoaliePose {
        let t = tick.min(clip.duration_ticks);
        clip.frames
            .iter().rfind(|f| f.tick <= t)
            .map(|f| f.pose)
            .unwrap_or_else(PenaltyGoaliePose::idle)
    }
}

/// The library of the five authored dive clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyGoalieClipLibrary;

impl PenaltyGoalieClipLibrary {
    /// The clip for a lane.
    pub fn clip(lane: PenaltyGoalieDiveLane) -> PenaltyGoaliePoseClip {
        PenaltyGoaliePoseClip::for_lane(lane)
    }
}

/// The goalie's deterministic animation sub-state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyGoalieAnimationState {
    Idle,
    TrackingShot,
    Diving,
    Landed,
}

/// The goalie animation carried on the interaction state: current phase, the
/// chosen dive lane (once locked), and the local clip tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyGoalieAnimation {
    pub state: PenaltyGoalieAnimationState,
    pub lane: Option<PenaltyGoalieDiveLane>,
    pub clip_tick: u32,
}

impl PenaltyGoalieAnimation {
    /// The idle animation: no lane, tick 0.
    pub const fn idle() -> Self {
        Self { state: PenaltyGoalieAnimationState::Idle, lane: None, clip_tick: 0 }
    }

    /// Choose a dive lane from the locked target and start tracking (tick 0).
    pub fn locked(target_x: i32, target_y: i32) -> Self {
        Self {
            state: PenaltyGoalieAnimationState::TrackingShot,
            lane: Some(PenaltyGoalieDiveLane::select(target_x, target_y)),
            clip_tick: 0,
        }
    }

    /// Advance the dive one tick (only meaningful once a lane is chosen).
    pub fn advanced(self) -> Self {
        let tick = (self.clip_tick + 1).min(CLIP_DURATION_TICKS);
        let state = self
            .lane
            .map(|_| {
                if tick >= CLIP_DURATION_TICKS { PenaltyGoalieAnimationState::Landed } else { PenaltyGoalieAnimationState::Diving }
            })
            .unwrap_or(PenaltyGoalieAnimationState::Idle);
        Self { state, lane: self.lane, clip_tick: tick }
    }

    /// The current authored pose (idle when no lane is chosen).
    pub fn current_pose(&self) -> PenaltyGoaliePose {
        self.lane
            .map(|lane| PenaltyGoaliePoseSampler::sample(&PenaltyGoalieClipLibrary::clip(lane), self.clip_tick))
            .unwrap_or_else(PenaltyGoaliePose::idle)
    }

    /// The resolved (world) pose descriptor for volumes / contact geometry.
    /// This rides the un-rotated `idle` rig at rest so the deterministic save
    /// volumes never move (see [`PenaltyGoaliePose::idle_display`]).
    pub fn descriptor(&self) -> PenaltyGoaliePoseDescriptor {
        self.current_pose().resolve()
    }

    /// The **render-only** pose descriptor. At rest the keeper stands in the
    /// braced `idle_display` ready stance — arms flung wide and near-horizontal
    /// with the elbows breaking so the gloves drop into a catching set —
    /// matching the reference's large "set" keeper, instead of the stiff
    /// scarecrow-T the translation-only `idle` rig renders as. During a dive it
    /// follows the authored clip exactly, identical to [`Self::descriptor`].
    /// Deliberately decoupled from [`Self::descriptor`] / [`Self::animated_volumes`]:
    /// the deterministic save geometry keeps riding the un-rotated `idle` rig,
    /// so this changes only the visible silhouette, never gameplay.
    pub fn render_descriptor(&self) -> PenaltyGoaliePoseDescriptor {
        self.lane
            .map(|_| self.current_pose())
            .unwrap_or_else(PenaltyGoaliePose::idle_display)
            .resolve()
    }

    /// The animated save-volume set for the current pose.
    pub fn animated_volumes(&self) -> PenaltyGoalieVolumeSet {
        PenaltyGoalieAnimatedVolumeSet::from_descriptor(&self.descriptor()).set
    }
}

impl Default for PenaltyGoalieAnimation {
    fn default() -> Self {
        Self::idle()
    }
}

/// The Pass 6 save volumes attached to the animated goalie parts: hands →
/// hand parts, torso → torso part, body → pelvis part. Priority order is
/// preserved by [`PenaltyGoalieVolumeSet::at_centers`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalieAnimatedVolumeSet {
    pub set: PenaltyGoalieVolumeSet,
}

impl PenaltyGoalieAnimatedVolumeSet {
    /// Attach the volume set to a resolved pose.
    pub fn from_descriptor(desc: &PenaltyGoaliePoseDescriptor) -> Self {
        Self {
            set: PenaltyGoalieVolumeSet::at_centers(
                desc.world_position(PenaltyGoaliePartKind::LeftHand),
                desc.world_position(PenaltyGoaliePartKind::RightHand),
                desc.world_position(PenaltyGoaliePartKind::Torso),
                desc.world_position(PenaltyGoaliePartKind::Pelvis),
            ),
        }
    }
}
