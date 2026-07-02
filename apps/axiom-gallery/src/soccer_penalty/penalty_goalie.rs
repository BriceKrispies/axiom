//! Pass 6 — deterministic goalie save volumes + contact detection.
//!
//! The static goalie gets a small, fixed set of invisible collision volumes
//! (two hand spheres, a torso box, a broad body box). The Pass 5 ball sphere is
//! tested against them each flight tick to produce a deterministic
//! [`PenaltyGoalieContactFrame`]. **This decides no shot outcome** — it only
//! reports *neutral* contact facts (`Hand` / `Torso` / `Body` / `None`) that a
//! later pass will turn into a save/goal/miss/post result.
//!
//! This is **not** a physics engine, **not** a collision system, and **not** a
//! character controller: it is four fixed shapes and one sphere-overlap test,
//! evaluated over an explicit ordered array in strict priority order. No maps,
//! no wall-clock time, no randomness.

use axiom_math::Vec3;

use crate::soccer_penalty::penalty_scene::{BALL_RADIUS, GOALIE_X, GOALIE_Z};

/// Which goalie volume a contact hit. Declaration order **is** the priority
/// order (`derive(Ord)`): `LeftHand` > `RightHand` > `Torso` > `Body`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PenaltyGoalieVolumeKind {
    LeftHand,
    RightHand,
    Torso,
    Body,
}

/// The neutral, **pre-result** contact label. Deliberately not `Save` / `Goal`
/// / `Miss` / `Post` / `Score` — resolution is a later pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyGoalieContactKind {
    None,
    Hand,
    Torso,
    Body,
}

impl PenaltyGoalieVolumeKind {
    /// The neutral contact label this volume produces.
    pub fn contact_kind(self) -> PenaltyGoalieContactKind {
        match self {
            PenaltyGoalieVolumeKind::LeftHand | PenaltyGoalieVolumeKind::RightHand => {
                PenaltyGoalieContactKind::Hand
            }
            PenaltyGoalieVolumeKind::Torso => PenaltyGoalieContactKind::Torso,
            PenaltyGoalieVolumeKind::Body => PenaltyGoalieContactKind::Body,
        }
    }
}

/// An app-local volume shape. A sphere (hands) or an axis-aligned box
/// (torso/body). No capsule needed for Pass 6.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PenaltyGoalieVolumeShape {
    Sphere { radius: f32 },
    Aabb { half_extents: Vec3 },
}

/// One goalie collision volume: its kind, its stable ordinal (priority index),
/// its world center, and its shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalieVolume {
    pub kind: PenaltyGoalieVolumeKind,
    pub ordinal: u32,
    pub center: Vec3,
    pub shape: PenaltyGoalieVolumeShape,
}

impl PenaltyGoalieVolume {
    /// If the ball sphere overlaps this volume, the approximate contact point;
    /// otherwise `None`. Pure closest-point math.
    pub fn overlap(&self, ball_center: Vec3, ball_radius: f32) -> Option<Vec3> {
        match self.shape {
            PenaltyGoalieVolumeShape::Sphere { radius } => {
                let diff = ball_center.subtract(self.center);
                let dist = diff.length();
                (dist <= radius + ball_radius).then(|| {
                    let dir = if dist > 1.0e-6 { diff.mul_scalar(1.0 / dist) } else { Vec3::UNIT_Y };
                    self.center.add(dir.mul_scalar(radius))
                })
            }
            PenaltyGoalieVolumeShape::Aabb { half_extents } => {
                let closest = Vec3::new(
                    ball_center.x.clamp(self.center.x - half_extents.x, self.center.x + half_extents.x),
                    ball_center.y.clamp(self.center.y - half_extents.y, self.center.y + half_extents.y),
                    ball_center.z.clamp(self.center.z - half_extents.z, self.center.z + half_extents.z),
                );
                let d = ball_center.subtract(closest);
                (d.dot(d) <= ball_radius * ball_radius).then_some(closest)
            }
        }
    }
}

// --- fixed volume geometry (mirrors the Stage 1 goalie puppet) --------------

pub const HAND_X_OFFSET: f32 = 0.58;
pub const HAND_Y: f32 = 0.94;
pub const HAND_RADIUS: f32 = 0.22;
pub const TORSO_Y: f32 = 1.28;
pub const TORSO_HALF: Vec3 = Vec3::new(0.34, 0.42, 0.22);
pub const BODY_Y: f32 = 1.0;
pub const BODY_HALF: Vec3 = Vec3::new(0.72, 1.02, 0.2);

/// The static goalie volume set, in strict priority order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalieVolumeSet {
    volumes: [PenaltyGoalieVolume; 4],
}

