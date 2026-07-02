//! Pass 8 — deterministic shot result resolution.
//!
//! Turns the Pass 5 ball flight + Pass 7 animated goalie contact into one final
//! result — `Goal` / `Save` / `Miss` / `Post` — using fixed goal-mouth and
//! goal-frame (post/crossbar) tests. This is **not** a rules engine, a collision
//! framework, or a physics engine: it is a handful of fixed shapes and a strict
//! priority order, evaluated over explicit ordered arrays. No maps, no
//! wall-clock, no randomness, no probability.
//!
//! ## Priority (highest first)
//! 1. goalie contact → `Save`
//! 2. post / crossbar → `Post`
//! 3. inside the goal mouth → `Goal`
//! 4. otherwise → `Miss`
//!
//! Goalie contact is detected *during* flight (Pass 7), so it necessarily
//! precedes goal-plane arrival and wins over post/goal/miss automatically.

use axiom_math::Vec3;

use crate::soccer_penalty::penalty_goalie::{PenaltyGoalieContactFrame, PenaltyGoalieVolumeKind};
use crate::soccer_penalty::penalty_scene::{
    BALL_RADIUS, GOAL_HALF_WIDTH, GOAL_HEIGHT, GOAL_LINE_Z, GROUND_Y, POST_THICKNESS,
};

/// The four final result kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyShotResultKind {
    Goal,
    Save,
    Miss,
    Post,
}

/// The specific detail behind a result (how it was saved / where it hit / which
/// way it missed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyShotResultDetail {
    Scored,
    SavedByLeftHand,
    SavedByRightHand,
    SavedByTorso,
    SavedByBody,
    HitLeftPost,
    HitRightPost,
    HitCrossbar,
    MissedLeft,
    MissedRight,
    MissedHigh,
    MissedWideOrHigh,
}

/// A final resolved shot result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyShotResult {
    pub kind: PenaltyShotResultKind,
    pub detail: PenaltyShotResultDetail,
}

// --- goal mouth -------------------------------------------------------------

/// The fixed, deterministic goal-mouth descriptor. All constants match the
/// Stage 1 visual goal dimensions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalMouth {
    pub goal_plane_z: f32,
    pub left_post_x: f32,
    pub right_post_x: f32,
    pub ground_y: f32,
    pub crossbar_y: f32,
    pub post_thickness: f32,
    pub crossbar_thickness: f32,
}

impl PenaltyGoalMouth {
    /// The Stage 1 goal mouth.
    pub fn stage1() -> Self {
        Self {
            goal_plane_z: GOAL_LINE_Z,
            left_post_x: -GOAL_HALF_WIDTH,
            right_post_x: GOAL_HALF_WIDTH,
            ground_y: GROUND_Y,
            crossbar_y: GOAL_HEIGHT,
            post_thickness: POST_THICKNESS,
            crossbar_thickness: POST_THICKNESS,
        }
    }

    /// Whether a ball *center* is inside the legal goal mouth (between the posts
    /// and under the bar). Uses the ball center only, per Pass 8 scope.
    pub fn contains_center(&self, x: f32, y: f32) -> bool {
        x > self.left_post_x && x < self.right_post_x && y >= self.ground_y && y <= self.crossbar_y
    }
}

// --- goal-frame volumes -----------------------------------------------------

/// A post/crossbar frame volume kind, in stable priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PenaltyGoalFrameVolumeKind {
    LeftPost,
    RightPost,
    Crossbar,
}

/// A goal-frame collision volume: a narrow axis-aligned box on a post or the
/// crossbar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalFrameVolume {
    pub kind: PenaltyGoalFrameVolumeKind,
    pub ordinal: u32,
    pub center: Vec3,
    pub half_extents: Vec3,
}

impl PenaltyGoalFrameVolume {
    /// Whether the ball sphere overlaps this box (closest-point test).
    pub fn overlap(&self, ball_center: Vec3, ball_radius: f32) -> bool {
        let closest = Vec3::new(
            ball_center.x.clamp(self.center.x - self.half_extents.x, self.center.x + self.half_extents.x),
            ball_center.y.clamp(self.center.y - self.half_extents.y, self.center.y + self.half_extents.y),
            ball_center.z.clamp(self.center.z - self.half_extents.z, self.center.z + self.half_extents.z),
        );
        let d = ball_center.subtract(closest);
        d.dot(d) <= ball_radius * ball_radius
    }
}

