//! The whole-body carriage: how the runner's *body* rides the leg cycle.
//!
//! This is the pass that stops the hips being anchored to the player root. It
//! takes the already-advanced gait (phase, stance side, foot targets) and
//! resolves the weight transfer — the pelvis rising, falling, shifting toward
//! the stance leg and rotating over it, and the spine counter-rotating against
//! it. It is a **pure function**: same gait in, same carriage out, no state, no
//! simulation access, no wall clock.
//!
//! ### The three roots
//!
//! * **gameplay root** — the authoritative simulated position/facing. It
//!   arrives here as read-only input and is never written. Movement, collision,
//!   tackling and navigation continue to use it alone.
//! * **visual body root** — the cosmetic frame derived from the gameplay root:
//!   [`Carriage::root_lift`] / [`Carriage::root_lateral`] /
//!   [`Carriage::root_pitch`] / [`Carriage::root_roll`], applied by
//!   [`crate::player::rig::body_transform`]. Bounded, and one-way — nothing
//!   here can flow back into the sim.
//! * **pelvis** — a skeleton joint rotating *under* the visual body root
//!   ([`Carriage::pelvis_yaw`] / `pelvis_roll` / `pelvis_pitch`).
//!
//! Because the legs are IK-solved to world-locked foot targets, moving the
//! visual body root **is** the weight transfer: the stance leg automatically
//! compresses as the pelvis sinks and extends as it drives back up, while the
//! planted foot stays put. No separate compression pass is needed — the
//! existing solver does it, once the pelvis is actually allowed to move.
//!
//! ### Why not one sine wave
//!
//! Each component is shaped around its own biomechanical event, so they peak at
//! different moments of the stride:
//!
//! | component | shape | peak |
//! |---|---|---|
//! | vertical  | sink bell + push-off ramp + flight arc | sink ~30% of stance, rise at toe-off |
//! | lateral   | held toward the stance leg, smooth crossover | whole stance |
//! | yaw       | driven by *actual* swing-vs-stance foot separation | mid-swing |
//! | roll      | single-support bell (unsupported hip drops) | mid-stance |
//! | pitch     | speed + acceleration, not phase | steady at speed |

use crate::data::{BiomechTuning, LocomotionTuning};

use super::foot;
use super::gait::{GaitState, PlantedFoot};
use super::stride::{bell, foot_separation, smoothstep, stride_of, PI};

/// The resolved whole-body carriage for one tick. Angles in radians, offsets in
/// yards; every field is finite and bounded by the tuning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Carriage {
    // Visual body root (cosmetic frame under the gameplay root).
    pub root_lift: f32,
    pub root_lateral: f32,
    pub root_pitch: f32,
    pub root_roll: f32,
    // Pelvis joint.
    pub pelvis_yaw: f32,
    pub pelvis_roll: f32,
    pub pelvis_pitch: f32,
    // Lower spine (torso joint).
    pub spine_yaw: f32,
    pub spine_roll: f32,
    pub spine_pitch: f32,
    // Ribcage / shoulder girdle (pad joint) — shoulders and head hang here.
    pub ribcage_yaw: f32,
    pub ribcage_pitch: f32,
    // Head.
    pub head_pitch: f32,
    pub head_yaw: f32,
    // Diagnostics (debug view + tests).
    /// Which foot bears weight this half-cycle.
    pub stance: PlantedFoot,
    /// Progress through the stance, 0 = foot-strike, 1 = toe-off.
    pub stance_progress: f32,
    /// Whether the body is in the flight / transition part of the stride.
    pub in_flight: bool,
    /// How strongly the gait-driven components are engaged, 0..1.
    pub activity: f32,
}

impl Carriage {
    /// The neutral carriage: a standing body, no gait-driven offset.
    pub fn neutral() -> Self {
        Carriage {
            root_lift: 0.0,
            root_lateral: 0.0,
            root_pitch: 0.0,
            root_roll: 0.0,
            pelvis_yaw: 0.0,
            pelvis_roll: 0.0,
            pelvis_pitch: 0.0,
            spine_yaw: 0.0,
            spine_roll: 0.0,
            spine_pitch: 0.0,
            ribcage_yaw: 0.0,
            ribcage_pitch: 0.0,
            head_pitch: 0.0,
            head_yaw: 0.0,
            stance: PlantedFoot::Left,
            stance_progress: 0.0,
            in_flight: false,
            activity: 0.0,
        }
    }

    /// True when every resolved value is finite — the invariant the pose pass
    /// depends on and `tests/biomech.rs` pins.
    pub fn is_finite(&self) -> bool {
        [
            self.root_lift,
            self.root_lateral,
            self.root_pitch,
            self.root_roll,
            self.pelvis_yaw,
            self.pelvis_roll,
            self.pelvis_pitch,
            self.spine_yaw,
            self.spine_roll,
            self.spine_pitch,
            self.ribcage_yaw,
            self.ribcage_pitch,
            self.head_pitch,
            self.head_yaw,
            self.stance_progress,
            self.activity,
        ]
        .iter()
        .all(|v| v.is_finite())
    }
}

/// How the body is carried for a given stance, beyond the running default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carry {
    /// Normal forward running / jogging.
    Running,
    /// Backpedalling (quarterback drop-back): the lean reverses.
    Backpedal,
    /// A set pre-snap stance: upright, no sprint lean.
    Ready,
}

