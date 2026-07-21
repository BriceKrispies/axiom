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

pub mod carriage;
pub mod foot;
pub mod gait;
pub mod leg;
pub mod pose;
pub mod spring;
pub mod stride;

pub use carriage::{Carriage, Carry};
pub use foot::FootPhase;
pub use gait::{GaitState, LocomotionInput, LocomotionMode, OverrideReason, PlantedFoot};
pub use spring::{BodySprings, Spring};

use axiom::prelude::Vec3;

use crate::config::PLAYER_COUNT;
use crate::data::{BiomechTuning, LocomotionTuning};
use crate::player::model::{PARTS, PELVIS};
use crate::player::rig;
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
    /// The authoritative gameplay root this tick (position the sim owns).
    pub gameplay_root: Vec3,
    /// The derived visual body root — gameplay root plus the bounded cosmetic
    /// weight-transfer offsets. Never feeds back into the sim.
    pub visual_root: Vec3,
    /// World position of the pelvis joint under the visual body root.
    pub pelvis: Vec3,
    /// The weight-shift point: the pelvis dropped to the turf, so the debug
    /// view shows directly whether weight is stacked over the stance foot.
    pub weight_point: Vec3,
    /// The resolved whole-body carriage targets for this tick.
    pub carriage: Carriage,
}

/// The body-frame read-out the sample records alongside the gait: where the
/// three roots ended up this tick, and the carriage that put them there.
#[derive(Debug, Clone, Copy)]
struct BodyFrame {
    gameplay_root: Vec3,
    visual_root: Vec3,
    pelvis: Vec3,
    carriage: Carriage,
}

impl LocomotionSample {
    fn from_gait(
        g: &GaitState,
        ov: OverrideReason,
        speed: f32,
        distance: f32,
        move_vec: Vec3,
        body: BodyFrame,
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
            gameplay_root: body.gameplay_root,
            visual_root: body.visual_root,
            pelvis: body.pelvis,
            weight_point: Vec3::new(body.pelvis.x, 0.02, body.pelvis.z),
            carriage: body.carriage,
        }
    }
}

/// The persistent locomotion animator: one gait state per player slot.
#[derive(Debug)]
pub struct LocomotionAnimator {
    tuning: LocomotionTuning,
    biomech: BiomechTuning,
    bank: Vec<GaitState>,
    /// One persistent virtual-muscle bank per player slot, alongside the gait.
    springs: Vec<BodySprings>,
    last: Vec<LocomotionSample>,
}

impl LocomotionAnimator {
    /// An animator with the default whole-body biomechanics.
    pub fn new(tuning: LocomotionTuning) -> Self {
        LocomotionAnimator::with_biomech(tuning, BiomechTuning::default())
    }

    /// An animator with explicit leg-cycle and whole-body biomechanics tuning.
    pub fn with_biomech(tuning: LocomotionTuning, biomech: BiomechTuning) -> Self {
        LocomotionAnimator {
            tuning,
            biomech,
            bank: vec![GaitState::new(); PLAYER_COUNT],
            springs: vec![BodySprings::new(); PLAYER_COUNT],
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
            let springs = &mut self.springs[index];
            // Any discontinuity that re-anchored the gait must also drop the
            // virtual muscle's momentum, so the body never springs across a
            // teleport, a play reset, or the hand-off to an override pose.
            let discontinuous = ov != OverrideReason::None || teleport;
            if discontinuous {
                springs.reset();
            }

            let carrying = carrier == Some(view.id);
            let is_qb = view.id == snapshot.quarterback;
            let past_line =
                (view.pos.z - snapshot.line_of_scrimmage_z) * snapshot.drive_sign > POCKET_MARGIN;
            let hold = animation::ball_hold(carrying, is_qb, past_line, view.anim);

            let (mut jp, carriage) = if allowed {
                pose::locomotion_pose(
                    gait,
                    springs,
                    view.facing,
                    view.pos,
                    view.anim,
                    &self.tuning,
                    &self.biomech,
                )
            } else {
                (
                    animation::override_pose(view.anim, view.anim_ticks),
                    Carriage::neutral(),
                )
            };
            animation::apply_hold(&mut jp, hold);

            // Read back the three roots for the biomechanical debug view. The
            // gameplay root is the sim's own position, untouched; the visual
            // body root is what the rig derived from it; the pelvis is the
            // joint riding under that.
            let body = rig::body_transform(view.pos, view.facing, &jp, 0.0);
            let frame = BodyFrame {
                gameplay_root: view.pos,
                visual_root: body.translation,
                pelvis: body.transform_point(PARTS[PELVIS].offset),
                carriage,
            };
            let sample =
                LocomotionSample::from_gait(gait, ov, view.speed, distance, move_vec, frame);
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
