//! Tuning constants for **Gravix** — a physics rolling-ball speed game on shallow
//! half-pipe tracks. Every control-feel, physics, camera, spin-launch, half-pipe
//! shape, and course-layout knob lives here so the whole game is legible and
//! reproducible from named values (an app-tier concern; the engine spine holds
//! none of this). Adjust these to re-tune the feel.

use axiom::prelude::Vec3;

// --- simulation -------------------------------------------------------------

/// Gravity (world units / s²) — heavier than earth so descents build real speed
/// and the ball reads weighty, not floaty.
pub const GRAVITY: Vec3 = Vec3::new(0.0, -26.0, 0.0);

/// Fixed simulation step: 60 Hz, in nanoseconds for the runtime step.
pub const FIXED_STEP_NANOS: u64 = 16_666_667;

/// Substeps per fixed step — extra integration slices so a fast ball does not
/// tunnel through the track surface at speed.
pub const MAX_SUBSTEPS: u32 = 8;

/// Sequential-impulse solver iterations per (sub)step.
pub const SOLVER_ITERATIONS: u32 = 10;

/// Per-step linear velocity decay (tiny — applied every fixed step). Enough to
/// settle jitter and give a gentle coast drag without fighting acceleration.
pub const LINEAR_DAMPING: f32 = 0.0002;

/// Per-step angular velocity decay (rolling resistance) — keeps roll bounded.
pub const ANGULAR_DAMPING: f32 = 0.001;

// --- ball -------------------------------------------------------------------

/// Ball collision + visual radius.
pub const BALL_RADIUS: f32 = 0.55;

/// Ball mass.
pub const BALL_MASS: f32 = 2.0;

/// Ball surface friction (combined geometric-mean with the track) — deliberately
/// low/arcade-slippery so gravity accelerates the ball down the shallow slope
/// (high friction would statically pin a rolling ball on a gentle grade). Control
/// comes from the direct linear drive force, and the raised half-pipe lips contain
/// the ball geometrically, so low friction costs neither steering nor banking.
pub const BALL_FRICTION: f32 = 0.10;

/// Ball restitution — low, so it rolls and banks rather than bouncing (this is
/// what keeps a shallow curved surface jitter-free).
pub const BALL_RESTITUTION: f32 = 0.05;

/// Static track surface friction (matches the ball — slippery arcade track).
pub const TRACK_FRICTION: f32 = 0.10;

/// Static track surface restitution (near zero → no bounce on the facets).
pub const TRACK_RESTITUTION: f32 = 0.02;

/// Safety cap on horizontal speed (world units / s). A guard against runaway
/// integration on steep descents — **not** the main way the game feels controlled.
pub const MAX_SPEED: f32 = 42.0;

// --- movement controls ------------------------------------------------------

/// Camera-relative drive force applied along the pressed input direction — the
/// primary accelerator, so the ball responds immediately from a standstill instead
/// of waiting on friction. Forward and steering share one force because input is a
/// single combined direction vector.
pub const DRIVE_FORCE: f32 = 78.0;

/// Roll torque applied per pressed direction — the visible spin that accompanies
/// the linear drive and lets the ball carve into a bank.
pub const ROLL_TORQUE: f32 = 60.0;

/// Reference horizontal speed for the sublinear drive attenuation (a soft top
/// speed: input force fades as the ball approaches it, so high-speed input biases
/// direction rather than piling on velocity).
pub const DRIVE_SPEED_REFERENCE: f32 = 30.0;

/// Exponent of the sublinear drive-vs-speed attenuation.
pub const DRIVE_SPEED_EXPONENT: f32 = 1.5;

/// Drive/steer force multiplier while airborne (reduced control off the surface).
pub const AIR_CONTROL: f32 = 0.35;

/// A contact counts as "grounded" when its world normal points upward at least
/// this much (dot with +Y) — forgiving so banks and slopes still read as grounded.
pub const GROUNDED_NORMAL_Y: f32 = 0.35;

