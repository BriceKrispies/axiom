/*
 * Scoring — a faithful port of `penalty_scoring.rs`. Points per resolved shot:
 * a base by result, plus (Goal only) a power sweet-spot bonus, a corner-placement
 * bonus, and a consecutive-goal streak bonus.
 */

import type { ResultKind } from "./result.ts";

const GOAL_BASE = 500;
const POST_BASE = 100;
const POWER_BONUS_SWEET = 150; // Goal, power in [70, 90]
const POWER_BONUS_OVER = 75; // Goal, power > 90
const PLACEMENT_UPPER_CORNER = 250;
const PLACEMENT_ANY_CORNER = 150;
const STREAK_STEP = 100;

const isUpperLeft = (x: number, y: number): boolean => x <= -70 && y >= 70;
const isUpperRight = (x: number, y: number): boolean => x >= 70 && y >= 70;
const isLowerLeft = (x: number, y: number): boolean => x <= -70 && y <= 30;
const isLowerRight = (x: number, y: number): boolean => x >= 70 && y <= 30;
const isUpperCorner = (x: number, y: number): boolean => isUpperLeft(x, y) || isUpperRight(x, y);
const isAnyCorner = (x: number, y: number): boolean => isUpperCorner(x, y) || isLowerLeft(x, y) || isLowerRight(x, y);

const baseFor = (kind: ResultKind): number => (kind === "Goal" ? GOAL_BASE : kind === "Post" ? POST_BASE : 0);

const powerBonus = (kind: ResultKind, power: number): number => {
  if (kind !== "Goal") return 0;
  if (power >= 70 && power <= 90) return POWER_BONUS_SWEET;
  return power > 90 ? POWER_BONUS_OVER : 0;
};

const placementBonus = (kind: ResultKind, x: number, y: number): number => {
  if (kind !== "Goal") return 0;
  if (isUpperCorner(x, y)) return PLACEMENT_UPPER_CORNER;
  return isAnyCorner(x, y) ? PLACEMENT_ANY_CORNER : 0;
};

const streakAfter = (kind: ResultKind, streakBefore: number): number => (kind === "Goal" ? streakBefore + 1 : 0);

const streakBonus = (kind: ResultKind, streakBefore: number): number => {
  const after = streakAfter(kind, streakBefore);
  return kind === "Goal" && after >= 2 ? STREAK_STEP * (after - 1) : 0;
};

export interface ScoreAward {
  readonly roundNumber: number;
  readonly resultKind: ResultKind;
  readonly base: number;
  readonly powerBonus: number;
  readonly placementBonus: number;
  readonly streakBonus: number;
  readonly total: number;
  readonly scoreBefore: number;
  readonly scoreAfter: number;
  readonly streakBefore: number;
  readonly streakAfter: number;
}

export const awardShot = (
  roundNumber: number,
  kind: ResultKind,
  power: number,
  targetX: number,
  targetY: number,
  scoreBefore: number,
  streakBefore: number,
): ScoreAward => {
  const base = baseFor(kind);
  const pBonus = powerBonus(kind, power);
  const placeBonus = placementBonus(kind, targetX, targetY);
  const sBonus = streakBonus(kind, streakBefore);
  const total = base + pBonus + placeBonus + sBonus;
  return {
    roundNumber,
    resultKind: kind,
    base,
    powerBonus: pBonus,
    placementBonus: placeBonus,
    streakBonus: sBonus,
    total,
    scoreBefore,
    scoreAfter: scoreBefore + total,
    streakBefore,
    streakAfter: streakAfter(kind, streakBefore),
  };
};
