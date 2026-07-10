/*
 * gameplay.ts — the pure, unit-tested rules of Minimal 3v3 Basketball: the
 * deterministic shot-chance formula, the seeded roll, timing classification, flight
 * arcs, turnover predicates, and the AI target-position math. Every function here is
 * SDK-free and side-effect-free; `session.ts` composes them into the state machine.
 */

import { type Vec3, add, clamp, distXZ, lerp, mix, normalizeXZ, scale, vec3 } from "./vec.ts";
import type { ShotArc, TimingTag } from "./types.ts";
import * as C from "./constants.ts";

// ── deterministic randomness ──────────────────────────────────────────────────

/**
 * The ONLY randomness in the game: an FNV-style integer hash of the given fields,
 * finalized to a unit float in [0, 1). Same fields → same roll, forever. Used for
 * the shot make roll and the side-miss variant; `Math.random` appears nowhere.
 */
export const hashUnit = (...fields: readonly number[]): number => {
  let h = 2166136261;
  for (const f of fields) {
    h = Math.imul(h ^ (f | 0), 16777619) >>> 0;
    h ^= h >>> 13;
    h = Math.imul(h, 2654435761) >>> 0;
  }
  h ^= h >>> 16;
  return (h >>> 0) / 2 ** 32;
};

// ── shot chance ───────────────────────────────────────────────────────────────

const clamp01 = (v: number): number => clamp(v, 0, 1);

/** 1 at a perfect-apex release, fading to 0 at `TIMING_WINDOW` ticks of error. */
export const timingScore = (errTicks: number): number => clamp01(1 - Math.abs(errTicks) / C.TIMING_WINDOW);

/** The HUD timing tag from the SIGNED release error (releaseTick − apexTick). */
export const classifyTiming = (signedErr: number): TimingTag => {
  const err = Math.abs(signedErr);
  if (err <= C.PERFECT_ERR) {
    return "perfect";
  }
  if (err <= C.GOOD_ERR) {
    return "good";
  }
  return signedErr < 0 ? "early" : "late";
};

/** What the contest penalty needs to know about one defender. */
export interface DefenderThreat {
  readonly pos: Vec3;
  readonly jumping: boolean;
}

/**
 * The nearest defender inside `CONTEST_RADIUS` lowers the shot: a jumping defender
 * costs 0.20–0.35, a standing one 0.10–0.20 (both scaling with closeness). Nobody
 * close costs nothing.
 */
export const contestPenalty = (shooterPos: Vec3, defenders: readonly DefenderThreat[]): number => {
  let best = 0;
  for (const d of defenders) {
    const dist = distXZ(shooterPos, d.pos);
    if (dist >= C.CONTEST_RADIUS) {
      continue;
    }
    const closeness = 1 - dist / C.CONTEST_RADIUS;
    const penalty = d.jumping
      ? mix(C.CONTEST_JUMPING_PENALTY_MIN, C.CONTEST_JUMPING_PENALTY_MAX, closeness)
      : mix(C.CONTEST_STANDING_PENALTY_MIN, C.CONTEST_STANDING_PENALTY_MAX, closeness);
    best = Math.max(best, penalty);
  }
  return best;
};

export interface ShotChance {
  readonly chance: number;
  /** Signed release error in ticks (negative = early). */
  readonly signedErr: number;
  readonly tag: TimingTag;
  readonly distance: number;
}

/**
 * The deterministic shot formula, exactly as specified:
 *   base = 0.20 + timingScore·0.55 − distancePenalty·0.25 − contestPenalty
 * clamped to [0.05, 0.90] — a PERFECT release strongly helps but never guarantees.
 */
export const computeShotChance = (
  releaseTick: number,
  shooterPos: Vec3,
  defenders: readonly DefenderThreat[],
): ShotChance => {
  const signedErr = releaseTick - C.JUMP_APEX_TICK;
  const distance = distXZ(shooterPos, C.HOOP_POS);
  const distancePenalty = clamp01(distance / C.MAX_USEFUL_DISTANCE);
  const base =
    C.SHOT_BASE +
    timingScore(signedErr) * C.SHOT_TIMING_WEIGHT -
    distancePenalty * C.SHOT_DIST_WEIGHT -
    contestPenalty(shooterPos, defenders);
  return {
    chance: clamp(base, C.CHANCE_MIN, C.CHANCE_MAX),
    distance,
    signedErr,
    tag: classifyTiming(signedErr),
  };
};

/** The seeded make roll: attempt + shooter + timing bucket + distance bucket. */
export const rollShot = (
  chance: number,
  attemptNumber: number,
  shooterId: number,
  signedErr: number,
  distance: number,
): boolean => hashUnit(attemptNumber, shooterId, Math.abs(signedErr), Math.round(distance * 4)) < chance;

// ── jump + flight geometry ────────────────────────────────────────────────────

/** The shooter's y-offset `t` ticks after the Space press — a parabola peaking at the apex. */
export const jumpY = (t: number): number => {
  if (t <= 0 || t >= C.JUMP_TOTAL_TICKS) {
    return 0;
  }
  const u = (t - C.JUMP_APEX_TICK) / C.JUMP_APEX_TICK;
  return C.JUMP_HEIGHT * Math.max(0, 1 - u * u);
};

