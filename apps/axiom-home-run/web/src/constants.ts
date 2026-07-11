/*
 * constants.ts — every tuning number for Home Run!, in one place, imported by
 * nothing but this game. The top blocks are the gameplay contract (field geometry,
 * batter movement, the spring-loaded swing, pitch profiles, fielders, outcomes,
 * scoring, round shape). The lower blocks are the presentation layout the scene
 * builds against (camera, palette anchors). SDK-free — plain numbers only.
 *
 * World frame: home plate at the origin, +Z toward the pitcher and center field,
 * +Y up. The camera sits behind home plate at -Z, so world +X projects to
 * screen-LEFT (the same convention as the sibling heat-check app).
 */

import { type Vec3, vec3 } from "./vec.ts";

// ── fixed-step clock ──────────────────────────────────────────────────────────
export const FIXED_HZ = 60;
export const TICK_SECONDS = 1 / FIXED_HZ;

// ── field geometry (a toy square diamond, corner at home) ────────────────────
/**
 * The field is a square rotated 45°: home at (0,0), foul-line corners at
 * (±FIELD_CORNER, FIELD_CORNER), apex (dead center field) at (0, 2·FIELD_CORNER).
 * Fair territory: |x| ≤ z. The outfield walls are the two upper edges, the line
 * |x| + z = WALL_LINE.
 */
export const FIELD_CORNER = 17;
export const WALL_LINE = FIELD_CORNER * 2;
export const WALL_HEIGHT = 2.6;
/** Bases: 1B at (-BASE_CORNER, BASE_CORNER), 2B at (0, 2·BASE_CORNER), 3B mirrored. */
export const BASE_CORNER = 7.5;
/** The pitching machine's mound center. */
export const MOUND: Vec3 = vec3(0, 0, 10.2);
/** Where a pitch leaves the machine's barrel. */
export const PITCH_RELEASE: Vec3 = vec3(0, 1.12, 9.7);
/** A pitch that reaches this z behind the plate was not hit — a miss/take. */
export const CATCHER_Z = -2.2;
/** Infield radius used by outcome classification (grounders die inside it). */
export const INFIELD_RADIUS = 14;

// ── ball ──────────────────────────────────────────────────────────────────────
export const BALL_RADIUS = 0.12;
/** Gravity for a ball IN PLAY (u/s²) — arcade-light so arcs read clearly. */
export const GRAVITY = 22;
export const BOUNCE_RESTITUTION = 0.42;
export const BOUNCE_FRICTION = 0.68;
/** Per-tick horizontal decay once the ball is rolling. */
export const ROLL_DECAY = 0.965;
/** Below this horizontal speed (u/s) a rolling ball is at rest → resolve. */
export const REST_SPEED = 0.5;
export const WALL_RESTITUTION = 0.35;
/** A ball still unresolved after this many flight ticks resolves where it is. */
export const FLIGHT_TIMEOUT_TICKS = 420;

// ── batter movement (A/D repositioning inside the box) ───────────────────────
/** The batter stands on the +X side of the plate; A/D slides them within this range. */
export const BATTER_MIN_X = 0.55;
export const BATTER_MAX_X = 1.35;
export const BATTER_START_X = 0.95;
/** Lateral step speed (u/tick) — quick but grounded, ~¼ s across the full box. */
export const BATTER_STEP_SPEED = 0.055;
/** The batter's feet (and the bat pivot) stand slightly behind the plate center. */
export const BATTER_Z = -0.15;

// ── the spring-loaded swing ───────────────────────────────────────────────────
/**
 * The bat is a segment from the pivot (the batter's hands) sweeping in a mostly
 * horizontal plane. Its direction at angle θ is d(θ) = (−sin θ, 0, −cos θ):
 * θ=0 points straight back at the catcher, θ=π/2 points across the plate (−X),
 * θ=π points at the pitcher. The swing sweeps θ upward through the contact zone.
 */
