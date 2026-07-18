//! The distance-driven gait state machine and planted-foot targeting — the
//! persistent per-player heart of the locomotion animator. It advances the
//! gait cycle from ACTUAL resolved planar displacement (never intended
//! velocity), latches each foot's world-space ground contact at foot-strike and
//! holds it while the body travels over it, and classifies the locomotion mode.
//! It never reads or writes any authoritative simulation state.

use axiom::prelude::Vec3;

use crate::data::LocomotionTuning;

use super::foot::{self, Foot};

/// The locomotion mode, chosen explicitly rather than from a raw speed cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocomotionMode {
    Idle,
    Starting,
    Jogging,
    Sprinting,
    Stopping,
    Turning,
}

/// Why normal locomotion is suspended for a player this tick (the single
/// animation-priority boundary's reason code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideReason {
    /// Not overridden — normal locomotion owns the lower body.
    None,
    /// An action pose (throw / catch / block / tackle) owns the body.
    Action,
    /// Airborne / falling / diving.
    Airborne,
    /// Grounded impact or recovery.
    Down,
}

/// Which foot is currently the primary planted (weight-bearing) foot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlantedFoot {
    Left,
    Right,
}

/// The persistent gait state for one player.
#[derive(Debug, Clone, Copy)]
pub struct GaitState {
    pub phase: f32,
    pub prev_pos: Vec3,
    pub prev_vel: Vec3,
    pub traveled: f32,
    pub planted: PlantedFoot,
    pub left: Foot,
    pub right: Foot,
    pub stride_length: f32,
    pub cadence: f32,
    pub norm_speed: f32,
    pub startup: f32,
    pub stopping: f32,
    pub turn_intensity: f32,
    /// Signed lateral bank driver (+ = turning right), for torso bank.
    pub turn_bank: f32,
    /// Resolved planar acceleration this tick (for torso forward lean).
    pub accel: Vec3,
    pub mode: LocomotionMode,
    initialized: bool,
}

impl GaitState {
    pub fn new() -> Self {
        GaitState {
            phase: 0.0,
            prev_pos: Vec3::ZERO,
            prev_vel: Vec3::ZERO,
            traveled: 0.0,
            planted: PlantedFoot::Left,
            left: Foot::at(Vec3::ZERO),
            right: Foot::at(Vec3::ZERO),
            stride_length: 1.7,
            cadence: 0.0,
            norm_speed: 0.0,
            startup: 0.0,
            stopping: 0.0,
            turn_intensity: 0.0,
            turn_bank: 0.0,
            accel: Vec3::ZERO,
            mode: LocomotionMode::Idle,
            initialized: false,
        }
    }

    /// Re-anchor the feet under the body and drop all locks — called on a
    /// teleport, reset, or override so no stale plant survives the discontinuity.
    fn reanchor(&mut self, ground: Vec3, facing: f32, tuning: &LocomotionTuning) {
        let (right_dir, _) = foot::dirs(facing);
        let lat = right_dir.mul_scalar(tuning.stance_half_width);
        self.left = Foot::at(ground.subtract(lat));
        self.right = Foot::at(ground.add(lat));
        self.prev_pos = ground;
        self.prev_vel = Vec3::ZERO;
    }
}

impl Default for GaitState {
    fn default() -> Self {
        GaitState::new()
    }
}

/// The per-tick locomotion input for one player (the resolved, presentation-side
/// view of what the authoritative movement actually did this tick).
#[derive(Debug, Clone, Copy)]
pub struct LocomotionInput {
    pub pos: Vec3,
    pub vel: Vec3,
    pub facing: f32,
    pub speed: f32,
    pub grounded: bool,
    /// Whether normal distance-driven locomotion is allowed (false for action /
    /// fall / recovery / pre-snap-idle states, which pose their own body).
    pub allowed: bool,
    /// The override reason to report when `allowed` is false.
    pub reason: OverrideReason,
    /// A play reset / snap teleport fired this tick — invalidate the gait.
    pub teleported: bool,
}

fn planar_len(v: Vec3) -> f32 {
    Vec3::new(v.x, 0.0, v.z).length()
}