// --- chase camera -----------------------------------------------------------

/// Distance the chase camera trails behind the ball.
pub const CAM_DISTANCE: f32 = 12.0;

/// Height the chase camera sits above the ball.
pub const CAM_HEIGHT: f32 = 4.6;

/// How far ahead of the ball the camera looks (along its facing).
pub const CAM_LOOK_AHEAD: f32 = 6.0;

/// Exponential smoothing rate for the camera eye (per second) — higher snaps
/// faster; low enough to stay launch-safe (no snapping on a spin launch).
pub const CAM_EYE_SMOOTHING: f32 = 6.0;

/// Exponential smoothing rate for the camera facing (per second) — how fast the
/// facing turns toward the ball's horizontal velocity direction.
pub const CAM_FACING_SMOOTHING: f32 = 3.2;

/// Below this horizontal speed the camera holds its facing (no random spinning
/// when the ball is nearly stopped); above it, facing aligns to velocity.
pub const CAM_ALIGN_MIN_SPEED: f32 = 2.5;

// --- spin-launch (Sonic-style spin dash) ------------------------------------

/// Below this horizontal speed the braked ball is "nearly stopped" and tapping a
/// move key charges a spin instead of steering.
pub const SPIN_STOP_SPEED: f32 = 2.2;

/// Per-second exponential linear velocity decay while braking.
pub const BRAKE_LINEAR_DECAY: f32 = 8.5;

/// Per-second exponential angular velocity decay while braking.
pub const BRAKE_ANGULAR_DECAY: f32 = 9.5;

/// Charge added per move-key tap while spin-charging.
pub const SPIN_CHARGE_PER_TAP: f32 = 0.22;

/// Maximum stored spin charge (taps beyond this are capped).
pub const SPIN_CHARGE_MAX: f32 = 1.0;

/// Per-second charge decay while braked + charging but not tapping.
pub const SPIN_CHARGE_DECAY: f32 = 0.35;

/// Visible in-place spin rate (angular velocity, rad/s) per unit of charge while
/// charging — the ball whirls faster as it winds up.
pub const SPIN_CHARGE_VISUAL: f32 = 34.0;

/// Launch forward speed (world units / s) at full charge on release.
pub const SPIN_LAUNCH_LINEAR: f32 = 40.0;

/// Launch angular speed (rad/s) at full charge — matches the linear launch so the
/// ball rolls, not slides.
pub const SPIN_LAUNCH_ANGULAR: f32 = 55.0;

// --- half-pipe shape --------------------------------------------------------

/// Half-pipe channel full width (world units, across the roll direction).
pub const HALFPIPE_WIDTH: f32 = 12.0;

/// Shallow-U depth: how much higher the channel edge sits than its centre — small,
/// so it is a gentle bank the ball rides, not a deep skate ramp.
pub const HALFPIPE_CURVE_DEPTH: f32 = 1.4;

/// Extra raised **lip** height at the very edge, so the ball is contained and can
/// bank without immediately falling off.
pub const HALFPIPE_LIP_HEIGHT: f32 = 1.6;

/// Fraction of the half-width at which the lip begins rising (inside this the
/// surface is the shallow parabola; outside it the lip climbs steeply).
pub const HALFPIPE_LIP_START: f32 = 0.82;

/// Grid vertex spacing along the width (fine enough to reveal the shallow curve).
pub const HALFPIPE_TESS_WIDTH: f32 = 0.75;

/// Grid vertex spacing along the length.
pub const HALFPIPE_TESS_LENGTH: f32 = 2.0;

// --- course -----------------------------------------------------------------

/// How far below the course's lowest track point the kill plane sits — a fall past
/// it resets the ball to the spawn.
pub const KILL_PLANE_DROP: f32 = 14.0;

/// Finish-gate trigger radius (horizontal).
pub const FINISH_RADIUS: f32 = 5.0;
