/*
 * constants.ts — every tuning number for Heat Check, in one place, imported by
 * nothing but this game. The top block is the gameplay contract the brief names
 * verbatim (round length, court bounds, movement, defender, shot-quality thresholds,
 * heat + streak, timing). The lower blocks are the 3D world layout the scene builds
 * against (court, hoop, camera, lights). SDK-free — plain numbers only.
 */

import { type Vec2, type Vec3, vec2, vec3 } from "./vec.ts";

// ── fixed-step clock ──────────────────────────────────────────────────────────
export const FIXED_HZ = 60;
export const ROUND_SECONDS = 60;
export const ROUND_TICKS = ROUND_SECONDS * FIXED_HZ;

// ── court + player movement ───────────────────────────────────────────────────
/** Lateral bounds the player (and defender) are clamped to, in world units. */
export const COURT_MIN_X = -6;
export const COURT_MAX_X = 6;
/** Max lateral speed the player body can carry (world units / tick). */
export const PLAYER_MOVE_SPEED = 0.26;
/** How fast velocity tracks the stick's target speed (0..1 / tick) — higher = tighter, less woosh. */
export const PLAYER_ACCELERATION = 0.3;
/** How fast velocity bleeds to rest when the stick recenters (0..1 / tick) — higher = crisper stop. */
export const PLAYER_FRICTION = 0.45;

// ── floating dribble-stick control ────────────────────────────────────────────
/** Thumb displacement (displayed px) from the anchor for a full-tilt stick. */
export const STICK_RADIUS = 90;
/** |stickX| below this reads as no movement intent (a still thumb). */
export const STICK_DEADZONE = 0.12;
/** Holds shorter than this are taps, not shots — they never fire. */
export const MIN_SHOOT_HOLD_MS = 120;
/** MIN_SHOOT_HOLD_MS in fixed ticks (the deterministic core counts ticks). */
export const MIN_SHOOT_HOLD_TICKS = Math.round((MIN_SHOOT_HOLD_MS / 1000) * FIXED_HZ);

// ── defender ──────────────────────────────────────────────────────────────────
/** Base closing speed toward the (delayed) player position (world units / tick). */
export const DEFENDER_BASE_SPEED = 0.092;
/** Extra closing speed per heat level — the defender gets hungrier as you heat up. */
export const DEFENDER_HEAT_SPEED_BONUS = 0.014;
/** How many ticks the defender's target lags the player's real position. */
export const DEFENDER_REACTION_DELAY = 12;
/** Balance regained per tick when not being crossed over (toward 1 = fully balanced). */
export const DEFENDER_BALANCE_RECOVERY = 0.02;
/** Balance an off-balance defender snaps down to when beaten by a sharp cut. */
export const DEFENDER_BEATEN_BALANCE = 0.18;

// ── separation / crossover ────────────────────────────────────────────────────
/** Min |player lateral speed| for a direction change to count as a sharp crossover. */
export const CROSSOVER_SPEED_THRESHOLD = 0.15;
/** Stick swing magnitude (|stickX - prevStickX|) that counts as a hard crossover. */
export const CROSSOVER_REVERSAL_THRESHOLD = 0.9;
/** Lateral separation (world units) that yields a full separation score. */
export const SPACE_REQUIRED_FOR_CLEAN_SHOT = 3;
/** Separation at/above which a shot is scored as "deep" (worth +1). */
export const DEEP_SHOT_SEPARATION = 4.2;
/** Body stability a deep/"clean" shot needs — a fast, off-balance release isn't clean. */
export const STABILITY_REQUIRED_FOR_CLEAN_SHOT = 0.6;

// ── advantage window (beat your man, then shoot before it closes) ──────────────
// Advantage is a TRANSIENT edge you earn by genuinely beating the defender. It decays
// fast, decays faster as the defender recovers, and can't be farmed by wiggling
// (dribble fatigue). Creating an advantage and releasing IN THE WINDOW is the loop.
/** Advantage gained from one clean crossover (before fatigue / repeat penalties). */
export const ADVANTAGE_GAIN_CROSSOVER = 0.7;
/** Base advantage lost per second. */
export const ADVANTAGE_DECAY_PER_SECOND = 1.1;
/** Extra advantage lost per second, scaled by how recovered (balanced) the defender is. */
export const ADVANTAGE_DEFENDER_RECOVERY_DECAY = 0.7;
/** Multiplier on advantage gain for a REPEAT reversal (spam) vs a fresh, committed move. */
export const ADVANTAGE_REPEAT_MOVE_PENALTY = 0.3;
/** Advantage at/above which the defender is truly beaten (BROKEN ankles). */
export const ADVANTAGE_WINDOW_STRONG_THRESHOLD = 0.55;
/** Advantage at/above which a real window is open. */
export const ADVANTAGE_WINDOW_WEAK_THRESHOLD = 0.22;

// ── dribble fatigue (anti-spam) ───────────────────────────────────────────────
/** Fatigue added by a reversal that comes too soon after the last one (wiggle spam). */
export const DRIBBLE_FATIGUE_GAIN = 0.33;
/** Fatigue shed per second while committing to a direction (not reversing). */
export const DRIBBLE_FATIGUE_DECAY = 0.8;
/** Reversals within this many ticks of each other count as spam (fatigue). */
export const REPEATED_REVERSAL_WINDOW = 28;
/** Max stability lost at full fatigue (a gassed handle is shaky). */
export const FATIGUE_STABILITY_PENALTY = 0.3;

// ── shot quality thresholds ───────────────────────────────────────────────────
export const SHOT_REQUIRED_QUALITY = 0.55;
export const SHOT_SWISH_QUALITY = 0.82;
/** Required quality creeps up this much per heat level (defender pressure). */
export const REQUIRED_QUALITY_HEAT_STEP = 0.012;