impl Carry {
    /// Which way the body leans (+1 forward, -1 back, 0 upright).
    fn lean_sign(self) -> f32 {
        match self {
            Carry::Running => 1.0,
            Carry::Backpedal => -1.0,
            Carry::Ready => 0.0,
        }
    }
}

/// Resolve the whole-body carriage for this tick.
///
/// `facing` is the gameplay root's yaw (read-only). `carry` selects the running
/// / backpedal / ready carriage. The returned offsets are the *targets* the
/// per-region springs then chase — nothing here is applied directly.
pub fn solve(
    gait: &GaitState,
    facing: f32,
    carry: Carry,
    loco: &LocomotionTuning,
    bio: &BiomechTuning,
) -> Carriage {
    let stride = stride_of(gait.phase, foot::planted_fraction(gait.stride_length, loco));
    let speed = gait.norm_speed.clamp(0.0, 1.0);
    // Gait-driven amplitude fades out at a standstill so a stopped player does
    // not keep performing a stride in place.
    let activity = smoothstep(speed / bio.full_carriage_speed.max(1.0e-3));
    let lean_sign = carry.lean_sign();

    // --- vertical: sink into the stance leg, drive back up through push-off --
    let sink = bell(stride.stance_progress, bio.weight_accept_center);
    let push_span = (1.0 - bio.push_off_start).max(1.0e-3);
    let push = smoothstep((stride.stance_progress - bio.push_off_start) / push_span);
    // Push-off extension fades out across flight so the next strike starts level.
    let push_fade = 1.0 - stride.flight_progress;
    let flight_arc = (stride.flight_progress * PI).sin();
    let root_lift = ((-bio.weight_accept_dip * sink)
        + (bio.push_off_rise * push * push_fade)
        + (bio.flight_rise * flight_arc))
        * activity;

    // --- lateral: hold weight over the stance leg, cross over smoothly -------
    let cross_start = (1.0 - bio.lateral_crossover).clamp(0.0, 0.95);
    let cross_span = (1.0 - cross_start).max(1.0e-3);
    let cross = smoothstep((stride.step_progress - cross_start) / cross_span);
    let root_lateral = bio.lateral_shift * stride.stance_sign * (1.0 - 2.0 * cross) * activity;

    // --- visual body root attitude (lean / bank) ----------------------------
    let forward_accel = {
        let (_, forward) = foot::dirs(facing);
        gait.accel.dot(forward)
    };
    let root_pitch = (lean_sign * bio.root_lean_speed * speed
        + forward_accel * loco.torso_lean_per_accel)
        .clamp(-0.18, loco.torso_lean_max);
    let root_roll = (loco.torso_bank * gait.turn_bank).clamp(-loco.torso_bank, loco.torso_bank);

    // --- pelvis rotation ----------------------------------------------------
    // Yaw follows the advancing leg, driven by where the feet actually are.
    // `stance_sign` carries the sign: the SWING-side hip rotates forward, and a
    // forward-rotating right hip is a negative yaw about +Y.
    let separation = foot_separation(gait, stride.stance, facing);
    let pelvis_yaw = (stride.stance_sign * separation * bio.pelvis_yaw_per_yard)
        .clamp(-bio.pelvis_yaw_max, bio.pelvis_yaw_max)
        * activity;
    // The unsupported hip drops, most deeply at mid-stance single support.
    let support = (stride.stance_progress * PI).sin() * (1.0 - stride.flight_progress);
    let pelvis_roll = stride.stance_sign * bio.pelvis_drop * support * activity;
    // Anterior tilt from speed and acceleration — not from the stride phase.
    let pelvis_pitch = (bio.pelvis_tilt_speed * speed
        + bio.pelvis_tilt_per_accel * forward_accel)
        .clamp(-bio.pelvis_tilt_max, bio.pelvis_tilt_max);

    // --- torso coupling -----------------------------------------------------
    // Forward carriage from the waist, distributed across the two spine joints
    // rather than stacked on one.
    let waist = loco.waist_lean * speed * lean_sign;
    let spine_yaw = -bio.spine_counter_yaw * pelvis_yaw;
    let ribcage_yaw = -bio.ribcage_counter_yaw * pelvis_yaw;
    let spine_roll = -bio.spine_roll_compensation * pelvis_roll;
    let spine_pitch = waist * bio.lean_spine_share;
    let ribcage_pitch = waist * (1.0 - bio.lean_spine_share);

    // --- head: follows the body, but damps most of the pelvis oscillation ---
    // The helmet hangs off the ribcage, so it inherits everything above it.
    let inherited_pitch = root_pitch + pelvis_pitch + spine_pitch + ribcage_pitch;
    let inherited_yaw = pelvis_yaw + spine_yaw + ribcage_yaw;
    let head_pitch = -bio.head_stabilization * inherited_pitch;
    let head_yaw = -bio.head_stabilization * inherited_yaw;

    Carriage {
        root_lift: root_lift.clamp(-bio.vertical_bound, bio.vertical_bound),
        root_lateral: root_lateral.clamp(-bio.lateral_bound, bio.lateral_bound),
        root_pitch,
        root_roll,
        pelvis_yaw,
        pelvis_roll,
        pelvis_pitch,
        spine_yaw,
        spine_roll,
        spine_pitch,
        ribcage_yaw,
        ribcage_pitch,
        head_pitch,
        head_yaw,
        stance: stride.stance,
        stance_progress: stride.stance_progress,
        in_flight: stride.in_flight,
        activity,
    }
}
