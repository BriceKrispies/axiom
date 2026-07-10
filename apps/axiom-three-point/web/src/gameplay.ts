/*
 * gameplay.ts — the pure, fully-deterministic rules of the shootout. Every function
 * here is a small referentially-transparent rule: charge → power, aim + power →
 * launch velocity (NO random spread — identical inputs give identical trajectories),
 * the two-plane basket detector, the streak scoring formula, and outcome
 * classification. `session.ts` folds these into the phase machine; the test file
 * hammers them directly.
 */

import { type Vec2, type Vec3, clamp, lerp, scale, smoothstep, vec3 } from "./vec.ts";
import type { ShotOutcome, SwipeGesture } from "./types.ts";
import {
  type ShootingStation,
  BALLS_PER_RACK,
  EYE_HEIGHT,
  LOWER_PLANE_Y,
  OUT_OF_BOUNDS_X,
  OUT_OF_BOUNDS_Z_BEHIND,
  OUT_OF_BOUNDS_Z_FAR,
  RIM_RADIUS,
  SHOT_TUNING,
  UPPER_PLANE_Y,
  yawForward,
  yawRight,
} from "./constants.ts";

// ── keyframed motion curves ───────────────────────────────────────────────────

/** A `(progress, value)` keyframe list, progress-sorted. */
export type CurveKeys = readonly (readonly [number, number])[];

/**
 * Piecewise smoothstep interpolation of `(progress, value)` keyframes. Holds the
 * first value before the first key and the last value after the last key — the
 * one scalar channel every motion curve in the shot is built from.
 */
export const sampleCurve = (keys: CurveKeys, t: number): number => {
  const first = keys[0]!;
  if (t <= first[0]) return first[1];
  const last = keys[keys.length - 1]!;
  if (t >= last[0]) return last[1];
  for (let i = 0; i < keys.length - 1; i += 1) {
    const [ta, va] = keys[i]!;
    const [tb, vb] = keys[i + 1]!;
    if (t >= ta && t <= tb) {
      return va + (vb - va) * smoothstep((t - ta) / (tb - ta));
    }
  }
  return last[1];
};

/** The center of the ideal release window (the curves' middle keyframe). */
export const IDEAL_PROGRESS = (SHOT_TUNING.idealWindowStart + SHOT_TUNING.idealWindowEnd) / 2;

/** Forward launch speed over motion progress: early → ideal → late. */
export const FORWARD_SPEED_CURVE: CurveKeys = [
  [0, SHOT_TUNING.earlyReleaseForwardSpeed],
  [IDEAL_PROGRESS, SHOT_TUNING.idealReleaseForwardSpeed],
  [1, SHOT_TUNING.lateReleaseForwardSpeed],
];

/** Vertical launch speed over motion progress. */
export const VERTICAL_SPEED_CURVE: CurveKeys = [
  [0, SHOT_TUNING.earlyReleaseVerticalSpeed],
  [IDEAL_PROGRESS, SHOT_TUNING.idealReleaseVerticalSpeed],
  [1, SHOT_TUNING.lateReleaseVerticalSpeed],
];

/** Release-pitch offset over motion progress (tilts the launch vector). */
export const RELEASE_PITCH_CURVE: CurveKeys = [
  [0, SHOT_TUNING.earlyReleasePitchOffset],
  [IDEAL_PROGRESS, SHOT_TUNING.idealReleasePitchOffset],
  [1, SHOT_TUNING.lateReleasePitchOffset],
];

// ── the launch (a pure function of yaw + motion progress; NO spread) ──────────

export interface ReleaseParams {
  /** The effective release pitch — the TRUE aim direction the reticle renders. */
  readonly aimPitch: number;
  readonly speed: number;
}

/** The release parameters at motion progress `p ∈ [0,1]`. */
export const releaseParams = (p: number): ReleaseParams => {
  const q = clamp(p, 0, 1);
  const vFwd = sampleCurve(FORWARD_SPEED_CURVE, q);
  const vUp = sampleCurve(VERTICAL_SPEED_CURVE, q);
  return {
    aimPitch: Math.atan2(vUp, vFwd) + sampleCurve(RELEASE_PITCH_CURVE, q),
    speed: Math.hypot(vFwd, vUp),
  };
};

export interface Launch {
  readonly velocity: Vec3;
  /** Backspin about the camera-right axis. */
  readonly angularVelocity: Vec3;
}