// ── shot-quality sub-weights ──────────────────────────────────────────────────
// Advantage + separation (SPACE you created) dominate at 45%; timing 25% can UPGRADE a
// good look but never rescues a contested one; stability 15%, shot-selection 10%, heat
// a small 5% bonus. Pressure is a separate PENALTY subtracted for a smothering defender.
export const ADVANTAGE_WEIGHT = 0.23;
export const SEPARATION_WEIGHT = 0.22;
export const TIMING_WEIGHT = 0.25;
export const STABILITY_WEIGHT = 0.15;
export const SELECTION_WEIGHT = 0.1;
/** Max quality bonus contributed at HEAT_MAX (small — heat never fixes a bad decision). */
export const HEAT_BONUS_WEIGHT = 0.05;
/** Max quality PENALTY from a defender smothering the shot. */
export const PRESSURE_PENALTY_WEIGHT = 0.2;
/** Defender contest zone radius (world units): inside it, the pressure penalty ramps. */
export const CONTEST_RADIUS = 2.4;
/** Extra contest layered on during the final-seconds double-points window. */
export const FINAL_CONTEST = 0.12;
/** Lateral speed (units/tick) at which the stability penalty saturates. */
export const STABILITY_SPEED_REF = 0.26;
/** Stability regained per plant tick (a brief settle before the shot). */
export const STABILITY_PLANT_BONUS = 0.03;
/** Readiness band below the make bar: RISKY within this margin of it, else BAD. */
export const RISKY_MARGIN = 0.12;
/** A miss this far under the required bar reads as FORCED (garbage), not one reason. */
export const FORCED_MARGIN = 0.2;

// ── readiness indicator thresholds (SPACE / RHYTHM / BALANCE tags) ─────────────
export const TIMING_PERFECT = 0.85;
export const TIMING_GOOD = 0.5;
export const OPEN_PRESSURE_MAX = 0.28;
export const SMOTHERED_PRESSURE_MIN = 0.62;
export const BALANCE_SET_STABILITY = 0.65;
export const BALANCE_PLANTED_STABILITY = 0.85;
export const BALANCE_PLANTED_TICKS = 8;

// ── heat + streak ─────────────────────────────────────────────────────────────
export const HEAT_MAX = 5;
export const MAKE_HEAT_GAIN = 1;
export const SWISH_HEAT_GAIN = 2;
/** Heat lost on an ordinary miss. */
export const MISS_HEAT_DROP = 2;
/** A miss this far below the required bar is "severe" and resets heat toward 0. */
export const SEVERE_MISS_MARGIN = 0.18;
export const STREAK_MULTIPLIER_STEP = 3;
export const STREAK_MULTIPLIER_CAP = 4;
/** Point values before multiplier / doubling. */
export const MAKE_POINTS = 2;
export const SWISH_POINTS = 3;
export const DEEP_BONUS_POINTS = 1;

// ── timing rhythm ─────────────────────────────────────────────────────────────
/** Ticks for one full pass of the shot-rhythm meter (bad → good → perfect → bad). */
export const RHYTHM_PERIOD_TICKS = 84;
/** Half-width (in phase units, 0..1) of the centered "perfect" release window. */
export const RHYTHM_PERFECT_HALF = 0.11;

// ── round pacing ──────────────────────────────────────────────────────────────
export const FINAL_SECONDS_DOUBLE_POINTS = 10;
/** Ticks the ball spends in flight from release to rim. */
export const SHOT_ARC_DURATION = 40;
/** Ticks of made/missed feedback hold before returning to live play. */
export const SCORED_FEEDBACK_TICKS = 22;
/** Max buffered feedback events (bounded — oldest dropped past this). */
export const FEEDBACK_MAX = 6;

// ── ball dribble ──────────────────────────────────────────────────────────────
/** Peak height of the auto-dribble bounce (world units). */
export const BALL_DRIBBLE_HEIGHT = 0.9;
/** Ticks for one dribble bounce. */
export const DRIBBLE_PERIOD_TICKS = 26;
export const BALL_RADIUS = 0.16;

// ── 3D world layout (the scene builds against these) ──────────────────────────
export const GROUND_Y = 0;
/** Depth (Z, downcourt) the player, defender, and hoop sit at. */
export const PLAYER_Z = 0;
export const DEFENDER_Z = 5;
export const HOOP_Z = 13.5;
/** Rim + backboard heights. */
export const HOOP_Y = 3.05;
export const RIM_RADIUS = 0.34;
export const RIM_TUBE = 0.045;
export const RIM_SEGMENTS = 16;
export const BACKBOARD_Y = 3.55;
export const BACKBOARD_HALF_W = 0.9;
export const BACKBOARD_HALF_H = 0.55;
export const BACKBOARD_HALF_D = 0.04;
/** Where the ball leaves the player's hands on a shot. */
export const RELEASE_Y = 2.55;
/** Body heights for the symbolic figures. */
export const PLAYER_HEIGHT = 1.9;
export const DEFENDER_HEIGHT = 1.95;

// ── camera + pointer viewport ─────────────────────────────────────────────────
/** Fixed, slightly-elevated camera behind the baseline looking downcourt. */
export const CAMERA_POS: Vec3 = vec3(0, 4.2, -6.4);
export const CAMERA_TARGET: Vec3 = vec3(0, 2.35, 7.8);
export const CAMERA_FOV_Y = 0.92; // ~52°
export const CAMERA_NEAR = 0.1;
export const CAMERA_FAR = 60;
/** Extra additive camera shake magnitude on a made basket. */
export const CAMERA_SHAKE = 0.06;
/** Backing-store canvas size; the harness overrides with the displayed size. */
export const DEFAULT_VIEWPORT: Vec2 = vec2(760, 600);
