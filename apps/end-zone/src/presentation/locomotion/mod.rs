//! The app-local locomotion animator: the single owner of normal running,
//! jogging, starting, stopping, and turning poses. It consumes the immutable
//! presentation snapshot (authoritative movement, already resolved through
//! collision and boundary clamping) plus this tick's events, advances one
//! persistent [`GaitState`] per player from ACTUAL displacement, and produces a
//! fully composed pose per player. It cannot touch simulation state.
//!
//! Pose-composition order (one explicit boundary, keyed on the authoritative
//! `AnimState`):
//!
//! 1. base rig pose (neutral);
//! 2. locomotion pose (legs by IK + pelvis/torso/arms) for the holdable states,
//!    OR an action / fall / recovery override pose for every other state;
//! 3. football carry / quarterback throw-ready arm overlay.
//!
//! Presentation-only impact compression (squash) is applied later, at render.

pub mod foot;
pub mod gait;
pub mod leg;
pub mod pose;

pub use foot::FootPhase;
pub use gait::{GaitState, LocomotionInput, LocomotionMode, OverrideReason, PlantedFoot};

use axiom::prelude::Vec3;

use crate::config::PLAYER_COUNT;
use crate::data::LocomotionTuning;
use crate::events::{SimEvent, StampedEvent};
use crate::player::animation::{self, BallHold, JointPose};
use crate::player::AnimState;
use crate::presentation::snapshot::PresentationSnapshot;

/// How far past the line of scrimmage a quarterback must get before the
/// throw-ready hold gives way to a scrambling cradle (yards).
const POCKET_MARGIN: f32 = 1.0;

/// One player's fully composed pose plus its locomotion diagnostics.
#[derive(Debug, Clone, Copy)]
pub struct PlayerPose {
    pub pose: JointPose,
    pub hold: BallHold,
    pub sample: LocomotionSample,
}

/// The per-player locomotion read-out: everything the diagnostic overlay and
/// debug markers show, captured each tick.
#[derive(Debug, Clone, Copy)]
pub struct LocomotionSample {
    pub mode: LocomotionMode,
    pub gait_phase: f32,
    pub stride_length: f32,
    pub cadence: f32,
    pub norm_speed: f32,
    pub planted: PlantedFoot,
    pub left_phase: FootPhase,
    pub right_phase: FootPhase,
    pub left_target: Vec3,
    pub right_target: Vec3,
    pub left_ankle: Vec3,
    pub right_ankle: Vec3,
    pub left_lock_error: f32,
    pub right_lock_error: f32,
    pub next_landing: Vec3,
    pub planted_target: Vec3,
    pub overridden: bool,
    pub reason: OverrideReason,
    pub speed: f32,
    pub distance_moved: f32,
    pub move_vector: Vec3,
}

impl LocomotionSample {
    fn from_gait(
        g: &GaitState,
        ov: OverrideReason,
        speed: f32,
        distance: f32,
        move_vec: Vec3,
    ) -> Self {
        let planted_target = match g.planted {
            PlantedFoot::Left => g.left.lock,
            PlantedFoot::Right => g.right.lock,
        };
        let next_landing = match g.planted {
            PlantedFoot::Left => g.right.pending,
            PlantedFoot::Right => g.left.pending,
        };
        LocomotionSample {
            mode: g.mode,
            gait_phase: g.phase,
            stride_length: g.stride_length,
            cadence: g.cadence,
            norm_speed: g.norm_speed,
            planted: g.planted,
            left_phase: g.left.phase,
            right_phase: g.right.phase,
            left_target: g.left.target,
            right_target: g.right.target,
            left_ankle: g.left.ankle,
            right_ankle: g.right.ankle,
            left_lock_error: g.left.lock_error,
            right_lock_error: g.right.lock_error,
            next_landing,
            planted_target,
            overridden: ov != OverrideReason::None,
            reason: ov,
            speed,
            distance_moved: distance,
            move_vector: move_vec,
        }
    }
}

/// The persistent locomotion animator: one gait state per player slot.
#[derive(Debug)]
pub struct LocomotionAnimator {
    tuning: LocomotionTuning,
    bank: Vec<GaitState>,
    last: Vec<LocomotionSample>,
}

impl LocomotionAnimator {
    pub fn new(tuning: LocomotionTuning) -> Self {
        LocomotionAnimator {
            tuning,
            bank: vec![GaitState::new(); PLAYER_COUNT],
            last: Vec::new(),
        }
    }

    /// Advance every player's gait one tick and compose their poses, in id order.
    pub fn step(
        &mut self,
        snapshot: &PresentationSnapshot,
        events: &[StampedEvent],
    ) -> Vec<PlayerPose> {
        let teleport = events
            .iter()
            .any(|e| matches!(e.event, SimEvent::PlayReset | SimEvent::PlayStarted { .. }));
        let carrier = snapshot.ball.carrier();
        let mut out = Vec::with_capacity(snapshot.players.len());
        self.last.clear();
        for (index, view) in snapshot.players.iter().enumerate() {
            let gait = &mut self.bank[index];
            let allowed = view.anim.holds_ball();
            let grounded = view.pos.y.abs() < 0.05 && allowed;
            let prev = gait.prev_pos;
            let move_vec = Vec3::new(view.pos.x - prev.x, 0.0, view.pos.z - prev.z);
            let distance = move_vec.length();
            let input = LocomotionInput {
                pos: view.pos,
                vel: view.vel,
                facing: view.facing,
                speed: view.speed,
                grounded,
                allowed,
                reason: override_reason(view.anim),
                teleported: teleport,
            };
            let ov = gait::advance(gait, input, &self.tuning);

            let carrying = carrier == Some(view.id);
            let is_qb = view.id == snapshot.quarterback;
            let past_line =
                (view.pos.z - snapshot.line_of_scrimmage_z) * snapshot.drive_sign > POCKET_MARGIN;
            let hold = animation::ball_hold(carrying, is_qb, past_line, view.anim);

            let mut jp = if allowed {
                pose::locomotion_pose(gait, view.facing, view.pos, view.anim, &self.tuning)
            } else {
                animation::override_pose(view.anim, view.anim_ticks)
            };
            animation::apply_hold(&mut jp, hold);

            let sample = LocomotionSample::from_gait(gait, ov, view.speed, distance, move_vec);
            self.last.push(sample);
            out.push(PlayerPose {
                pose: jp,
                hold,
                sample,
            });
        }
        out
    }

    /// The last-resolved sample for a player slot (diagnostic overlay).
    pub fn sample(&self, index: usize) -> Option<&LocomotionSample> {
        self.last.get(index)
    }
}

/// Map a non-locomotion animation state to its override reason.
fn override_reason(anim: AnimState) -> OverrideReason {
    match anim {
        AnimState::ReadyStance
        | AnimState::Idle
        | AnimState::Jog
        | AnimState::Sprint
        | AnimState::DropBack => OverrideReason::None,
        AnimState::Throw
        | AnimState::Catch
        | AnimState::Block
        | AnimState::Tackle
        | AnimState::HitReaction
        | AnimState::Stumble => OverrideReason::Action,
        AnimState::Dive | AnimState::AirborneFall => OverrideReason::Airborne,
        AnimState::GroundImpact | AnimState::Recovery => OverrideReason::Down,
    }
}