impl PenaltyGoalieVolumeSet {
    /// Build a volume set from four world centers, keeping the fixed shapes,
    /// radii, kinds, and priority ordinals. This is the single constructor both
    /// the static Pass 6 set and the Pass 7 animated set go through.
    pub fn at_centers(left_hand: Vec3, right_hand: Vec3, torso: Vec3, body: Vec3) -> Self {
        Self {
            volumes: [
                PenaltyGoalieVolume {
                    kind: PenaltyGoalieVolumeKind::LeftHand,
                    ordinal: 0,
                    center: left_hand,
                    shape: PenaltyGoalieVolumeShape::Sphere { radius: HAND_RADIUS },
                },
                PenaltyGoalieVolume {
                    kind: PenaltyGoalieVolumeKind::RightHand,
                    ordinal: 1,
                    center: right_hand,
                    shape: PenaltyGoalieVolumeShape::Sphere { radius: HAND_RADIUS },
                },
                PenaltyGoalieVolume {
                    kind: PenaltyGoalieVolumeKind::Torso,
                    ordinal: 2,
                    center: torso,
                    shape: PenaltyGoalieVolumeShape::Aabb { half_extents: TORSO_HALF },
                },
                PenaltyGoalieVolume {
                    kind: PenaltyGoalieVolumeKind::Body,
                    ordinal: 3,
                    center: body,
                    shape: PenaltyGoalieVolumeShape::Aabb { half_extents: BODY_HALF },
                },
            ],
        }
    }

    /// The fixed volume set for the Stage 1 goalie's rest pose.
    pub fn stage1() -> Self {
        let z = GOALIE_Z;
        Self::at_centers(
            Vec3::new(GOALIE_X - HAND_X_OFFSET, HAND_Y, z),
            Vec3::new(GOALIE_X + HAND_X_OFFSET, HAND_Y, z),
            Vec3::new(GOALIE_X, TORSO_Y, z),
            Vec3::new(GOALIE_X, BODY_Y, z),
        )
    }

    /// The volumes in priority order.
    pub fn volumes(&self) -> &[PenaltyGoalieVolume] {
        &self.volumes
    }
}

/// A recorded contact against a goalie volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalieContact {
    pub contact_kind: PenaltyGoalieContactKind,
    pub volume_kind: PenaltyGoalieVolumeKind,
    pub volume_ordinal: u32,
    pub contact_point: Vec3,
}

/// The deterministic per-tick contact record. `contact` is `None` when the ball
/// touched nothing this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalieContactFrame {
    pub tick: u32,
    pub ball_position: Vec3,
    pub ball_radius: f32,
    pub contact: Option<PenaltyGoalieContact>,
}

impl PenaltyGoalieContactFrame {
    /// The neutral contact kind for this frame (`None` if no contact).
    pub fn contact_kind(&self) -> PenaltyGoalieContactKind {
        self.contact.map(|c| c.contact_kind).unwrap_or(PenaltyGoalieContactKind::None)
    }
}

/// Tests a ball sphere against the goalie volume set in priority order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalieContactDetector {
    set: PenaltyGoalieVolumeSet,
}

impl PenaltyGoalieContactDetector {
    /// A detector over an arbitrary volume set (e.g. an animated one).
    pub fn new(set: PenaltyGoalieVolumeSet) -> Self {
        Self { set }
    }

    /// The detector for the Stage 1 goalie's rest pose.
    pub fn stage1() -> Self {
        Self::new(PenaltyGoalieVolumeSet::stage1())
    }

    /// The volume set (for debug descriptors / tests).
    pub fn volume_set(&self) -> &PenaltyGoalieVolumeSet {
        &self.set
    }

    /// Detect contact for a ball sphere at `ball_center` on shot-local `tick`.
    /// The first overlapping volume in priority order wins (fixed ball radius).
    pub fn detect(&self, ball_center: Vec3, tick: u32) -> PenaltyGoalieContactFrame {
        let contact = self.set.volumes().iter().find_map(|v| {
            v.overlap(ball_center, BALL_RADIUS).map(|point| PenaltyGoalieContact {
                contact_kind: v.kind.contact_kind(),
                volume_kind: v.kind,
                volume_ordinal: v.ordinal,
                contact_point: point,
            })
        });
        PenaltyGoalieContactFrame { tick, ball_position: ball_center, ball_radius: BALL_RADIUS, contact }
    }
}

/// Debug visualization flag for the save volumes. **Off by default** and purely
/// cosmetic — it never influences contact detection or gameplay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyGoalieDebugDescriptor {
    pub enabled: bool,
}

impl PenaltyGoalieDebugDescriptor {
    pub const DISABLED: Self = Self { enabled: false };
    pub const ENABLED: Self = Self { enabled: true };

    /// The volumes to visualize: the whole set when enabled, empty otherwise.
    pub fn markers(&self, set: &PenaltyGoalieVolumeSet) -> Vec<PenaltyGoalieVolume> {
        if self.enabled { set.volumes().to_vec() } else { Default::default() }
    }
}

impl Default for PenaltyGoalieDebugDescriptor {
    fn default() -> Self {
        Self::DISABLED
    }
}