export const THETA_IDLE = 0.55;
/** Fully wound: pulled back past straight-behind. */
export const THETA_LOADED = -0.5;
/** Bat perpendicular to the pitch — square contact sends the ball dead center. */
export const THETA_SWEET = Math.PI / 2;
/** The angle where the forward strike hands off to the decelerating follow-through. */
export const THETA_FOLLOW_START = 2.3;
/** The bat overshoots to here before recovering. */
export const THETA_FOLLOW_END = 3.05;
/** Load saturation rate per held tick: fast start, resisting toward full (~⅓ s to full). */
export const LOAD_RATE = 0.2;
/** Load at/above this reads as "fully loaded" (the pose stops compressing). */
export const LOAD_FULL = 0.98;
/** Swing angular velocity (rad/tick) at zero load … full load. */
export const OMEGA_MIN = 0.15;
export const OMEGA_MAX = 0.3;
/** The release "snap": ω ramps from SNAP_START·ω₀ to ω₀ over the first SNAP_TICKS. */
export const SNAP_TICKS = 2;
export const SNAP_START = 0.55;
/** Per-tick ω decay through the follow-through (recovery is slower than the strike). */
export const FOLLOW_DRAG = 0.86;
/** Follow-through ends (→ recover) when ω falls below this. */
export const FOLLOW_MIN_OMEGA = 0.02;
/** Recovery eases θ back to idle at this rate per tick — visibly slower than the strike. */
export const RECOVER_RATE = 0.055;
/** Recovery is done when θ is within this of idle. */
export const RECOVER_EPSILON = 0.02;

// ── bat geometry + contact model ─────────────────────────────────────────────
/** The hittable segment of the bat, as radii from the pivot (grip → tip). */
export const BAT_GRIP_R = 0.14;
export const BAT_TIP_R = 1.02;
/** Horizontal contact tolerance: bat thickness + ball radius + arcade grace. */
export const CONTACT_RADIUS = 0.19;
/** Vertical contact window (the bat plane's effective reach up/down). */
export const CONTACT_HEIGHT = 0.26;
/** The bat plane's height at the sweet angle… */
export const BAT_PLANE_Y = 0.85;
/** …rising through the arc (a slight uppercut): batY = plane + UPPERCUT·(θ − sweet). */
export const BAT_UPPERCUT = 0.22;
export const BAT_UPPERCUT_CLAMP = 0.18;
/** The sweet spot sits here along the bat, with a gaussian falloff of this width. */
export const SWEET_SPOT_R = 0.78;
export const SWEET_SPOT_WIDTH = 0.34;
/** Exit speed = batPointSpeed·HIT_POWER·(sweet blend) + |pitch speed|·PITCH_BOUNCE_SHARE. */
export const HIT_POWER = 2.4;
export const PITCH_BOUNCE_SHARE = 0.35;
/** Launch loft (radians) at square contact, plus gain per unit of undercut (ball above bat). */
export const LOFT_BASE = 0.34;
export const LOFT_GAIN = 2.0;
export const LOFT_MIN = -0.5;
export const LOFT_MAX = 1.15;
/** Vertical mishit: contact beyond this |dy| starts bleeding exit speed… */
export const VERT_CLEAN_DY = 0.06;
/** …reaching full penalty at CONTACT_HEIGHT; a full mishit keeps this speed share. */
export const VERT_MISHIT_KEEP = 0.4;
/** Timing quality gaussian width around the sweet angle. */
export const TIMING_WIDTH = 0.38;
/** How much of the exit speed rides on square timing (0 = timing only steers). */
export const TIMING_SPEED_SHARE = 0.22;
/** Contact substeps for the swept bat-vs-ball test (kills tunneling at max ω). */
export const CONTACT_SUBSTEPS = 8;
/** Fair territory half-angle: |spray| beyond this is a foul ball. */
export const FOUL_ANGLE = Math.PI / 4;

