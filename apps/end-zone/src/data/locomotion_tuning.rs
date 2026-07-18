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
    /// Peak foot lift height mid-swing, yd.
    pub foot_lift: f32,
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
    /// Half the lateral stance width (foot offset from centerline), yd.
    pub stance_half_width: f32,
    /// Extra stance widening at full turn intensity, yd.
    pub turn_widen: f32,
    /// Pelvis vertical bob amplitude, yd.
    pub pelvis_bob: f32,
    /// Pelvis yaw amplitude toward the leading leg, rad.
    pub pelvis_yaw: f32,
    /// Forward torso lean per unit planar acceleration, rad per yd/s².
    pub torso_lean_per_accel: f32,
    /// Maximum forward torso lean, rad.
    pub torso_lean_max: f32,
    /// Lateral torso bank at full turn intensity, rad.
    pub torso_bank: f32,
    /// Shoulder counter-rotation amplitude, rad.
    pub shoulder_counter: f32,
    /// Arm-swing amplitude (upper arm pitch), rad.
    pub arm_swing: f32,
    /// Turn rate (rad/s) that maps to full turn intensity.
    pub turn_full_rate: f32,
    /// A per-tick planar jump beyond this (yd) is treated as a teleport: the
    /// gait does not advance and the foot locks reset.
    pub teleport_distance: f32,
    /// Landing-compression dip at foot strike, yd.
    pub landing_dip: f32,
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
            stance_reach: 0.22,
            planted_fraction: 0.62,
            startup_stride_scale: 0.55,
            stopping_stride_scale: 0.5,
            turning_stride_scale: 0.72,
            startup_ticks: 16.0,
            stopping_ticks: 12.0,
            stance_half_width: 0.14,
            turn_widen: 0.12,
            pelvis_bob: 0.05,
            pelvis_yaw: 0.12,
            torso_lean_per_accel: 0.02,
            torso_lean_max: 0.34,
            torso_bank: 0.22,
            shoulder_counter: 0.16,
            arm_swing: 0.7,
            turn_full_rate: 3.0,
            teleport_distance: 3.0,
            landing_dip: 0.05,
        }
    }
}