/// The three goal-frame volumes in priority order (LeftPost, RightPost,
/// Crossbar).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalFrameVolumeSet {
    volumes: [PenaltyGoalFrameVolume; 3],
}

impl PenaltyGoalFrameVolumeSet {
    /// The Stage 1 posts + crossbar.
    pub fn stage1() -> Self {
        let mouth = PenaltyGoalMouth::stage1();
        let post_half = Vec3::new(POST_THICKNESS, GOAL_HEIGHT * 0.5, POST_THICKNESS);
        let bar_half = Vec3::new(GOAL_HALF_WIDTH + POST_THICKNESS, POST_THICKNESS, POST_THICKNESS);
        Self {
            volumes: [
                PenaltyGoalFrameVolume {
                    kind: PenaltyGoalFrameVolumeKind::LeftPost,
                    ordinal: 0,
                    center: Vec3::new(mouth.left_post_x, GOAL_HEIGHT * 0.5, mouth.goal_plane_z),
                    half_extents: post_half,
                },
                PenaltyGoalFrameVolume {
                    kind: PenaltyGoalFrameVolumeKind::RightPost,
                    ordinal: 1,
                    center: Vec3::new(mouth.right_post_x, GOAL_HEIGHT * 0.5, mouth.goal_plane_z),
                    half_extents: post_half,
                },
                PenaltyGoalFrameVolume {
                    kind: PenaltyGoalFrameVolumeKind::Crossbar,
                    ordinal: 2,
                    center: Vec3::new(0.0, GOAL_HEIGHT, mouth.goal_plane_z),
                    half_extents: bar_half,
                },
            ],
        }
    }

    /// The volumes in priority order.
    pub fn volumes(&self) -> &[PenaltyGoalFrameVolume] {
        &self.volumes
    }

    /// The first overlapping frame volume in priority order, if any.
    pub fn first_hit(&self, ball_center: Vec3, ball_radius: f32) -> Option<PenaltyGoalFrameVolumeKind> {
        self.volumes.iter().find(|v| v.overlap(ball_center, ball_radius)).map(|v| v.kind)
    }
}

// --- goal-plane crossing ----------------------------------------------------

/// A deterministic record of the ball reaching the goal plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyGoalPlaneCrossing {
    pub tick: u32,
    pub ball_position: Vec3,
    pub ball_radius: f32,
    pub target_x: i32,
    pub target_y: i32,
    pub inside_mouth: bool,
    pub frame_hit: Option<PenaltyGoalFrameVolumeKind>,
}

impl PenaltyGoalPlaneCrossing {
    /// Build the crossing at the goal plane from the ball position + target.
    pub fn at(tick: u32, ball_position: Vec3, target_x: i32, target_y: i32) -> Self {
        let mouth = PenaltyGoalMouth::stage1();
        let frames = PenaltyGoalFrameVolumeSet::stage1();
        Self {
            tick,
            ball_position,
            ball_radius: BALL_RADIUS,
            target_x,
            target_y,
            inside_mouth: mouth.contains_center(ball_position.x, ball_position.y),
            frame_hit: frames.first_hit(ball_position, BALL_RADIUS),
        }
    }
}

// --- resolver ---------------------------------------------------------------

/// Classifies a shot into a final [`PenaltyShotResult`] from either the goalie
/// contact frame (a save) or the goal-plane crossing (post / goal / miss).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyShotResultResolver;

impl PenaltyShotResultResolver {
    /// A goalie contact resolves to a `Save`, keyed by the contacted volume.
    pub fn from_contact(frame: &PenaltyGoalieContactFrame) -> PenaltyShotResult {
        let detail = frame
            .contact
            .map(|c| match c.volume_kind {
                PenaltyGoalieVolumeKind::LeftHand => PenaltyShotResultDetail::SavedByLeftHand,
                PenaltyGoalieVolumeKind::RightHand => PenaltyShotResultDetail::SavedByRightHand,
                PenaltyGoalieVolumeKind::Torso => PenaltyShotResultDetail::SavedByTorso,
                PenaltyGoalieVolumeKind::Body => PenaltyShotResultDetail::SavedByBody,
            })
            .unwrap_or(PenaltyShotResultDetail::SavedByBody);
        PenaltyShotResult { kind: PenaltyShotResultKind::Save, detail }
    }

