/*
 * constants.ts — every tuning number for Sky Drop in one place. Pure data, no
 * imports — the SDK-free core and the scene both read it.
 *
 * World frame: +X right, +Y up, +Z toward the camera. The TARGET sits on the ground
 * plane (y = 0) centred at the origin. You STAND at a fixed point DROP_ALTITUDE metres
 * up and horizontally offset from it, and throw a rack of balls at it one after
 * another. All lengths are metres; all times are fixed 60 Hz ticks (dt = 1/60 s).
 *
 * The numbers here are not arbitrary — the fall time, the horizontal reach of a throw,
 * and the wind drift are all closed-form consequences of GRAVITY, LINEAR_DAMPING and
 * DROP_ALTITUDE. `sky-drop.test.ts` §6 asserts the ones that matter, so retuning any
 * one of them fails loudly rather than quietly making the game unplayable.
 */

// ── simulation ────────────────────────────────────────────────────────────────

/** Fixed simulation rate — one deterministic step per 1/60 s. */
export const FIXED_HZ = 60;
/** The fixed timestep in seconds. */
export const DT = 1 / FIXED_HZ;
/** The round seed. The stand position and the wind derive from this and the round. */
export const SEED = 20260808;

// ── the stand ─────────────────────────────────────────────────────────────────

/** Throwing altitude (m). High enough that the target reads as a disc far below. */
export const DROP_ALTITUDE = 180;
/** The ground plane. The target is painted on it. */
export const GROUND_Y = 0;
/** The ball's radius — a big arcade ball, readable from the fixed camera. */
export const BALL_RADIUS = 0.6;

/**
 * How far the stand sits from the target, horizontally (deterministic per round).
 *
 * Bounded at BOTH ends by the framing, not by taste. Too small and simply dropping a
 * ball nearly scores, so there is no game; too large and the angular separation
 * between the ball (metres from the camera) and the target (180 m below) exceeds the
 * field of view and one of the two leaves the frame. At 180 m, 26–48 m subtends 8°–15°.
 *
 * The lower bound has a second job: it must exceed `RING_OUTER + DRAG_REACH`, or a ball
 * could be carried out over the target and simply released, never thrown.
 */
export const STAND_OFFSET_MIN = 26;
export const STAND_OFFSET_MAX = 48;

// ── physics ───────────────────────────────────────────────────────────────────

/**
 * Gravity (m/s², negative = down). Frankly unphysical, and deliberately so: paired
 * with the damping below it gives a terminal velocity around 194 m/s and lands a throw
 * in ≈2.8 s. An honest 9.81 made a throw take five and a half seconds — accurate, and
 * far too long to sit through eight times.
 */
export const GRAVITY = -62.0;
/**
 * Air drag as a fraction of speed bled off per second, applied to the WHOLE velocity.
 * The most load-bearing constant in the game: it sets terminal velocity, caps the fall
 * time, and turns both the throw and the wind into bounded, learnable distances
 * instead of runaway ones.
 */
export const LINEAR_DAMPING = 0.32;
/** Restitution on the ground bounce — a landing is scored on FIRST contact; this is flourish. */
export const RESTITUTION_GROUND = 0.42;
/** Tangential velocity retained on a ground contact (1 = frictionless, 0 = grip). */
export const TANGENTIAL_FRICTION = 0.62;
/** Multiplicative energy loss stamped on the velocity right after a bounce. */
export const POST_COLLISION_DAMPING = 0.86;
/** Below this speed (m/s) a landed ball counts as settled and is frozen where it lies. */
export const REST_SPEED = 0.8;
/** Ticks a ball must be slow before it is frozen. */
export const REST_TICKS = 14;

// ── wind ──────────────────────────────────────────────────────────────────────
//
// Wind is a constant HORIZONTAL acceleration applied for the whole fall, and it is the
// same for every ball in a round — it is the weather you are throwing in, not a
// per-throw dice roll. Because the same damping acts on it, it converges to a drift
// speed of accel/LINEAR_DAMPING, which is exactly the number the HUD shows: a quantity
// you can learn to compensate rather than a decorative arrow.