/** A contest-jumping defender's y-offset `t` ticks after leaving the ground. */
export const defenderJumpY = (t: number): number => {
  if (t < 0 || t >= C.DEF_JUMP_TICKS) {
    return 0;
  }
  const u = (t - C.DEF_JUMP_APEX) / C.DEF_JUMP_APEX;
  return C.DEF_JUMP_HEIGHT * Math.max(0, 1 - u * u);
};

/** A quadratic bezier from `start` to `end` whose control point arcs `height` above the higher endpoint. */
export const makeArc = (start: Vec3, end: Vec3, height: number): ShotArc => ({
  control: vec3((start.x + end.x) / 2, Math.max(start.y, end.y) + height, (start.z + end.z) / 2),
  end,
  start,
});

/** Sample the arc at `t` in 0..1. */
export const sampleArc = (arc: ShotArc, t: number): Vec3 => {
  const u = clamp(t, 0, 1);
  const a = lerp(arc.start, arc.control, u);
  const b = lerp(arc.control, arc.end, u);
  return lerp(a, b, u);
};

/**
 * Where a missed shot lands: an early release falls short on the shooter→hoop line,
 * a late one clangs long off the backboard, an on-time miss rims out to a
 * deterministically-picked side.
 */
export const missEndpoint = (signedErr: number, shooterPos: Vec3, attemptNumber: number, shooterId: number): Vec3 => {
  if (signedErr < -C.PERFECT_ERR) {
    const toHoop = normalizeXZ(shooterPos, C.HOOP_POS);
    return add(vec3(C.HOOP_POS.x, C.HOOP_Y - 0.15, C.HOOP_POS.z), scale(toHoop, -(C.RIM_RADIUS + 0.45)));
  }
  if (signedErr > C.PERFECT_ERR) {
    return vec3(0, C.HOOP_Y + 0.35, C.HOOP_Z + 0.4);
  }
  const side = hashUnit(attemptNumber, shooterId, 3) < 0.5 ? -1 : 1;
  return vec3(side * (C.RIM_RADIUS + 0.17), C.HOOP_Y + 0.02, C.HOOP_Z);
};

// ── court + turnover predicates ───────────────────────────────────────────────

/** Clamp a position to the playable half-court. */
export const clampToBounds = (p: Vec3): Vec3 =>
  vec3(clamp(p.x, -C.BOUND_X, C.BOUND_X), p.y, clamp(p.z, C.BOUND_Z_MIN, C.BOUND_Z_MAX));

/** True when any defender can reach the in-flight pass at its current position. */
export const passIntercepted = (ballPos: Vec3, defenderPositions: readonly Vec3[]): boolean =>
  ballPos.y <= C.INTERCEPT_MAX_BALL_Y && defenderPositions.some((d) => distXZ(ballPos, d) <= C.INTERCEPT_RADIUS);

/** True when any defender is touching the gathering handler — a steal. */
export const stealTouch = (handlerPos: Vec3, defenderPositions: readonly Vec3[]): boolean =>
  defenderPositions.some((d) => distXZ(handlerPos, d) <= C.STEAL_RADIUS);

// ── AI target math ────────────────────────────────────────────────────────────

/** The defender nearest the handler (ties → lowest index) becomes the primary. */
export const primaryDefenderIndex = (handlerPos: Vec3, defenderPositions: readonly Vec3[]): number => {
  let best = 0;
  let bestDist = Number.POSITIVE_INFINITY;
  defenderPositions.forEach((d, i) => {
    const dist = distXZ(handlerPos, d);
    if (dist < bestDist) {
      best = i;
      bestDist = dist;
    }
  });
  return best;
};

/**
 * Where a defender wants to stand: the primary shades the handler (PRIMARY_GAP
 * toward the hoop from them); help defenders sit partway from their assignment
 * toward the hoop, protecting the lane.
 */
export const defenderTarget = (isPrimary: boolean, assignmentPos: Vec3, handlerPos: Vec3): Vec3 => {
  if (isPrimary) {
    const toHoop = normalizeXZ(handlerPos, C.HOOP_POS);
    return vec3(handlerPos.x + toHoop.x * C.PRIMARY_GAP, 0, handlerPos.z + toHoop.z * C.PRIMARY_GAP);
  }
  return vec3(
    mix(assignmentPos.x, C.HOOP_POS.x, C.HELP_FRACTION),
    0,
    mix(assignmentPos.z, C.HOOP_POS.z, C.HELP_FRACTION),
  );
};

/** A wing's spot: its home slot, drifting away from a defender that crowds it. */
export const teammateTarget = (homeSlot: Vec3, nearestDefender: Vec3 | undefined): Vec3 => {
  if (nearestDefender === undefined || distXZ(homeSlot, nearestDefender) >= C.TEAMMATE_CROWD_RADIUS) {
    return clampToBounds(homeSlot);
  }
  const away = normalizeXZ(nearestDefender, homeSlot);
  return clampToBounds(add(homeSlot, scale(away, C.TEAMMATE_DRIFT)));
};

/**
 * Which teammate Q and E pass to. World +x renders screen-LEFT (the camera looks
 * downcourt), so the "left" target is the non-controlled blue with the GREATER x.
 */
export const passTargets = (
  bluePositions: readonly Vec3[],
  controlledIndex: number,
): { readonly left: number; readonly right: number } => {
  const others = [0, 1, 2].filter((i) => i !== controlledIndex);
  const [a, b] = others as [number, number];
  return bluePositions[a]!.x >= bluePositions[b]!.x ? { left: a, right: b } : { left: b, right: a };
};