/**
 * The launch at aim `yaw` and motion progress `p`: the curves give the speed and
 * the effective release pitch; yaw gives the horizontal direction. Identical
 * `(yaw, p)` always produce the identical trajectory.
 */
export const launchVelocity = (yaw: number, p: number): Launch => {
  const { aimPitch, speed } = releaseParams(p);
  const fwd = yawForward(yaw);
  const horizontal = Math.cos(aimPitch) * speed;
  return {
    angularVelocity: scale(yawRight(yaw), -SHOT_TUNING.backspinRadPerSec),
    velocity: vec3(fwd.x * horizontal, Math.sin(aimPitch) * speed, fwd.z * horizontal),
  };
};

// ── the shot motion, all deterministic ────────────────────────────────────────
//
// The pickup is NOT part of the held motion: the next ball is dealt into the
// hands automatically the moment the previous one is released (the pickup
// animation plays through the follow-through). Holding Space therefore starts at
// the chest: a short settle, then the rise sweeps progress 0 → 1.

/** Where the shot-motion ticks fall inside the held motion. */
export const RISE_START_TICKS = SHOT_TUNING.chestSettleTicks;
export const MOTION_END_TICKS = RISE_START_TICKS + SHOT_TUNING.shotRiseTicks;
export const AUTO_RELEASE_TICKS = MOTION_END_TICKS + SHOT_TUNING.maxHoldTicks;

/** Motion progress p from ticks held: 0 through the chest settle, 0→1 over the rise. */
export const motionProgress = (motionTicks: number): number =>
  clamp((motionTicks - RISE_START_TICKS) / SHOT_TUNING.shotRiseTicks, 0, 1);

/** The chest hold anchor (lower-center of view), with the slot's small signature. */
export const chestAnchor = (station: ShootingStation, yaw: number, slot: number): Vec3 => {
  const fwd = yawForward(yaw);
  const right = yawRight(yaw);
  const slotLean = (slot - (BALLS_PER_RACK - 1) / 2) * 0.045 * SHOT_TUNING.rackSlotPoseInfluence;
  return vec3(
    station.position.x + fwd.x * 0.5 + right.x * (0.1 + slotLean),
    EYE_HEIGHT - 0.42 + slot * 0.012 * SHOT_TUNING.rackSlotPoseInfluence,
    station.position.z + fwd.z * 0.5 + right.z * (0.1 + slotLean),
  );
};

/** The top of the rise: a p=1 release leaves from here. */
export const riseTop = (station: ShootingStation, yaw: number): Vec3 => {
  const fwd = yawForward(yaw);
  return vec3(
    station.position.x + fwd.x * SHOT_TUNING.releaseForwardOffset,
    SHOT_TUNING.releaseHeight,
    station.position.z + fwd.z * SHOT_TUNING.releaseForwardOffset,
  );
};

/** The ball's hand position at rise progress `p` (chest → top). The release
 * position of a shot let go at `p` is exactly this point. */
export const risePosition = (station: ShootingStation, yaw: number, slot: number, p: number): Vec3 =>
  lerp(chestAnchor(station, yaw, slot), riseTop(station, yaw), smoothstep(clamp(p, 0, 1)));

// ── the swipe shot (mobile; the swipe-basketball gesture model) ───────────────

/**
 * Decompose a smoothed swipe velocity (normalized px/tick, screen-down +y — see
 * `pointer.ts`) into a shot, following the swipe-basketball reference: the
 * UPWARD flick strength maps through a deadzone→full band to the release
 * progress (raw screen-Y is never copied into the launch), and the sideways
 * flick becomes a BOUNDED launch-yaw offset. A lift-off under the deadzone is
 * not a shot (`null`). Deterministic — identical swipes launch identical shots.
 */
export const swipeIntents = (velocity: Vec2): SwipeGesture | null => {
  const upward = Math.max(0, -velocity.y);
  if (upward <= SHOT_TUNING.swipeGestureDeadzone) return null;
  const progress = clamp(
    (upward - SHOT_TUNING.swipeGestureDeadzone) / (SHOT_TUNING.swipeGestureFull - SHOT_TUNING.swipeGestureDeadzone),
    0,
    1,
  );
  const lateral = clamp(velocity.x / SHOT_TUNING.swipeGestureFull, -1, 1);
  return { progress, yawOffset: lateral * SHOT_TUNING.swipeLateralMaxYaw };
};

