/*
 * constants.ts — every tuning number for Swipe Basketball in one place, as the
 * brief asks: gravity, ball radius, throw/lift/forward scales, drag smoothing,
 * scoring-trigger dimensions, restitution, damping, cabinet dimensions, and hoop
 * dimensions. Pure data, no imports — the SDK-free core and the scene both read it.
 *
 * World frame: +X right, +Y up, +Z toward the player (out of the screen). The
 * player stands in front of the cabinet near +Z; the hoop is high and deep at −Z;
 * the balls rest in a rack in the low near foreground. All lengths are metres,
 * all times are fixed 60 Hz ticks (`dt = 1/60 s`), so the sim is replayable.
 */

// ── simulation ────────────────────────────────────────────────────────────────

/** Fixed simulation rate — one deterministic step per 1/60 s. */
export const FIXED_HZ = 60;
/** The fixed timestep in seconds. */
export const DT = 1 / FIXED_HZ;
/** The deterministic seed (the rack layout is fixed, so this is only a formality). */
export const SEED = 1n;

// ── ball ──────────────────────────────────────────────────────────────────────

/** Basketball radius (a slightly-large arcade ball, easy to grab and read). */
export const BALL_RADIUS = 0.12;
/** How many basketballs live in the machine at once (rack + in play). */
export const BALL_COUNT = 5;

// ── physics ─────────────────────────────────────────────────────────────────

/** Gravity acceleration (m/s²), negative = down. Punchier than real g for arcade feel. */
export const GRAVITY = -11.0;
/** Fraction of linear speed bled off per second in flight (air drag). */
export const LINEAR_DAMPING = 0.12;
/** Coefficient of restitution for a bounce off static geometry (0 dead … 1 elastic). */
export const RESTITUTION = 0.52;
/** Tangential velocity retained after a contact (1 = frictionless slide, 0 = grip). */
export const TANGENTIAL_FRICTION = 0.78;
/** Below this speed (m/s) a ball resting on the ramp/tray is snapped to rest + recycled. */
export const REST_SPEED = 0.55;
/** Ticks a ball must be slow-and-low before it recycles to the rack. */
export const REST_TICKS = 36;

// ── swipe → throw mapping ─────────────────────────────────────────────────────
//
// Pointer velocity arrives in canvas-pixels-per-tick (see pointer.ts). These
// scales convert that 2D flick into a 3D launch velocity (m/s).

/** Horizontal swipe (px/tick, +right) → world +X launch velocity. */
export const THROW_SCALE_X = 0.09;
/** Upward swipe (px/tick, up) → world +Y lift. Only upward motion contributes. */
export const LIFT_SCALE = 0.22;
/** Overall flick speed (px/tick) → world −Z forward velocity (into the machine). */
export const FORWARD_SCALE = 0.12;
/** A floor of forward velocity so even a straight-up flick carries toward the hoop. */
export const FORWARD_BASE = 0.8;
/** Clamp on the launch speed so an absurd flick can't fling the ball out of the world. */
export const MAX_THROW_SPEED = 14.0;

// ── drag / hold ───────────────────────────────────────────────────────────────

/** Interpolation toward the pointer target while holding (0 sluggish … 1 rigid). */
export const DRAG_SMOOTHING = 0.45;
/** The z-depth of the vertical plane the held ball is dragged on (near the rack). */
export const DRAG_PLANE_Z = 1.15;
/** Selecting a ball: the pointer must fall within this many *projected* ball radii. */
export const SELECT_RADIUS_FACTOR = 1.6;

// ── pointer history ───────────────────────────────────────────────────────────

/** Fixed capacity of the pointer-sample ring buffer (bounded, never grows). */
export const POINTER_HISTORY = 12;
/** Samples newer than this many ticks feed the release-velocity estimate. */
export const VELOCITY_WINDOW = 6;
/** A per-tick pixel delta larger than this is a tab-switch/focus glitch → discard history. */
export const MAX_POINTER_DELTA = 400;

// ── cabinet dimensions ────────────────────────────────────────────────────────

/** Half-width of the play shaft (side rails sit at ±this). */
export const CABINET_HALF_WIDTH = 1.05;
/** Cabinet floor plane height. */
export const FLOOR_Y = 0.0;
/** Front lip (near wall) — how far up the front of the tray it rises. */
export const FRONT_LIP_Y = 0.42;
/** Near/far Z extents of the cabinet interior. */
export const CABINET_NEAR_Z = 1.65;
export const CABINET_FAR_Z = -3.05;

// ── return ramp ───────────────────────────────────────────────────────────────
//
// A plane sloping down from just under the hoop (far, higher) to the rack (near,
// lower), so a ball that misses rolls back to the player.

export const RAMP_FAR_Z = -2.35;
export const RAMP_FAR_Y = 0.62;
export const RAMP_NEAR_Z = 1.5;
export const RAMP_NEAR_Y = 0.26;

/** Where balls sit at rest in the rack (near foreground, on the ramp lip). */
export const RACK_Z = 1.25;
export const RACK_Y = RAMP_NEAR_Y + BALL_RADIUS + 0.02;
/** The rack spreads the balls evenly across this fraction of the shaft width. */
export const RACK_SPREAD = 0.72;

// ── hoop ──────────────────────────────────────────────────────────────────────

/** Rim centre (the hoop opening). High and deep, framed in the upper-middle. */
export const HOOP_X = 0.0;
export const HOOP_Y = 2.28;
export const HOOP_Z = -2.35;
/** Inner radius of the rim opening (forgiving arcade size vs. the 0.12 ball). */
export const RIM_RADIUS = 0.24;
/** Thickness of the rim tube (the torus minor radius / the collider-box size). */
export const RIM_TUBE = 0.022;
/** How many small static boxes approximate the rim ring for collision + the torus segs. */
export const RIM_SEGMENTS = 16;

/** Backboard: a vertical panel behind and above the rim. */
export const BACKBOARD_Y = HOOP_Y + 0.42;
export const BACKBOARD_Z = HOOP_Z - 0.30;
export const BACKBOARD_HALF_W = 0.62;
export const BACKBOARD_HALF_H = 0.42;
export const BACKBOARD_HALF_D = 0.03;

// ── scoring trigger ───────────────────────────────────────────────────────────
//
// A box just below the rim. A shot scores only when the ball centre crosses the
// rim plane (HOOP_Y) downward, from above to below, while inside this box.

/** Half-extents of the scoring trigger volume, centred under the rim. */
export const TRIGGER_HALF_W = RIM_RADIUS;
export const TRIGGER_HALF_D = RIM_RADIUS;
/** The trigger box spans from just under the rim down this far. */
export const TRIGGER_HALF_H = 0.14;
/** The trigger box centre Y (its top ≈ the rim plane). */
export const TRIGGER_CENTER_Y = HOOP_Y - TRIGGER_HALF_H;

// ── camera ────────────────────────────────────────────────────────────────────
//
// A fixed camera at the player's viewpoint, facing the machine. Hoop lands in the
// upper-middle; the rack of balls sits reachable in the lower foreground.

export const CAMERA_POS = { x: 0.0, y: 1.12, z: 4.5 };
export const CAMERA_TARGET = { x: 0.0, y: 1.48, z: -1.2 };
/** Vertical field of view in radians. */
export const CAMERA_FOV_Y = (44 * Math.PI) / 180;
export const CAMERA_NEAR = 0.05;
export const CAMERA_FAR = 60.0;

/** Default viewport (canvas backing size) used until the harness reports the real one. */
export const DEFAULT_VIEWPORT = { x: 960, y: 600 };
