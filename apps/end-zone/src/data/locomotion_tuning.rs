//! Locomotion tuning: the named stride / gait / foot-lock knobs the app-local
//! locomotion animator reads. Split out of [`crate::data::tuning`] so each
//! tuning file stays narrowly owned. Nothing about the leg cycle is a scattered
//! constant — every number lives here.

/// Locomotion tuning: the named stride/gait/foot-lock numbers the app-local
/// locomotion animator reads. Nothing about the leg cycle is a scattered
/// constant — every knob lives here. Units: yards, seconds, radians, cycles.
///
/// A "cycle" is one full two-step gait (left plant + right plant). The gait
/// phase advances by `actual planar distance / effective stride length`, so a
/// full cycle covers `stride_length` yards of real travel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocomotionTuning {
    /// Full-cycle stride length at a slow jog, yd.
    pub jog_stride: f32,
    /// Full-cycle stride length at a full sprint, yd.
    pub sprint_stride: f32,
    /// Below this planar speed the gait settles instead of running, yd/s.
    pub min_gait_speed: f32,
    /// Speed at (or above) which the stride reaches `sprint_stride`, yd/s.
    pub sprint_speed: f32,
    /// Cadence ceiling, cycles/s — stride is lengthened before cadence exceeds
    /// this, so the legs never blur to compensate for too-short a stride.
    pub max_cadence: f32,
    /// Base mid-swing foot lift at a walk, yd — the always-on arc height.
    pub foot_lift: f32,
    /// Peak EXTRA foot height at a full-speed knee lift, yd — the swing foot is
    /// driven this much higher on top of `foot_lift`, blended by speed, which is
    /// what gives a sprint a high, snappy knee instead of a low shuffle.
    pub knee_height: f32,
    /// How far FORWARD of the hip the driven knee reaches at mid-swing, yd — the
    /// mid-swing foot aims at a point this far ahead of (and `knee_height` above)
    /// the hip, so the thigh lifts the knee up in FRONT rather than the foot
    /// tucking straight under the body.
    pub knee_forward: f32,
    /// Strength of the knee drive: how strongly (0..1) the mid-swing foot is
    /// pulled from its flat glide toward the forward-and-high knee apex, blended
    /// by speed. `0` restores a flat skimming swing.
    pub knee_drive: f32,
    /// How far ahead of the hip a foot plants, yd — bounded by the (stubby
    /// arcade) leg's reach so the stance foot is always solvable.
    pub stance_reach: f32,
    /// Maximum fraction of the cycle a foot may stay planted at LOW speed. At
    /// speed the planted fraction shrinks (`2·stance_reach / stride`) so the
    /// world-locked foot is released before the leg over-extends — the anti-slide
    /// guarantee. Short ground contact at a sprint is the correct, realistic look.
    pub planted_fraction: f32,
    /// Stride multiplier while starting from a stand.
    pub startup_stride_scale: f32,
    /// Stride multiplier while stopping.
    pub stopping_stride_scale: f32,
    /// Stride multiplier through a sharp turn.
    pub turning_stride_scale: f32,
    /// Ticks the startup ramp takes to reach full stride.
    pub startup_ticks: f32,
    /// Ticks the stopping settle takes.
    pub stopping_ticks: f32,
    /// Half the lateral PLANT width (foot offset from centerline), yd. Kept
    /// INSIDE the model's hip half-width so the legs converge toward the midline
    /// the way a runner's do; a plant wider than the hips reads bow-legged from
    /// behind. Only the plant is laterally free — the tucked mid-swing foot is
    /// always held on its own hip's line (see `locomotion::foot`).
    pub stance_half_width: f32,
    /// Extra PLANT widening at full turn intensity, yd — a wider base through a
    /// cut. Does not widen the swing.
    pub turn_widen: f32,
    /// Pelvis yaw amplitude toward the leading leg, rad.
    pub pelvis_yaw: f32,
    /// Forward torso lean per unit planar acceleration, rad per yd/s².
    pub torso_lean_per_accel: f32,
    /// Maximum forward torso lean, rad.
    pub torso_lean_max: f32,
    /// Lateral torso bank at full turn intensity, rad.
    pub torso_bank: f32,
    /// Forward lean from the waist (torso-joint pitch) at a full sprint, rad,
    /// blended in by normalized speed — the runner's forward carriage over the
    /// hips, on top of the whole-body `root_pitch` lean. The head is counter-
    /// pitched so it stays up.
    pub waist_lean: f32,
    /// Arm-swing amplitude (upper arm pitch), rad.
    pub arm_swing: f32,
    /// Elbow flex held while standing / walking slowly (forearm pitch), rad.
    pub elbow_flex_idle: f32,
    /// Elbow flex held at a full run (forearm pitch), rad — runners keep the
    /// elbows bent ~90°, so this is deeper than `elbow_flex_idle` and the two
    /// are blended by normalized speed.
    pub elbow_flex_run: f32,
    /// Extra elbow flex on the arm that is driving forward this half-cycle, rad
    /// — the pump that opens the trailing elbow and closes the leading one.
    pub elbow_pump: f32,
    /// Turn rate (rad/s) that maps to full turn intensity.
    pub turn_full_rate: f32,
    /// A per-tick planar jump beyond this (yd) is treated as a teleport: the
    /// gait does not advance and the foot locks reset.
    pub teleport_distance: f32,
    /// How far below standing height the hips ride at a full run, yd, blended in
    /// by normalized speed. The (stubby, arcade) leg's reach is barely longer
    /// than the standing hip height, so at full height the stance leg solves to
    /// a LOCKED-STRAIGHT pole at every foot-strike — a stiff, stilted gait with
    /// no landing flex. Riding the hips lower is what buys the stance knee room
    /// to bend, so the runner absorbs each strike instead of pogo-sticking.
    pub run_crouch: f32,
    /// How far below standing height the hips ride in the *set* pre-snap
    /// stance, yd. This is the deliberate crouch of a player waiting for the
    /// snap — it is held only in [`crate::player::AnimState::ReadyStance`],
    /// never by a player standing around after the whistle.
    pub ready_crouch: f32,
    /// Forward pitch of the visual body root in the set pre-snap stance, rad.
    pub ready_pitch: f32,
}

impl Default for LocomotionTuning {
    fn default() -> Self {
        LocomotionTuning {
            jog_stride: 1.7,
            sprint_stride: 3.2,
            min_gait_speed: 0.4,
            sprint_speed: 8.4,
            max_cadence: 3.1,
            foot_lift: 0.34,
            knee_height: 0.26,
            knee_forward: 0.28,
            knee_drive: 0.72,
            stance_reach: 0.22,
            planted_fraction: 0.62,
            startup_stride_scale: 0.55,
            stopping_stride_scale: 0.5,
            turning_stride_scale: 0.72,
            startup_ticks: 16.0,
            stopping_ticks: 12.0,
            stance_half_width: 0.09,
            turn_widen: 0.09,
            pelvis_yaw: 0.12,
            torso_lean_per_accel: 0.02,
            torso_lean_max: 0.44,
            torso_bank: 0.28,
            waist_lean: 0.30,
            arm_swing: 0.95,
            elbow_flex_idle: 0.35,
            elbow_flex_run: 1.35,
            elbow_pump: 0.52,
            turn_full_rate: 3.0,
            teleport_distance: 3.0,
            run_crouch: 0.07,
            ready_crouch: 0.14,
            ready_pitch: 0.2,
        }
    }
}
