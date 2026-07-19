/*
 * bases.ts — the base-running model: how many bases a fair ball earns, and the
 * pure per-tick advancement of the runners around the diamond. It is SDK-free and
 * deterministic (a pure function of the hit outcome and the ticks), so it replays
 * bit-for-bit and runs under bare `node --test` like the rest of the game core.
 *
 * Runners are PERSISTENT: after a fair ball that isn't caught on the fly, the
 * batter becomes a new runner at home and every runner already on base advances by
 * the same number of bases (real forced-advance order is preserved — everyone moves
 * the same amount, so no runner passes another). A runner who reaches home scores a
 * run. Position is a continuous base index `pos ∈ [0,4]` (0 and 4 are home); the
 * session drives `advanceRunner` each tick and reads `runnerWorld` for the scene.
 */

import { type Vec3, add, scale, sub, vec3 } from "./vec.ts";
import type { Outcome } from "./types.ts";
import * as C from "./constants.ts";

/** A single base runner (mutable, owned by the session — like `BallFlight`). */
export interface Runner {
  /** Continuous base index along the path: 0 = home (just batted), 1 = 1B, 2 = 2B,
   * 3 = 3B, 4 = home again (scored). Integers are "standing on that base". */
  pos: number;
  /** The integer base this runner is advancing to on the current play. */
  target: number;
  /** Cumulative distance run (world units) — drives the running gait's phase. */
  traveled: number;
  /** Set once the runner reaches home (a scored run); the session then retires it. */
  scored: boolean;
  /** A small fixed inward lane offset so two runners never perfectly overlap. */
  lane: number;
}

/**
 * How many bases a resolved hit earns (0…4). Only a ball caught ON THE FLY is an
 * out (0). A homer clears the yard (4); any other fair ball that drops or is
 * fielded on the ground is a hit, its base count scaling with how far it got. A
 * foul, ball, or miss is not in play (0).
 */
export const basesForHit = (outcome: Outcome, dist: number, caughtOnFly: boolean): number => {
  if (outcome === "homer") {
    return 4;
  }
  if (outcome === "foul" || outcome === "ball" || outcome === "miss" || caughtOnFly) {
    return 0;
  }
  if (dist >= C.TRIPLE_DIST) {
    return 3;
  }
  if (dist >= C.DOUBLE_DIST) {
    return 2;
  }
  return 1;
};

/** The world XZ point at a continuous base index `pos ∈ [0,4]` (4 wraps to home),
 * with the runner's inward lane offset applied. */
export const runnerWorld = (runner: Runner): Vec3 => {
  const p = Math.min(Math.max(runner.pos, 0), 4);
  const seg = Math.min(Math.floor(p), 3);
  const f = p - seg;
  const a = C.BASE_POINTS[seg]!;
  const b = C.BASE_POINTS[(seg + 1) % 4]!;
  const along = add(a, scale(sub(b, a), f));
  // Offset toward the diamond center (2B side) by the runner's lane amount.
  const center = C.BASE_POINTS[2]!;
  const toCenter = sub(center, along);
  const len = Math.hypot(toCenter.x, toCenter.z);
  const inward = len > 1e-6 ? scale(vec3(toCenter.x, 0, toCenter.z), runner.lane / len) : vec3(0, 0, 0);
  return add(along, inward);
};

/** The runner's heading (yaw) along the base path at its current position — the
 * tangent of the leg it is running, so the figure faces where it runs. */
export const runnerFacing = (runner: Runner): number => {
  const seg = Math.min(Math.floor(Math.min(Math.max(runner.pos, 0), 4)), 3);
  const dir = sub(C.BASE_POINTS[(seg + 1) % 4]!, C.BASE_POINTS[seg]!);
  return Math.atan2(dir.x, dir.z);
};

/** Whether the runner is still short of its target (i.e. actively running). */
export const runnerMoving = (runner: Runner): boolean => runner.pos < runner.target - 1e-6;

/**
 * Advance one runner toward its target by this tick's foot distance. Returns the
 * base-units actually traversed (0 while resting), and flags a fresh score the
 * instant the runner reaches home so the caller can tally the run.
 */
export const advanceRunner = (runner: Runner): { readonly advanced: number; readonly justScored: boolean } => {
  if (!runnerMoving(runner)) {
    return { advanced: 0, justScored: false };
  }
  const stepBases = C.RUNNER_SPEED / C.FIXED_HZ / C.BASE_LEG;
  const before = runner.pos;
  runner.pos = Math.min(runner.target, runner.pos + stepBases);
  const advanced = runner.pos - before;
  runner.traveled += advanced * C.BASE_LEG;
  const justScored = !runner.scored && runner.target >= 4 && runner.pos >= 4 - 1e-6;
  if (justScored) {
    runner.scored = true;
  }
  return { advanced, justScored };
};

/** A fresh batter-turned-runner leaving home with a `bases`-base destination. */
export const newRunner = (bases: number, lane: number): Runner => ({ lane, pos: 0, scored: false, target: Math.min(4, bases), traveled: 0 });