/** Minimum wind acceleration (m/s²) — some rounds are nearly still. */
export const WIND_ACCEL_MIN = 0.1;
/** Maximum wind acceleration (m/s²). Drifts a ball ≈ 9 m over a full fall. */
export const WIND_ACCEL_MAX = 3.1;

// ── grab and throw ────────────────────────────────────────────────────────────
//
// The arcade cabinet's mechanic, kept: you pick a ball UP and move it, and when you let
// go it keeps the motion you gave it. There is no aiming widget and no power meter. The
// ball is carried across a horizontal plane at its own altitude, it follows your finger
// with a little weight, and the release velocity is read straight off how fast it was
// actually travelling (`motion.ts`).

/**
 * How far a ball can be carried (m) from the stand.
 *
 * Squeezed between two hard limits: small enough that a ball can never be walked out
 * over the target and released (`STAND_OFFSET_MIN − DRAG_REACH > RING_OUTER`, §6e), and
 * small enough that the whole reach stays on screen (§7e). It only has to be large
 * enough to get a ball up to speed, and the carry converges on the finger's speed
 * within ~5 ticks — about 2 m even for the longest throw.
 */
export const DRAG_REACH = 6;
/**
 * How hard a held ball chases the finger each tick (0 sluggish … 1 rigid). Below 1
 * gives the ball weight — it trails slightly, and a throw has to *carry* it — while
 * still converging on the finger's speed during a sustained swing.
 */
export const DRAG_SMOOTHING = 0.5;
/**
 * Safety ceiling on release speed (m/s). Not a game rule — a guard against a pointer
 * teleport (a tab switch, a lost capture) turning into an absurd launch.
 */
export const MAX_RELEASE_SPEED = 60;

/** Grabbing a ball: the pointer must land within this many projected ball radii. */
export const GRAB_RADIUS_FACTOR = 2.2;
/** …or this many pixels, whichever is larger, so the ball stays grabbable when small. */
export const GRAB_RADIUS_MIN_PX = 46;

/** Fixed capacity of the held-ball position ring buffer (bounded, never grows). */
export const MOTION_HISTORY = 12;
/** How many of the most recent samples the weighted release average spans. */
export const MOTION_SAMPLE_WINDOW = 5;

// ── the target ────────────────────────────────────────────────────────────────
//
// Concentric rings painted on the ground, scored by the horizontal distance from the
// centre at the moment of FIRST ground contact. Radii ascend; each band's points are
// the reward for landing inside it and outside the previous one.

/** Radius (m) of the dead-centre spot inside the bullseye — the best landing in the game. */
export const RING_DEAD_CENTRE = 0.6;
/** Radius (m) of the bullseye. */
export const RING_BULLSEYE = 1.8;
/** Radius (m) of the inner ring. */
export const RING_INNER = 4.5;
/** Radius (m) of the middle ring. */
export const RING_MID = 9.0;
/** Radius (m) of the outer ring — land beyond this and the throw scores nothing. */
export const RING_OUTER = 15.0;

/**
 * Four marker bars radiate from the target centre. A vertical beacon would be the
 * obvious "look here" cue, but it foreshortens to a dot in the near-vertical view —
 * flat ground markers are the shape that reads from overhead.
 */
export const MARKER_BAR_LENGTH = 42;
export const MARKER_BAR_WIDTH = 0.9;

export const POINTS_DEAD_CENTRE = 150;
export const POINTS_BULLSEYE = 100;
export const POINTS_INNER = 50;
export const POINTS_MID = 25;
export const POINTS_OUTER = 10;

// ── the round ─────────────────────────────────────────────────────────────────

