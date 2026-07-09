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

/**
 * Gravity acceleration (m/s²), negative = down. The app OWNS gravity (there is no
 * engine physics here); a heavy −20 keeps shots snappy and flat, not floaty.
 */
export const GRAVITY = -19.0;
/** Optional multiplier on gravity (kept at 1: `GRAVITY` is already the tuned value). */
export const THROW_GRAVITY_SCALE = 1.0;
/**
 * LOW air drag in free flight (fraction of linear speed bled off per second), so a
 * thrown ball keeps its momentum and drives to the hoop rather than mushing out.
 */
export const LINEAR_DAMPING = 0.03;
/**
 * MODERATE energy loss stamped onto a ball's velocity right after any bounce (a
 * factor it is multiplied by), so contacts feel like a heavy basketball settling
 * rather than a ping-pong ball — this, plus the restitutions below, kills the
 * endless bounce.
 */
export const POST_COLLISION_DAMPING = 0.88;
/** Restitution on a RIM contact — deadest, so a rim hit drops through the net, not out. */
export const RESTITUTION_RIM = 0.30;
/** Restitution on a BACKBOARD contact — livelier than the rim, for a real bank shot. */
export const RESTITUTION_BACKBOARD = 0.50;
/** Restitution on all other static geometry (ramp, rails, front lip). */
export const RESTITUTION_DEFAULT = 0.40;
/** Tangential velocity retained after a contact (1 = frictionless slide, 0 = grip). */
export const TANGENTIAL_FRICTION = 0.70;
/** Below this speed (m/s) a ball resting on the ramp/tray is snapped to rest + recycled. */
export const REST_SPEED = 0.55;
/** Ticks a ball must be slow-and-low before it recycles to the rack. */
export const REST_TICKS = 36;

// ── swipe → throw mapping (constrained arcade model) ──────────────────────────
//
// The release gesture (smoothed pointer velocity, canvas-px/tick — see pointer.ts)
// is decomposed into THREE independent intents — power, lateral aim, upward — and
// mapped to a launch velocity whose FORWARD (−Z) component dominates. Vertical lift
// is clamped to a fraction of forward speed so a hard upward flick becomes a fast,
// flat arc INTO the machine, never a tall rainbow. Raw screen-Y velocity never
// becomes raw world-Y velocity.

// NOTE ON VALUES: the brief's suggested starting numbers (forward 11–18, ratio
// 0.48) describe a flat bullet. In THIS cabinet (hoop 2.28 m high, ~3.5 m from the
// launch plane) a bullet that fast physically cannot arc DOWN through a hoop above
// the launch point — it crosses the rim still rising and banks off the backboard,
// so nothing scores (verified by a full launch-height × flick-speed sweep). Tuning
// down opens a real scoring window while keeping the model's spirit: the shot is
// still forward-DOMINANT (forward > vertical always) and still a quick, controlled
// arc — never a rainbow (a rainbow is a >55° launch; the cap below is ~36°).

/** Minimum forward launch speed (m/s, toward −Z) — the softest release still drives in. */
export const THROW_FORWARD_MIN = 7.5;
/** Maximum forward launch speed (m/s) at full power. */
export const THROW_FORWARD_MAX = 10.5;
/** Minimum upward launch speed (m/s) — every release carries at least this much lift. */
export const THROW_VERTICAL_MIN = 5.4;
/** Maximum upward launch speed (m/s) at full power, before the ratio clamp. */
export const THROW_VERTICAL_MAX = 7.2;
/** Hard cap: vertical launch ≤ forward launch × this, keeping the shot forward-dominant. */
export const THROW_VERTICAL_TO_FORWARD_MAX_RATIO = 0.75;
/** Maximum lateral launch speed (m/s, ±X) from a sideways flick. */
export const THROW_LATERAL_MAX = 4.0;
/** How many of the most recent pointer samples the weighted release average spans. */
export const THROW_SAMPLE_WINDOW = 5;

/** Gesture speed (px/tick) at/below which power is 0 — a barely-there flick falls short. */
export const THROW_GESTURE_DEADZONE = 6.0;
/** Gesture speed (px/tick) at/above which power is 1 (full range). */
export const THROW_GESTURE_FULL = 46.0;

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