    /// A goal-plane crossing resolves to `Post` (frame first), else `Goal`
    /// (inside the mouth), else `Miss`.
    pub fn from_crossing(crossing: &PenaltyGoalPlaneCrossing) -> PenaltyShotResult {
        match crossing.frame_hit {
            Some(kind) => PenaltyShotResult {
                kind: PenaltyShotResultKind::Post,
                detail: post_detail(kind),
            },
            None if crossing.inside_mouth => PenaltyShotResult {
                kind: PenaltyShotResultKind::Goal,
                detail: PenaltyShotResultDetail::Scored,
            },
            None => PenaltyShotResult {
                kind: PenaltyShotResultKind::Miss,
                detail: miss_detail(crossing.ball_position),
            },
        }
    }
}

fn post_detail(kind: PenaltyGoalFrameVolumeKind) -> PenaltyShotResultDetail {
    match kind {
        PenaltyGoalFrameVolumeKind::LeftPost => PenaltyShotResultDetail::HitLeftPost,
        PenaltyGoalFrameVolumeKind::RightPost => PenaltyShotResultDetail::HitRightPost,
        PenaltyGoalFrameVolumeKind::Crossbar => PenaltyShotResultDetail::HitCrossbar,
    }
}

fn miss_detail(pos: Vec3) -> PenaltyShotResultDetail {
    let mouth = PenaltyGoalMouth::stage1();
    if pos.x < mouth.left_post_x {
        PenaltyShotResultDetail::MissedLeft
    } else if pos.x > mouth.right_post_x {
        PenaltyShotResultDetail::MissedRight
    } else if pos.y > mouth.crossbar_y {
        PenaltyShotResultDetail::MissedHigh
    } else {
        PenaltyShotResultDetail::MissedWideOrHigh
    }
}

// --- resolved state + HUD descriptor ---------------------------------------

/// The frozen resolution of a shot: the result, the final ball position, and
/// the goal-plane crossing (for a post/goal/miss; `None` for a save).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyResolvedShotState {
    pub result: PenaltyShotResult,
    pub final_ball_position: Vec3,
    pub crossing: Option<PenaltyGoalPlaneCrossing>,
}

/// The HUD-facing view of a result: the big result word + an optional detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyResultHudDescriptor {
    pub result_text: &'static str,
    pub detail_text: Option<&'static str>,
}

impl PenaltyResultHudDescriptor {
    /// The HUD descriptor for a result (neutral, unlit text).
    pub fn from_result(result: PenaltyShotResult) -> Self {
        let result_text = match result.kind {
            PenaltyShotResultKind::Goal => "GOAL",
            PenaltyShotResultKind::Save => "SAVE",
            PenaltyShotResultKind::Post => "POST",
            PenaltyShotResultKind::Miss => "MISS",
        };
        let detail_text = match result.detail {
            PenaltyShotResultDetail::Scored => None,
            PenaltyShotResultDetail::SavedByLeftHand => Some("LEFT HAND"),
            PenaltyShotResultDetail::SavedByRightHand => Some("RIGHT HAND"),
            PenaltyShotResultDetail::SavedByTorso => Some("TORSO"),
            PenaltyShotResultDetail::SavedByBody => Some("BODY"),
            PenaltyShotResultDetail::HitLeftPost => Some("LEFT POST"),
            PenaltyShotResultDetail::HitRightPost => Some("RIGHT POST"),
            PenaltyShotResultDetail::HitCrossbar => Some("CROSSBAR"),
            PenaltyShotResultDetail::MissedLeft => Some("WIDE LEFT"),
            PenaltyShotResultDetail::MissedRight => Some("WIDE RIGHT"),
            PenaltyShotResultDetail::MissedHigh => Some("TOO HIGH"),
            PenaltyShotResultDetail::MissedWideOrHigh => Some("WIDE"),
        };
        Self { result_text, detail_text }
    }
}
