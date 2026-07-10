/*
 * constants.ts — every tuning number in the game, SDK-free. The single most
 * important export is `SHOT_TUNING`: ALL values affecting shot feel live in that one
 * immutable object (spec requirement) — nothing shot-related is hidden elsewhere.
 * Below it: the court geometry (hoop, rim, backboard, stations, racks), the camera,
 * and the phase timing. `gameplay.ts` / `physics.ts` / `session.ts` read from here;
 * `scene.ts` reads the same constants so the visual court and the physics court are
 * one authored truth (the rim you see IS the rim you hit).
 */

import { type Vec3, vec3 } from "./vec.ts";

// ── fixed-step clock ──────────────────────────────────────────────────────────

export const FIXED_HZ = 60;
export const DT = 1 / FIXED_HZ;

// ── the one shot-feel tuning object (spec: "SHOT_TUNING") ─────────────────────

/**
 * Every parameter of the shooting model. Times are in fixed 60 Hz ticks, speeds in
 * m/s, angles in radians, restitutions 0..1, dampings per second.
 *
 * THE SHOT IS ONE CONTINUOUS MOTION (implemented in `gameplay.ts` + `session.ts`,
 * no randomness anywhere). The moment a ball is released, the NEXT ball is dealt
 * off its rack slot into the hands (the pickup animation plays through the
 * follow-through). Holding Space then runs chest settle → shot rise; releasing
 * launches at the EXACT motion state of that instant, while earlier balls are
 * still in flight. The rise is a normalized progress `p ∈ [0,1]`; three keyframed
 * curves (early → ideal → late) give the launch its forward speed, vertical
 * speed, and a release-pitch offset:
 *
 *   vFwd(p), vUp(p), pitchOff(p)  =  smoothstep keyframe curves over p
 *   θ(p)  = atan2(vUp, vFwd) + pitchOff(p)          (the TRUE aim the reticle shows)
 *   v0    = dir(yaw, θ)·hypot(vFwd, vUp)            (yaw is the player's mouse aim)
 *
 * Early releases are low and weak, the ideal window (idealWindowStart..End) is
 * aligned and strong, late releases are hard and flat. The camera is exclusively
 * mouse-driven — the game never rotates, nudges, or drifts the view.
 */
export const SHOT_TUNING = {
  // ── motion timing (ticks) ──────────────────────────────────────────────────
  /** Automatic pickup: rack slot → chest hold, starting the moment the previous
   * ball is released (Space is ignored until the ball reaches the chest). */
  pickupTicks: 10,
  /** Chest settle at the start of the held motion, before the rise begins. */
  chestSettleTicks: 6,
  /** The shot rise — progress p sweeps 0 → 1 across these ticks. */
  shotRiseTicks: 36,
  /** Holding past the top: p stays 1 this long, then the shot auto-releases. */
  maxHoldTicks: 30,
  /** Follow-through after the ball leaves the hand. */
  followThroughTicks: 14,
  // ── release curves (early = p 0, ideal = window center, late = p 1) ────────
  /** Forward launch speed (m/s) at an early / ideal / late release. */
  earlyReleaseForwardSpeed: 3.0,
  idealReleaseForwardSpeed: 5.35,
  lateReleaseForwardSpeed: 6.8,
  /** Vertical launch speed (m/s) at an early / ideal / late release. */
  earlyReleaseVerticalSpeed: 4.4,
  idealReleaseVerticalSpeed: 7.05,
  lateReleaseVerticalSpeed: 7.6,
  /** Release-pitch offset (rad) tilting the launch at early / ideal / late. */
  earlyReleasePitchOffset: -0.12,
  idealReleasePitchOffset: 0,
  lateReleasePitchOffset: -0.08,
  /** The ideal release window, as motion progress. */
  idealWindowStart: 0.58,
  idealWindowEnd: 0.72,
  // ── pickup pose ────────────────────────────────────────────────────────────
  /** Scale of the per-slot variation in the ball's chest/entry pose (0..1).
   * Ball presentation only — the camera is NEVER moved by the game. */
  rackSlotPoseInfluence: 0.5,
  // ── aim (the camera is exclusively player-driven) ──────────────────────────
  /** Horizontal aim: radians of yaw per pixel of mouse movement. */
  aimYawSensitivity: 0.0026,
  /** Vertical look: radians of pitch per pixel of mouse movement (camera only). */
  aimPitchSensitivity: 0.0022,
  /** Touch look-drag deltas are scaled by this before the mouse sensitivities. */
  touchLookScale: 2.2,
  // ── swipe shot (mobile; the swipe-basketball gesture model) ────────────────
  /** Upward flick speed (normalized px/tick) below which a lift-off is not a shot. */
  swipeGestureDeadzone: 5,
  /** Upward flick speed (normalized px/tick) that maps to a full-rise release. */
  swipeGestureFull: 40,
  /** Launch-yaw offset (rad) at a full sideways flick — bounded lateral aim. */
  swipeLateralMaxYaw: 0.16,
  /** Soft yaw bound half-width around each station's hoop-facing base yaw (rad):
   * mouse movement deeper OUT of range is blocked, never snapped back. */
  yawClampHalf: 0.75,
  /** The camera pitch that frames the rim comfortably (rad). */
  pitchNeutral: 0.2,
  /** Camera pitch clamps (rad). */
  minPitch: -0.15,
  maxPitch: 0.6,
  // ── release geometry ───────────────────────────────────────────────────────
  /** Ball height at the TOP of the rise (a p=1 release leaves from here) (m). */
  releaseHeight: 2.05,
  /** Release point distance in front of the eye, along the aim yaw (m). */
  releaseForwardOffset: 0.35,
  // ── ball physics ───────────────────────────────────────────────────────────
  /** Multiplier on 9.8 m/s² gravity. */
  gravityScale: 1.0,
  /** Ball linear damping (fraction of velocity lost per second). */
  ballLinearDamping: 0.03,
  /** Ball angular damping (fraction of spin lost per second). */
  ballAngularDamping: 0.5,
  /** Ball↔court restitution (floor bounces). */
  ballRestitution: 0.55,
  /** Ball↔rim restitution (dead-ish iron — rim touches often still drop). */
  rimRestitution: 0.35,
  /** Ball↔backboard restitution (lively glass — hard banks bounce back out). */
  backboardRestitution: 0.85,
  /** A shot resolves as a miss after this many ticks in flight, no matter what. */
  maxShotLifetimeTicks: 300,
  /** Extra horizontal slack on the scoring-cylinder radius checks (m). */
  scoreDetectionTolerance: 0.02,
  /** Launch backspin about the camera-right axis (rad/s) — visual spin. */
  backspinRadPerSec: 12.5,
} as const;

