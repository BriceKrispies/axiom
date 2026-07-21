//! Whole-body sprint biomechanics tuning: the named knobs the locomotion
//! animator's *carriage* pass reads. Where [`crate::data::LocomotionTuning`]
//! owns the leg cycle (stride, cadence, foot lock, swing arc), this owns the
//! body riding on top of it — how the pelvis carries weight over the stance
//! leg, how the spine counter-rotates against it, and how stiffly each region
//! chases its target pose.
//!
//! Every number the carriage uses lives here; the carriage solver itself
//! contains no unexplained literals. Units: yards, radians, seconds.
//!
//! The stiffness/damping pairs drive the deterministic fixed-step springs in
//! [`crate::presentation::locomotion::spring`] — the "virtual muscle" response.
//! Stiffer regions (pelvis, stance leg) track their target almost exactly;
//! looser regions (arms, upper spine) trail slightly, which is what makes the
//! body read as connected rather than as independently rotating limbs.

/// Whole-body sprint biomechanics tuning. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiomechTuning {
    // ----- pelvis translation (the visual body root) ----------------------
    /// How far the pelvis sinks while the stance leg accepts body weight, yd.
    /// This is the compression that makes a stride land instead of hover.
    pub weight_accept_dip: f32,
    /// Where in the stance (0 = strike, 1 = toe-off) the sink bottoms out.
    pub weight_accept_center: f32,
    /// How far the pelvis is driven back up through push-off, yd.
    pub push_off_rise: f32,
    /// Fraction of the stance at which push-off extension begins (0..1).
    pub push_off_start: f32,
    /// Extra rise carried through the flight/transition arc, yd.
    pub flight_rise: f32,
    /// Peak lateral pelvis shift toward the stance leg, yd.
    pub lateral_shift: f32,
    /// Fraction of the half-cycle spent crossing weight to the other leg. A
    /// wider window is a smoother, less mechanical transfer.
    pub lateral_crossover: f32,
    /// Hard bound on total vertical pelvis travel, yd — the anti-bounce clamp.
    pub vertical_bound: f32,
    /// Hard bound on total lateral pelvis travel, yd.
    pub lateral_bound: f32,

    // ----- pelvis rotation ------------------------------------------------
    /// Pelvis yaw per yard of longitudinal separation between the swing and
    /// stance feet, rad/yd. Driving yaw off the *actual* foot geometry (rather
    /// than a raw sine) is what makes the pelvis follow the advancing leg.
    pub pelvis_yaw_per_yard: f32,
    /// Bound on pelvis yaw, rad.
    pub pelvis_yaw_max: f32,
    /// Peak drop of the unsupported (swing-side) hip, rad — the Trendelenburg
    /// dip that reads as weight hanging off the stance leg.
    pub pelvis_drop: f32,
    /// Anterior pelvic tilt at a full sprint, rad.
    pub pelvis_tilt_speed: f32,
    /// Extra anterior tilt per unit forward acceleration, rad per yd/s².
    pub pelvis_tilt_per_accel: f32,
    /// Bound on pelvis pitch, rad.
    pub pelvis_tilt_max: f32,
    /// Whole-body forward lean of the visual body root at a full sprint, rad —
    /// the carriage the waist lean and pelvis tilt then build on.
    pub root_lean_speed: f32,

    // ----- torso coupling -------------------------------------------------
    /// Fraction of the pelvis yaw countered at the lower spine (torso joint).
    pub spine_counter_yaw: f32,
    /// Fraction countered again at the ribcage / shoulder girdle (pad joint),
    /// on top of the spine. Shoulders and head hang off this joint, so this is
    /// what visibly counter-rotates the upper body against the hips.
    pub ribcage_counter_yaw: f32,
    /// Fraction of the pelvis roll compensated back by the spine.
    pub spine_roll_compensation: f32,
    /// Share of the sprint forward lean carried by the lower spine (the rest
    /// goes to the ribcage), 0..1 — the lean is distributed, not stacked on one
    /// joint.
    pub lean_spine_share: f32,
    /// How much of the accumulated body pitch/yaw the head cancels, 0..1. Below
    /// 1 the head still follows the body; at 1 it would be gimballed.
    pub head_stabilization: f32,

    // ----- virtual muscle (deterministic fixed-step springs) --------------
    /// Pelvis spring stiffness (1/s²) and damping ratio. Stiff: the pelvis is
    /// the controlled, weight-bearing region.
    pub pelvis_stiffness: f32,
    pub pelvis_damping: f32,
    /// Spine / ribcage spring stiffness (1/s²) and damping ratio.
    pub spine_stiffness: f32,
    pub spine_damping: f32,
    /// Arm spring stiffness (1/s²) and damping ratio. Loosest region: the arms
    /// are allowed to trail, which is most of the "connected body" read.
    pub arm_stiffness: f32,
    pub arm_damping: f32,
    /// Head spring stiffness (1/s²) and damping ratio.
    pub head_stiffness: f32,
    pub head_damping: f32,
    /// Maximum positional correction a spring may apply in one tick, yd.
    pub max_position_step: f32,
    /// Maximum angular correction a spring may apply in one tick, rad.
    pub max_angular_step: f32,

    // ----- activity gate --------------------------------------------------
    /// Normalized speed at (and above) which the carriage reaches full
    /// amplitude. Below it every gait-driven offset fades out, so a standing
    /// player does not keep performing a sprint cycle — and, just as
    /// importantly, a walk gets only a fraction of the sway a sprint does.
    /// The aggressive carriage is something the player has to earn with speed.
    pub full_carriage_speed: f32,
}

impl Default for BiomechTuning {
    fn default() -> Self {
        BiomechTuning {
            weight_accept_dip: 0.090,
            weight_accept_center: 0.30,
            push_off_rise: 0.080,
            push_off_start: 0.55,
            flight_rise: 0.055,
            lateral_shift: 0.088,
            lateral_crossover: 0.26,
            vertical_bound: 0.22,
            lateral_bound: 0.15,

            pelvis_yaw_per_yard: 0.17,
            pelvis_yaw_max: 0.27,
            pelvis_drop: 0.115,
            pelvis_tilt_speed: 0.10,
            pelvis_tilt_per_accel: 0.012,
            pelvis_tilt_max: 0.21,
            root_lean_speed: 0.17,

            spine_counter_yaw: 0.65,
            ribcage_counter_yaw: 0.95,
            spine_roll_compensation: 0.5,
            lean_spine_share: 0.6,
            head_stabilization: 0.8,

            pelvis_stiffness: 1750.0,
            pelvis_damping: 1.0,
            spine_stiffness: 480.0,
            spine_damping: 0.95,
            arm_stiffness: 215.0,
            arm_damping: 0.72,
            head_stiffness: 240.0,
            head_damping: 1.05,
            max_position_step: 0.07,
            max_angular_step: 0.26,

            full_carriage_speed: 0.72,
        }
    }
}