// ── pitch profiles ────────────────────────────────────────────────────────────
/** Difficulty tier controls when a profile can appear in the 10-pitch round. */
export type PitchTier = "easy" | "medium" | "hard";

export interface PitchProfile {
  readonly id: string;
  readonly name: string;
  /** Ball speed toward the plate, u/s. */
  readonly speed: number;
  /** Gravity during the pitch, u/s² (drop pitches fall hard; "rising" ones barely). */
  readonly gravity: number;
  /** Where it crosses the plate: lateral (+X = inside, toward the batter) and height. */
  readonly targetX: number;
  readonly targetY: number;
  readonly tier: PitchTier;
}

export const PITCH_PROFILES: readonly PitchProfile[] = [
  { gravity: 8, id: "slow-straight", name: "SLOW BALL", speed: 12.5, targetX: 0, targetY: 0.95, tier: "easy" },
  { gravity: 8, id: "medium-straight", name: "FASTBALL", speed: 17, targetX: 0, targetY: 0.95, tier: "easy" },
  { gravity: 8, id: "fast-straight", name: "HEATER", speed: 23, targetX: 0, targetY: 1.0, tier: "hard" },
  { gravity: 16, id: "slow-drop", name: "SINKER", speed: 12, targetX: 0, targetY: 0.72, tier: "medium" },
  { gravity: 3.5, id: "fast-flat", name: "RISER", speed: 24, targetX: 0, targetY: 1.1, tier: "hard" },
  { gravity: 8, id: "inside", name: "INSIDE", speed: 16.5, targetX: 0.34, targetY: 0.9, tier: "medium" },
  { gravity: 8, id: "outside", name: "OUTSIDE", speed: 16.5, targetX: -0.34, targetY: 0.9, tier: "medium" },
] as const;

/** Pitch indices below this draw only from the "easy" tier… */
export const EASY_ONLY_BEFORE = 2;
/** …and below this never draw "hard"; from here on, hard profiles are weighted up. */
export const HARD_ALLOWED_FROM = 5;
/** How much extra selection weight a hard profile carries late in the round. */
export const HARD_LATE_WEIGHT = 2;
/** Deterministic per-pitch jitter half-ranges (applied around the profile's aim). */
export const JITTER_X = 0.18;
export const JITTER_Y = 0.09;
export const JITTER_SPEED = 0.04;
/** Displayed pitch speed: mph = u/s · MPH_PER_UNIT. */
export const MPH_PER_UNIT = 3.4;

// ── round pacing ──────────────────────────────────────────────────────────────
export const PITCHES_PER_ROUND = 10;
/** Idle gap before the machine starts winding (plus a seeded 0…GAP_JITTER_TICKS). */
export const GAP_TICKS = 25;
export const GAP_JITTER_TICKS = 35;
/** The telegraphed wind-up: machine compresses, then fires at the end. */
export const WINDUP_TICKS = 48;
/** Muzzle-flash cue duration after release. */
export const FLASH_TICKS = 8;
/** How long a result message holds before the next pitch (longer for a homer). */
export const RESULT_TICKS = 85;
export const HOMER_RESULT_TICKS = 150;

// ── fielders ──────────────────────────────────────────────────────────────────
export interface FielderSpot {
  readonly name: string;
  readonly x: number;
  readonly z: number;
  /** The visible patrol circle the fielder wanders inside. */
  readonly radius: number;
}

export const FIELDER_SPOTS: readonly FielderSpot[] = [
  { name: "1B", radius: 1.7, x: -6.9, z: 7.9 },
  { name: "2B", radius: 1.7, x: -3.4, z: 11.8 },
  { name: "SS", radius: 1.7, x: 3.4, z: 11.8 },
  { name: "3B", radius: 1.7, x: 6.9, z: 7.9 },
  { name: "LF", radius: 2.4, x: 12.5, z: 17.5 },
  { name: "LC", radius: 2.4, x: 6.8, z: 22.5 },
  { name: "CF", radius: 2.4, x: 0, z: 24.5 },
  { name: "RC", radius: 2.4, x: -6.8, z: 22.5 },
  { name: "RF", radius: 2.4, x: -12.5, z: 17.5 },
  // The machine operator, feeding balls beside the mound (mostly stands there).
  { name: "OP", radius: 0.7, x: 2.2, z: 10 },
] as const;