/** Gravity (m/s²), derived from the tuning's gravityScale. */
export const GRAVITY_Y = -9.8 * SHOT_TUNING.gravityScale;


// ── ball ──────────────────────────────────────────────────────────────────────

export const BALL_RADIUS = 0.12;

// ── hoop (the visual rim constants ARE the collider constants) ───────────────

/** Rim center — the scoring cylinder's axis passes through (RIM_X, ·, RIM_Z). */
export const RIM_X = 0;
export const RIM_Y = 3.05;
export const RIM_Z = 0;
/** Rim ring radius (center of the tube). Inner diameter ≈ 2.6 ball diameters — forgiving, but missable. */
export const RIM_RADIUS = 0.34;
/** Rim tube (wire) radius. */
export const RIM_TUBE = 0.025;
/** Static collider spheres approximating the rim torus (and net-strand anchors). */
export const RIM_COLLIDER_COUNT = 16;

/** Basket-detection planes (two-stage downward-crossing detector). The lower plane
 * sits 0.15 under the rim: deep enough that an upward toss or a rim-out can never
 * fake both crossings, shallow enough that a steep diagonal swish exits through it
 * before leaving the scoring cylinder sideways. */
export const UPPER_PLANE_Y = RIM_Y + 0.25;
export const LOWER_PLANE_Y = RIM_Y - 0.15;

/** Backboard AABB (front face toward the court at z = center + half depth). */
export const BACKBOARD_CENTER: Vec3 = vec3(0, 3.55, -0.53);
export const BACKBOARD_HALF: Vec3 = vec3(0.9, 0.6, 0.025);

/** Support pole AABB behind the backboard. */
export const POLE_CENTER: Vec3 = vec3(0, 1.7, -1.1);
export const POLE_HALF: Vec3 = vec3(0.07, 1.7, 0.07);

// ── shooting stations + racks (data, not code) ────────────────────────────────

/** Distance from the rim axis to each shooting station (the three-point arc). */
export const ARC_RADIUS = 6.75;

/** Standing eye height (m). */
export const EYE_HEIGHT = 1.7;

/** One of the three fixed shooting spots, with its rack parked beside it. */
export interface ShootingStation {
  /** Where the player stands (y = 0, on the floor). */
  readonly position: Vec3;
  /** The yaw that faces the rim dead-on from this spot. */
  readonly baseYaw: number;
  /** Which side of the player the rack sits on (+1 = camera-right, −1 = left). */
  readonly rackSide: 1 | -1;
  /** HUD name. */
  readonly label: string;
}

/**
 * Forward direction of a given yaw: yaw 0 looks down −Z (toward the hoop from the
 * center station); positive yaw turns toward +X (screen-right).
 */
export const yawForward = (yaw: number): Vec3 => vec3(Math.sin(yaw), 0, -Math.cos(yaw));

/** Camera-right direction of a given yaw. */
export const yawRight = (yaw: number): Vec3 => vec3(Math.cos(yaw), 0, Math.sin(yaw));

/** Aim direction from yaw + pitch (pitch > 0 looks up). */
export const aimDirection = (yaw: number, pitch: number): Vec3 => {
  const c = Math.cos(pitch);
  return vec3(Math.sin(yaw) * c, Math.sin(pitch), -Math.cos(yaw) * c);
};

const STATION_ANGLES = [-0.7, 0, 0.7] as const;
const STATION_LABELS = ["LEFT WING", "TOP OF THE ARC", "RIGHT WING"] as const;