/// Advance the gait one tick and resolve both feet's ankle targets. Returns the
/// override reason in effect (`None` when normal locomotion ran).
pub fn advance(
    gait: &mut GaitState,
    input: LocomotionInput,
    tuning: &LocomotionTuning,
) -> OverrideReason {
    if !gait.initialized {
        gait.reanchor(input.pos, input.facing, tuning);
        gait.initialized = true;
    }

    // Suspend normal locomotion on any discontinuity or override: re-anchor the
    // feet, reset the ramps, and do not advance the distance-driven phase.
    let jump = planar_len(input.pos.subtract(gait.prev_pos));
    let discontinuity = input.teleported || jump > tuning.teleport_distance;
    if !input.allowed || !input.grounded || discontinuity {
        gait.reanchor(input.pos, input.facing, tuning);
        gait.startup = 0.0;
        gait.stopping = 0.0;
        gait.turn_intensity = 0.0;
        gait.turn_bank = 0.0;
        gait.accel = Vec3::ZERO;
        gait.norm_speed = 0.0;
        gait.cadence = 0.0;
        gait.mode = LocomotionMode::Idle;
        // A grounded, allowed teleport just re-anchors and keeps locomotion; a
        // suspended state reports why (an explicit reason, else airborne).
        return match (input.allowed, input.grounded) {
            (true, true) => OverrideReason::None,
            _ if input.reason != OverrideReason::None => input.reason,
            (_, false) => OverrideReason::Airborne,
            _ => OverrideReason::Action,
        };
    }

    let dt = crate::config::DT;
    let distance = jump;
    gait.traveled += distance;

    // Stride from speed, bounded, with cadence held under the ceiling.
    gait.norm_speed = (input.speed / tuning.sprint_speed).clamp(0.0, 1.0);
    let moving = input.speed >= tuning.min_gait_speed;

    // Startup / stopping ramps.
    let rate_up = 1.0 / tuning.startup_ticks.max(1.0);
    let rate_stop = 1.0 / tuning.stopping_ticks.max(1.0);
    gait.startup = if moving {
        (gait.startup + rate_up).min(1.0)
    } else {
        0.0
    };
    gait.stopping = if moving {
        0.0
    } else {
        (gait.stopping + rate_stop).min(1.0)
    };

    // Turn intensity + signed bank from the change in velocity direction, and
    // resolved planar acceleration for the torso lean.
    let turn_rate = turn_rate_of(gait.prev_vel, input.vel, dt);
    gait.turn_intensity = (turn_rate / tuning.turn_full_rate).clamp(0.0, 1.0) * f32::from(moving);
    let cross_y = gait.prev_vel.z * input.vel.x - gait.prev_vel.x * input.vel.z;
    gait.turn_bank = gait.turn_intensity * cross_y.signum() * f32::from(cross_y.abs() > 1.0e-4);
    gait.accel = Vec3::new(
        (input.vel.x - gait.prev_vel.x) / dt,
        0.0,
        (input.vel.z - gait.prev_vel.z) / dt,
    );

    let stride = effective_stride(gait, input.speed, tuning);
    gait.stride_length = stride;
    gait.cadence = (input.speed / stride).min(tuning.max_cadence);

    // Distance-driven phase advance — the anti-skate core. Blocked movement
    // (distance ≈ 0) barely advances; a stop settles the phase to a foot-down.
    if moving {
        gait.phase = (gait.phase + distance / stride).rem_euclid(1.0);
    } else {
        gait.phase = settle_phase(gait.phase, rate_stop);
    }

    gait.mode = classify(gait, moving);
    let left_planted = foot::resolve(
        gait.phase,
        &mut gait.left,
        &mut gait.right,
        input.pos,
        input.facing,
        stride,
        gait.turn_intensity,
        tuning,
    );
    gait.planted = if left_planted {
        PlantedFoot::Left
    } else {
        PlantedFoot::Right
    };

    gait.prev_pos = input.pos;
    gait.prev_vel = input.vel;
    OverrideReason::None
}

/// Effective full-cycle stride length, blended from speed, scaled by
/// startup/turn, bounded, and lengthened so cadence stays under the ceiling.
fn effective_stride(gait: &GaitState, speed: f32, tuning: &LocomotionTuning) -> f32 {
    let base = foot::lerp(tuning.jog_stride, tuning.sprint_stride, gait.norm_speed);
    let start_scale = foot::lerp(tuning.startup_stride_scale, 1.0, gait.startup);
    let turn_scale = foot::lerp(1.0, tuning.turning_stride_scale, gait.turn_intensity);
    let stop_scale = foot::lerp(1.0, tuning.stopping_stride_scale, gait.stopping);
    let mut stride = base * start_scale * turn_scale * stop_scale;
    // Never let cadence blur: raise the stride before it exceeds the ceiling.
    stride = stride.max(speed / tuning.max_cadence);
    stride.clamp(tuning.jog_stride * 0.35, tuning.sprint_stride * 1.15)
}

/// Ease the phase toward the nearest foot-planted position (0 or ½) so a stop
/// settles a foot down instead of freezing mid-swing.
fn settle_phase(phase: f32, rate: f32) -> f32 {
    let nearest = (phase * 2.0).round() / 2.0;
    let next = phase + (nearest - phase) * (rate * 3.0).min(1.0);
    next.rem_euclid(1.0)
}

/// Signed turn rate (rad/s) from the rotation between successive velocities.
fn turn_rate_of(prev: Vec3, now: Vec3, dt: f32) -> f32 {
    let a = Vec3::new(prev.x, 0.0, prev.z);
    let b = Vec3::new(now.x, 0.0, now.z);
    match (a.normalize(), b.normalize()) {
        (Ok(a), Ok(b)) => a.dot(b).clamp(-1.0, 1.0).acos() / dt.max(1.0e-4),
        _ => 0.0,
    }
}

/// Classify the locomotion mode from the ramps and speed.
fn classify(gait: &GaitState, moving: bool) -> LocomotionMode {
    if !moving {
        return if gait.stopping < 1.0 {
            LocomotionMode::Stopping
        } else {
            LocomotionMode::Idle
        };
    }
    if gait.turn_intensity > 0.5 {
        LocomotionMode::Turning
    } else if gait.startup < 1.0 {
        LocomotionMode::Starting
    } else if gait.norm_speed > 0.82 {
        LocomotionMode::Sprinting
    } else {
        LocomotionMode::Jogging
    }
}
