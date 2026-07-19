/*
 * fielding.ts — the defensive force-play model: given a fielded GROUND ball, decide
 * which forced runners the defense throws out (and whether it turns a double play).
 * Pure and deterministic — a function of where the ball is fielded and who is on
 * base — so it replays exactly.
 *
 * Model: a ground ball fielded in the INFIELD is a routine play — the batter is
 * forced out at first. If it is fielded shallow enough (and a runner is forced) the
 * defense turns two: the lead force, then a relay to first. A ground ball that
 * reaches the OUTFIELD (or any ball not on the ground) is a hit — the runners are
 * safe and advance by the hit's base value. Baseball force rule: the batter must
 * vacate home (always forced to first); a runner is forced only if every base
 * behind him back to home is occupied.
 */

import type { Vec3 } from "./vec.ts";
import type { Runner } from "./bases.ts";
import * as C from "./constants.ts";

/** The resolved defensive play on one fielded ball. */
export interface ForcePlay {
  /** Existing base runners thrown out on the force (0…1, the lead force). */
  readonly outRunners: readonly Runner[];
  /** Whether the batter was thrown out at first. */
  readonly batterOut: boolean;
  /** Total outs recorded (`outRunners.length` + the batter). */
  readonly outs: number;
  readonly doublePlay: boolean;
  /** The bag sequence the ball is thrown to (lead-first), for the throw animation:
   * base indices 1=1st, 2=2nd, 3=3rd, 4=home. Empty when no out is made. */
  readonly throwBases: readonly number[];
}

const SAFE: ForcePlay = { batterOut: false, doublePlay: false, outRunners: [], outs: 0, throwBases: [] };

/**
 * Resolve the defense's force play on a fielded ball. `isGroundBall` is true when
 * the ball is on the ground (a grounder / rolled to the fielder — the only ball a
 * force is made on; fly balls are caught or drop for hits). `runners` are the
 * runners already on base (the batter is implicit), resting on integer bases.
 */
export const resolveForcePlay = (fieldedPos: Vec3, isGroundBall: boolean, runners: readonly Runner[]): ForcePlay => {
  const dist = Math.hypot(fieldedPos.x, fieldedPos.z);
  // Only a ground ball fielded in the infield is a force out; anything else is a hit.
  if (!isGroundBall || dist >= C.INFIELD_FORCE_RADIUS) {
    return SAFE;
  }

  const onBase = (b: number): Runner | undefined => runners.find((r) => Math.round(r.pos) === b);
  const r1 = onBase(1);
  const r2 = onBase(2);
  const r3 = onBase(3);
  // The forced runners already aboard, trailing → lead (each forced only if every
  // base behind him back to home is occupied; the batter fills home).
  const forced: { readonly runner: Runner; readonly to: number }[] = [];
  if (r1) {
    forced.push({ runner: r1, to: 2 });
  }
  if (r1 && r2) {
    forced.push({ runner: r2, to: 3 });
  }
  if (r1 && r2 && r3) {
    forced.push({ runner: r3, to: 4 });
  }

  // The routine play always forces the batter out at first.
  const lead = forced[forced.length - 1];
  // Turn two when a runner is forced and the ball was fielded shallow enough.
  const turnTwo = lead !== undefined && dist < C.DOUBLE_PLAY_RADIUS;
  const outRunners = turnTwo ? [lead.runner] : [];
  const throwBases = turnTwo ? [lead.to, 1] : [1];
  return { batterOut: true, doublePlay: turnTwo, outRunners, outs: outRunners.length + 1, throwBases };
};