/** Left wing → top of the arc → right wing, each on the 6.75 m arc facing the rim. */
export const STATIONS: readonly ShootingStation[] = STATION_ANGLES.map((theta, i) => ({
  baseYaw: -theta,
  label: STATION_LABELS[i]!,
  // The wings put the rack toward the court center so it never covers the hoop.
  position: vec3(ARC_RADIUS * Math.sin(theta), 0, ARC_RADIUS * Math.cos(theta)),
  rackSide: theta < 0 ? 1 : -1,
}));

export const BALLS_PER_RACK = 5;
export const RACK_COUNT = STATIONS.length;
export const TOTAL_SHOTS = BALLS_PER_RACK * RACK_COUNT;
/** The golden bonus ball is the LAST ball of every rack (0-based slot index). */
export const GOLDEN_BALL_INDEX = BALLS_PER_RACK - 1;

/** Rack stand: offsets from the station, ball-row height, slot spacing. The rack
 * sits a step ahead and to the side — inside the first-person view next to the
 * shot, never between the shooter and the hoop. */
export const RACK_LATERAL_OFFSET = 1.15;
export const RACK_FORWARD_OFFSET = 2.4;
export const RACK_BALL_Y = 0.85;
export const RACK_SLOT_SPACING = 0.3;

/** The rack stand's center position for a station (balls sit in a row above it). */
export const rackCenter = (station: ShootingStation): Vec3 => {
  const right = yawRight(station.baseYaw);
  const fwd = yawForward(station.baseYaw);
  return vec3(
    station.position.x + right.x * station.rackSide * RACK_LATERAL_OFFSET + fwd.x * RACK_FORWARD_OFFSET,
    0,
    station.position.z + right.z * station.rackSide * RACK_LATERAL_OFFSET + fwd.z * RACK_FORWARD_OFFSET,
  );
};

/** World position of rack slot `slot` (0..4) at a station — a row along the aim direction. */
export const rackSlotPosition = (station: ShootingStation, slot: number): Vec3 => {
  const center = rackCenter(station);
  const fwd = yawForward(station.baseYaw);
  const offset = (slot - (BALLS_PER_RACK - 1) / 2) * RACK_SLOT_SPACING;
  return vec3(center.x + fwd.x * offset, RACK_BALL_Y, center.z + fwd.z * offset);
};

// ── court bounds (a ball clearly outside resolves the shot) ───────────────────

export const OUT_OF_BOUNDS_X = 12;
export const OUT_OF_BOUNDS_Z_FAR = 12;
export const OUT_OF_BOUNDS_Z_BEHIND = -3.2;

// ── camera ────────────────────────────────────────────────────────────────────

export const CAMERA_FOV_Y = (65 * Math.PI) / 180;
export const CAMERA_NEAR = 0.05;
export const CAMERA_FAR = 100;
/** The canvas aspect ratio (index.html's 960×540) — used to project the reticle. */
export const CAMERA_ASPECT = 960 / 540;

// ── phase timing (ticks) ──────────────────────────────────────────────────────

/** How long the shot-feedback (SWISH / MADE / RIM / …) holds before the next ball. */
export const FEEDBACK_TICKS = 50;
/** Rack-to-rack camera glide duration. */
export const MOVE_TICKS = 90;

// ── physics stepping ──────────────────────────────────────────────────────────

/** Integrator substeps per fixed tick (keeps per-substep travel « ball radius). */
export const PHYSICS_SUBSTEPS = 4;

// ── swipe gesture plumbing (the swipe-basketball reference values) ────────────

/** Retained pointer samples (fixed ring-buffer capacity). */
export const POINTER_HISTORY = 12;
/** A per-sample jump beyond this (px) is a focus/tab glitch — history clears. */
export const MAX_POINTER_DELTA = 400;
/** Release velocity averages the per-pair velocities of this many samples. */
export const SWIPE_SAMPLE_WINDOW = 5;
/** Gesture velocities are normalized to this canvas height, so a flick feels the
 * same on a phone and a desktop regardless of the displayed canvas size. */
export const GESTURE_REFERENCE_HEIGHT = 540;
/** Touch-downs in this canvas-fraction zone (around the held ball, lower-center)
 * begin a SHOT gesture; anywhere else the drag is a LOOK. */
export const SWIPE_ZONE_MIN_Y = 0.5;
export const SWIPE_ZONE_HALF_X = 0.35;

// ── polish ────────────────────────────────────────────────────────────────────

/** Golden-ball trail pool size. */
export const TRAIL_POOL = 6;
/** Ticks between golden-trail samples. */
export const TRAIL_SAMPLE_TICKS = 2;

// ── development trajectory preview ────────────────────────────────────────────

/**
 * When true, charging renders a dotted predicted trajectory computed by the SAME
 * integrator + tuning as the real shot (no separate approximation). Tuning aid —
 * MUST ship false.
 */
export const DEBUG_TRAJECTORY = false;
/** Preview dot count and tick stride between dots. */
export const PREVIEW_POINTS = 20;
export const PREVIEW_STRIDE_TICKS = 3;