/** How many balls you throw in one round. */
export const BALLS_PER_ROUND = 8;
/**
 * Ticks to wait after the last ball settles before the scoreboard appears.
 *
 * Nothing about the score is shown until every ball is down — no per-throw verdict, no
 * flash, no running total. A round is one continuous act of throwing, and interrupting
 * it with a scorecard eight times both breaks the rhythm and hands you a correction
 * mid-round. You throw the rack, then you find out.
 */
export const SETTLE_TICKS = 36;

// ── camera ────────────────────────────────────────────────────────────────────
//
// FIXED at the stand for the whole round, looking down at the target. It does not
// chase a thrown ball: the camera is where the player is, and balls fall away from it
// and get smaller, which is what makes 180 m read as a long way down. It also has to
// stay put because there are several balls in the air at once — following any one of
// them would be arbitrary, and would yank the frame away from the ball still in hand.

/** Camera pitch (radians below horizontal). Near-vertical — see STAND_OFFSET_MIN. */
export const CAMERA_PITCH = (86 * Math.PI) / 180;
/**
 * Camera distance (m) from the stand. Large on purpose: sit it a few metres off and a
 * 1.2 m ball fills as much of the frame as the 30 m target below it, and the shot reads
 * as a ball on a field rather than one high above a landscape.
 */
export const CAMERA_DIST = 30.0;
/**
 * How far along the stand→target line the camera looks. Aiming it AT the stand would
 * push the target to the frame edge and vice versa; a point partway between keeps both
 * on screen.
 */
export const CAMERA_LOOK_BIAS = 0.45;
/** Vertical field of view in radians. */
export const CAMERA_FOV_Y = (44 * Math.PI) / 180;
/**
 * Near plane. Deliberately far out, because depth precision — not clipping — is the
 * binding constraint: at 180 m the usable resolution is roughly `d² / (near · 2^24)`,
 * so a 0.08 m near plane resolves ~4 cm and the target's stacked ground rings z-fight
 * into speckle. Nothing is ever within 0.5 m of the eye.
 */
export const CAMERA_NEAR = 0.5;
/** Far plane — clears the far corner of the ground slab from the stand (≈529 m). */
export const CAMERA_FAR = 650.0;

/**
 * Vertical separation (m) between the target's stacked flat rings. Must stay well above
 * the depth resolution at viewing distance (see CAMERA_NEAR) or the rings interleave
 * into noise — invisible on a WebGPU device and glaring on the WebGL2 fallback.
 */
export const GROUND_LAYER_STEP = 0.06;
/** Height (m) of the lowest painted ground layer; each layer stacks a step above it. */
export const GROUND_LAYER_BASE = 0.14;

/** Default viewport (canvas backing size) used until the harness reports the real one. */
export const DEFAULT_VIEWPORT = { x: 960, y: 600 };

// ── world dressing ────────────────────────────────────────────────────────────

/** Half-extent (m) of the ground slab. Must cover the view cone from the stand. */
export const GROUND_HALF_EXTENT = 320;
/** How many scenery blocks ring the target, for parallax and a sense of scale. */
export const SCENERY_COUNT = 64;
/**
 * Scenery sits in an annulus between these radii (m). The inner radius must clear the
 * target AND its marker bars AND half a block's own diagonal — a block centred at 26 m
 * that is 26 m wide reaches inward to radius 13 and sits on the bullseye.
 */
export const SCENERY_INNER_RADIUS = 70;
export const SCENERY_OUTER_RADIUS = 300;
/** How many cloud slabs hang between the stand and the ground. */
export const CLOUD_COUNT = 13;
/**
 * Clouds sit in an annulus between these radii (m) — well outside the play area. A
 * cloud near the fall line is not atmosphere, it is an occluder between the camera and
 * the only two things the player needs to see.
 */
export const CLOUD_INNER_RADIUS = 90;
export const CLOUD_OUTER_RADIUS = 280;
/** Clouds hang between these altitudes (m) — a layer far below, not slabs by the lens. */
export const CLOUD_MIN_ALTITUDE = 45;
export const CLOUD_MAX_ALTITUDE = 105;
