//! Tuning constants for the marble game — control feel, gameplay rules, and the
//! procedural-generation dials. Kept in one place so the whole game is legible
//! and reproducible from a handful of named knobs (an app tier concern; the
//! engine spine holds none of this).

use axiom::prelude::Vec3;

/// Simulation gravity (world units / s²) — a snappy, heavier-than-earth pull so
/// the marble reads as fast and weighty rather than floaty.
pub const GRAVITY: Vec3 = Vec3::new(0.0, -28.0, 0.0);

/// Fixed simulation step: 60 Hz, expressed in nanoseconds for the runtime step.
pub const FIXED_STEP_NANOS: u64 = 16_666_667;

/// Substeps per fixed step — extra integration slices so a fast marble does not
/// tunnel through thin platforms.
pub const MAX_SUBSTEPS: u32 = 8;

/// Sequential-impulse solver iterations per (sub)step.
pub const SOLVER_ITERATIONS: u32 = 8;

/// Per-step global linear velocity decay fraction. This is applied *every* fixed
/// step, so it must be tiny: `0.002/step` ≈ an 11%/s coast drag, enough to settle
/// but not fight acceleration.
pub const LINEAR_DAMPING: f32 = 0.002;

/// Per-step global angular velocity decay fraction (keeps roll from winding up
/// without bound) — likewise tiny, applied every step.
pub const ANGULAR_DAMPING: f32 = 0.004;

/// Marble collision + visual radius (world units).
pub const MARBLE_RADIUS: f32 = 0.5;

/// Marble mass.
pub const MARBLE_MASS: f32 = 2.0;

/// Marble surface friction (combined with the platform's, geometric-mean) — high
/// so applied torque grips the deck and converts to forward roll.
pub const MARBLE_FRICTION: f32 = 1.0;

/// Marble restitution — low, so it rolls rather than bounces.
pub const MARBLE_RESTITUTION: f32 = 0.08;

/// Static platform friction.
pub const PLATFORM_FRICTION: f32 = 1.0;

/// Static platform restitution.
pub const PLATFORM_RESTITUTION: f32 = 0.05;

/// Direct camera-relative drive force — the *primary* accelerator, so steering
/// feels responsive rather than waiting on friction to convert spin to motion.
pub const ROLL_FORCE: f32 = 65.0;

/// Roll torque magnitude applied per pressed direction (camera-relative) — mainly
/// for the visual roll now that a linear force drives the acceleration.
pub const ROLL_TORQUE: f32 = 55.0;

/// Reference horizontal speed for the sublinear drive attenuation (a soft top
/// speed — the drive fades as the marble approaches it).
pub const ROLL_SPEED_REFERENCE: f32 = 26.0;

/// Exponent of the sublinear torque-vs-speed attenuation.
pub const ROLL_SPEED_EXPONENT: f32 = 1.55;

/// Upward impulse of a jump.
pub const JUMP_IMPULSE: f32 = 13.0;

/// While braking, roll torque is scaled by this (so steering still works a bit).
pub const BRAKE_TORQUE_SCALE: f32 = 0.52;

/// Per-second linear-velocity decay applied while braking (exponential).
pub const BRAKE_LINEAR_DECAY: f32 = 7.45;

/// Per-second angular-velocity decay applied while braking (exponential).
pub const BRAKE_ANGULAR_DECAY: f32 = 9.1;

/// A contact counts as "grounded" (jump enabled) when its world normal points
/// upward at least this much (dot with +Y). Slightly forgiving so ramps still
/// allow a jump.
pub const GROUNDED_NORMAL_Y: f32 = 0.5;

/// Camera orbit distance from the marble.
pub const CAMERA_DISTANCE: f32 = 13.0;

/// Camera pitch clamp (radians).
pub const CAMERA_PITCH_MIN: f32 = 0.12;
pub const CAMERA_PITCH_MAX: f32 = 1.45;

/// Camera orbit input speeds (radians / second).
pub const CAMERA_YAW_SPEED: f32 = 2.1;
pub const CAMERA_PITCH_SPEED: f32 = 1.5;

/// Initial camera pitch on level load.
pub const CAMERA_INITIAL_PITCH: f32 = 0.52;

/// Extra yaw offset applied when aiming the initial orbit down the course.
pub const CAMERA_COURSE_YAW_OFFSET: f32 = -core::f32::consts::FRAC_PI_6;

/// A fall this far below the spawn height is a death (the fallback kill plane
/// when the level does not specify a lower one).
pub const FALL_DEATH_BELOW_SPAWN: f32 = 12.0;

/// Falls allowed per run before it is over.
pub const RUN_MAX_FALLS: u32 = 3;

/// Coin pickup radius (added to the marble radius for the overlap test).
pub const COIN_PICKUP_RADIUS: f32 = 0.46;