/** Wander amplitude as a share of the patrol radius (leaves margin at the rim). */
export const WANDER_AMPLITUDE = 0.72;
/** Wander base angular frequencies (rad/tick); per-fielder values are seeded near these. */
export const WANDER_FREQ_LO = 0.011;
export const WANDER_FREQ_HI = 0.031;
/** Chase speed toward an interception point (u/tick). */
export const FIELDER_SPEED = 0.075;
/** A fielder reacts when the projected landing is within radius·REACH_MULT of home spot… */
export const FIELDER_REACH_MULT = 2.0;
/** …but may leave its circle only up to radius·CHASE_CLAMP while chasing. */
export const FIELDER_CHASE_CLAMP = 1.45;
/** Catch/field the ball inside this horizontal distance, below CATCH_HEIGHT. */
export const CATCH_RADIUS = 0.6;
export const CATCH_HEIGHT = 1.6;

// ── outcome thresholds ───────────────────────────────────────────────────────
/** Exit speeds (u/s) below this are weak contact no matter the arc. */
export const WEAK_EXIT_SPEED = 15;
/** Launch loft below this is a grounder; above POPUP_LOFT (landing short) a popup. */
export const GROUNDER_LOFT = 0.16;
export const POPUP_LOFT = 0.62;
export const POPUP_MAX_DIST = 19;

// ── scoring ───────────────────────────────────────────────────────────────────
export const SCORE_TABLE = {
  clean: 100,
  foul: 0,
  grounder: 50,
  homer: 500,
  miss: 0,
  popup: 50,
  weak: 25,
} as const;
/** Distance bonus per world unit, for clean hits and (doubled stakes) homers. */
export const CLEAN_DIST_BONUS = 1;
export const HOMER_DIST_BONUS = 2;
/** Consecutive homers multiply homer points: 1×, 2×, 3×, capped here. */
export const STREAK_MULT_CAP = 4;

// ── hit feel ──────────────────────────────────────────────────────────────────
/** Contact quality at/above this earns hit-stop + camera impact. */
export const HIT_STOP_QUALITY = 0.5;
export const HIT_STOP_BASE_TICKS = 2;
export const HIT_STOP_MAX_EXTRA = 4;
/** Camera shake magnitudes (world units) and durations (ticks). */
export const SHAKE_CONTACT = 0.09;
export const SHAKE_HOMER = 0.2;
export const SHAKE_TICKS = 14;
export const SHAKE_TICKS_HOMER = 24;

// ── camera (fixed, elevated, behind home plate) ──────────────────────────────
export const CAMERA_POS: Vec3 = vec3(0, 6.1, -6.4);
export const CAMERA_TARGET: Vec3 = vec3(0, 0.9, 12);
export const CAMERA_FOV_Y = 0.98;
/**
 * The near plane is deliberately generous: NDC depth is non-linear, and the
 * canvas2d backend's depth cues (fog toward the clear colour) key on NDC z —
 * a tiny near plane crushes the whole scene into NDC ≈ 1 and fogs everything.
 */
export const CAMERA_NEAR = 3.5;
export const CAMERA_FAR = 140;
/** Restrained camera animation: wind-up dolly, release punch, long-ball follow. */
export const CAMERA_WINDUP_DOLLY = 0.5;
export const CAMERA_RELEASE_PUNCH = 0.3;
export const CAMERA_PUNCH_TICKS = 8;
/** Ball-follow blends the camera target toward long hits, up to this share. */
export const CAMERA_FOLLOW_MAX = 0.42;
export const CAMERA_FOLLOW_RATE = 0.05;
