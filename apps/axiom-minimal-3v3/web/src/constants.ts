/*
 * constants.ts — every tuning number in Minimal 3v3 Basketball, in one SDK-free
 * place. Units are court meters and 60 Hz ticks (the SDK fixed step); the court is
 * x = left/right, z = depth toward the hoop, y = up. The hoop sits at +z.
 */

import { type Vec3, vec3 } from "./vec.ts";

export const FIXED_HZ = 60;

// ── court + hoop ──────────────────────────────────────────────────────────────

export const HOOP_Z = 12;
export const HOOP_Y = 3.05;
export const HOOP_POS: Vec3 = vec3(0, HOOP_Y, HOOP_Z);
export const RIM_RADIUS = 0.28;
export const RIM_TUBE = 0.025;
export const RIM_SEGMENTS = 12;
export const BACKBOARD_Y = 3.5;
export const BACKBOARD_HALF_W = 0.9;
export const BACKBOARD_HALF_H = 0.55;
export const BACKBOARD_HALF_D = 0.04;

/** Player-position clamps — slightly inside the painted boundary. */
export const BOUND_X = 6.4;
export const BOUND_Z_MIN = 1.0;
export const BOUND_Z_MAX = 11.2;

/** Painted markings (visual only). */
export const THREE_PT_RADIUS = 6.4;
export const KEY_HALF_W = 1.8;
export const KEY_LENGTH = 5.8;

// ── movement ──────────────────────────────────────────────────────────────────

export const PLAYER_SPEED = 0.085;
/** Velocity-lerp rate toward the target velocity (responsiveness). */
export const PLAYER_ACCEL = 0.25;
export const TEAMMATE_SPEED = 0.045;
export const TEAMMATE_ACCEL = 0.1;
export const DEFENDER_SPEED = 0.075;
/** Defender velocity-lerp rate — the anti-jitter. Never snap defender positions. */
export const DEFENDER_SMOOTHING = 0.12;
export const DEFENDER_SEPARATION = 0.9;
export const DEFENDER_SEPARATION_PUSH = 0.03;

// ── jump shot (ticks measured from the Space press) ───────────────────────────

export const JUMP_APEX_TICK = 18;
export const JUMP_TOTAL_TICKS = 36;
export const JUMP_HEIGHT = 0.55;
/** Holding past this forces the release (well after the apex → bad timing). */
export const AUTO_RELEASE_TICK = 30;
/** `allowedWindow` in the timing-score formula, in ticks of |release − apex|. */
export const TIMING_WINDOW = 9;
/** HUD timing buckets: |err| ≤ PERFECT → "perfect", ≤ GOOD → "good", else early/late. */
export const PERFECT_ERR = 2;
export const GOOD_ERR = 5;

// ── shot chance ───────────────────────────────────────────────────────────────

export const MAX_USEFUL_DISTANCE = 9;
export const SHOT_BASE = 0.2;
export const SHOT_TIMING_WEIGHT = 0.55;
export const SHOT_DIST_WEIGHT = 0.25;
export const CHANCE_MIN = 0.05;
export const CHANCE_MAX = 0.9;
/** A defender inside this xz radius of the shooter contests the shot. */
export const CONTEST_RADIUS = 1.8;
export const CONTEST_JUMPING_PENALTY_MIN = 0.2;
export const CONTEST_JUMPING_PENALTY_MAX = 0.35;
export const CONTEST_STANDING_PENALTY_MIN = 0.1;
export const CONTEST_STANDING_PENALTY_MAX = 0.2;

// ── ball flight ───────────────────────────────────────────────────────────────

export const BALL_RADIUS = 0.16;
/** Shot flight time scales with distance: round(dist × 5), clamped to this range. */
export const SHOT_FLIGHT_MIN = 30;
export const SHOT_FLIGHT_MAX = 60;
/** Canned post-arc segment (drop through net / rim-out bounce) inside the result freeze. */
export const RIM_SETTLE_TICKS = 18;

// ── pass / steal / interception ───────────────────────────────────────────────

export const PASS_TICKS = 20;
export const PASS_ARC_HEIGHT = 0.9;
/** Chest height the pass arc targets on the receiver. */
export const PASS_CATCH_Y = 1.3;
export const INTERCEPT_RADIUS = 0.55;
/** A pass higher than this sails over defenders' hands — no interception. */
export const INTERCEPT_MAX_BALL_Y = 2.0;
/** Defender touching the handler this close during the gather = steal. */
export const STEAL_RADIUS = 0.5;

// ── defender AI ───────────────────────────────────────────────────────────────

/** The primary defender stands this far from the handler, toward the hoop. */
export const PRIMARY_GAP = 1.1;
/** Help defenders sit this fraction of the way from their assignment to the hoop. */
export const HELP_FRACTION = 0.4;
/** Contest jumps only start when the handler is within this xz radius. */
export const CONTEST_TRIGGER_RADIUS = 2.2;
export const DEF_JUMP_TICKS = 24;
export const DEF_JUMP_APEX = 12;
export const DEF_JUMP_HEIGHT = 0.5;
/** Forward lunge per tick toward the shooter, applied only while rising. */
export const DEF_LUNGE_SPEED = 0.04;
/** Contest cooldown in ticks; the per-defender stagger keeps jumps deterministic but unsynchronized. */
export const DEF_JUMP_COOLDOWN_BASE = 120;
export const DEF_JUMP_COOLDOWN_STAGGER = 17;
/** A defender above this jump height counts as "jumping" for the contest penalty. */
export const DEF_JUMPING_MIN_Y = 0.1;

// ── teammate AI ───────────────────────────────────────────────────────────────

/** Wings drift away from a defender inside this radius, by this offset. */
export const TEAMMATE_CROWD_RADIUS = 1.5;
export const TEAMMATE_DRIFT = 0.8;

// ── flow / reset ──────────────────────────────────────────────────────────────

/** The made / miss / turnover freeze before the next possession (0.8 s). */
export const RESULT_TICKS = 48;
export const RESET_HANDLER: Vec3 = vec3(0, 0, 4);
/** World +x renders screen-LEFT (camera looks downcourt), so the LEFT wing is +x. */
export const RESET_WING_LEFT: Vec3 = vec3(4.2, 0, 7);
export const RESET_WING_RIGHT: Vec3 = vec3(-4.2, 0, 7);

// ── camera ────────────────────────────────────────────────────────────────────

export const CAM_BACK = 5.2;
export const CAM_HEIGHT = 3.2;
export const CAM_AIM_AHEAD = 4;
export const CAM_AIM_Y = 1.2;
export const CAM_LERP = 0.12;
/** Snap instead of lerp when the desired pose jumps farther than this (resets). */
export const CAM_SNAP_DIST = 6;
export const CAM_FOV_Y = 0.9;
export const CAM_NEAR = 0.1;
export const CAM_FAR = 80;

// ── minimal animation ─────────────────────────────────────────────────────────

export const DRIBBLE_PERIOD = 22;
export const DRIBBLE_HEIGHT = 0.6;
export const BOB_PERIOD = 90;
export const BOB_AMPL = 0.03;
/** Max body lean (radians) at full speed. */
export const LEAN_MAX = 0.18;
/** Gather crouch ramps in over this many ticks after the Space press. */
export const GATHER_CROUCH_TICKS = 8;