/**
 * Move an angle by `delta` inside a soft bound `[center − half, center + half]`.
 * From inside the range, outward movement stops exactly at the edge; from
 * outside the range (the bound's center changed under the player, e.g. a new
 * station), only inward movement passes — outward is blocked. The bound can
 * therefore never snap or rotate the view on its own — the camera is
 * exclusively mouse-driven.
 */
export const softBoundedTurn = (value: number, delta: number, center: number, half: number): number => {
  const next = value + delta;
  const outNow = Math.abs(value - center);
  if (outNow <= half) return clamp(next, center - half, center + half);
  return Math.abs(next - center) < outNow ? next : value;
};

// ── two-plane basket detection ────────────────────────────────────────────────

export interface DetectionState {
  /** The ball crossed the upper plane downward inside the scoring cylinder. */
  readonly enteredFromAbove: boolean;
  /** The basket has been awarded (never clears — no double scoring). */
  readonly scored: boolean;
}

export const INITIAL_DETECTION: DetectionState = { enteredFromAbove: false, scored: false };

export interface DetectionSample {
  readonly prevY: number;
  readonly y: number;
  readonly velY: number;
  readonly horizDistSq: number;
}

export interface DetectionStep {
  readonly state: DetectionState;
  /** True exactly on the substep the basket is awarded. */
  readonly scoredNow: boolean;
}

/**
 * Advance the detector one physics substep. A basket requires a downward crossing
 * of the upper plane inside the scoring cylinder, then a downward crossing of the
 * lower plane, both with downward vertical velocity. Any upward crossing of either
 * plane clears the entry record (a ball tossed up through the hoop never scores);
 * `scored` latches so one ball can never score twice.
 */
export const stepDetection = (state: DetectionState, sample: DetectionSample): DetectionStep => {
  const radius = RIM_RADIUS + SHOT_TUNING.scoreDetectionTolerance;
  const inCylinder = sample.horizDistSq <= radius * radius;
  const crossedDown = (plane: number): boolean => sample.prevY >= plane && sample.y < plane && sample.velY < 0;
  const crossedUp = (plane: number): boolean => sample.prevY <= plane && sample.y > plane;

  if (crossedUp(UPPER_PLANE_Y) || crossedUp(LOWER_PLANE_Y)) {
    return { scoredNow: false, state: { enteredFromAbove: false, scored: state.scored } };
  }
  if (crossedDown(UPPER_PLANE_Y) && inCylinder) {
    return { scoredNow: false, state: { enteredFromAbove: true, scored: state.scored } };
  }
  if (crossedDown(LOWER_PLANE_Y) && inCylinder && state.enteredFromAbove && !state.scored) {
    return { scoredNow: true, state: { enteredFromAbove: false, scored: true } };
  }
  return { scoredNow: false, state };
};

// ── scoring ───────────────────────────────────────────────────────────────────

/**
 * Points for a made shot, from the streak BEFORE this make (spec-exact):
 * `pointsAwarded = 3 + 3 * currentStreak`. The golden ball follows the same
 * formula — it is visually special, never a multiplier.
 */
export const pointsForMake = (streakBefore: number): number => 3 + 3 * streakBefore;

/** SWISH scores clean; MADE scores off iron/glass; misses read the loudest touch. */
export const classifyOutcome = (scored: boolean, touchedRim: boolean, touchedBackboard: boolean): ShotOutcome => {
  if (scored) return touchedRim || touchedBackboard ? "made" : "swish";
  if (touchedRim) return "rim";
  if (touchedBackboard) return "backboard";
  return "miss";
};

export const outcomeText = (outcome: ShotOutcome): string =>
  ({ backboard: "BACKBOARD", made: "MADE", miss: "MISS", rim: "RIM", swish: "SWISH" })[outcome];

/** The results-screen performance label (spec-exact bands). */
export const performanceLabel = (makes: number): string => {
  if (makes <= 4) return "WARMING UP";
  if (makes <= 8) return "SHARPSHOOTER";
  if (makes <= 12) return "ON FIRE";
  return "UNSTOPPABLE";
};

// ── court bounds ──────────────────────────────────────────────────────────────

/** True when the ball is clearly outside the playable court (resolves the shot). */
export const outOfBounds = (pos: Vec3): boolean =>
  Math.abs(pos.x) > OUT_OF_BOUNDS_X || pos.z > OUT_OF_BOUNDS_Z_FAR || pos.z < OUT_OF_BOUNDS_Z_BEHIND;
