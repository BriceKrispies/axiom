/*
 * round.ts — the round STATE MACHINE, pure and SDK-free (no physics, no rendering), so
 * every scoring rule is unit-testable in bare Node. The session owns one `RoundState`
 * and drives it: `recordThrow` when a ball leaves your hand, `recordLanding` when one
 * touches down, `settle` while the last balls come to rest. The scene + HUD only read
 * it.
 *
 * A round is BALLS_PER_ROUND throws from one stand in one wind, thrown as fast as you
 * like — the next ball is in your hand the instant the last one leaves it, and several
 * are in the air at once.
 *
 * ## Nothing is scored out loud until the rack is empty
 *
 * Landings accumulate silently. There is no per-throw verdict, no points popup, no
 * running total, and `phase` stays `"throwing"` no matter how many balls have already
 * landed. The scoreboard exists only in the `"results"` phase, once every ball is down.
 *
 * That is a deliberate design constraint, not an omission. A round is one continuous
 * act; interrupting it eight times with a scorecard breaks the rhythm and, worse, hands
 * the player a correction mid-rack. What they get instead is physical: the balls stay
 * where they landed, so the grouping is visible on the ground the whole time. You can
 * see you are throwing long — you are just never told in points.
 */

import { type Ring, isBigLanding, isOnTarget, labelFor, pointsFor, ringFor } from "./target.ts";
import { BALLS_PER_ROUND, SETTLE_TICKS } from "./constants.ts";

/** Where the round is right now. */
export type RoundPhase = "throwing" | "settling" | "results";

/** The verdict on one landed ball. Held back until the round ends. */
export interface ThrowResult {
  /** Which ball of the rack this was (0-based). */
  readonly index: number;
  /** Horizontal distance from the centre at first ground contact (m). */
  readonly distance: number;
  readonly ring: Ring;
  readonly label: string;
  readonly points: number;
}

/** The whole round. */
export interface RoundState {
  phase: RoundPhase;
  /** How many balls have left your hand (0 … BALLS_PER_ROUND). */
  thrown: number;
  /** How many have touched down. */
  landed: number;
  /** Every landing so far, in the order the balls came down. */
  results: ThrowResult[];
  /** Ticks left in the settle pause before the scoreboard appears. */
  settleTicks: number;
  best: number;
}

/** A fresh round, carrying `best` forward. */
export const newRound = (best: number): RoundState => ({
  best,
  landed: 0,
  phase: "throwing",
  results: [],
  settleTicks: SETTLE_TICKS,
  thrown: 0,
});

/** Balls still in the rack, including the one in your hand. */
export const ballsLeft = (state: RoundState): number => BALLS_PER_ROUND - state.thrown;

/** Whether another ball can be picked up. */
export const hasBallInHand = (state: RoundState): boolean =>
  state.phase === "throwing" && state.thrown < BALLS_PER_ROUND;

/** The round's score so far. Not shown until `"results"` — see the header. */
export const totalScore = (state: RoundState): number =>
  state.results.reduce((sum, result) => sum + result.points, 0);

/** How many landings were a bullseye or better. */
export const bullseyeCount = (state: RoundState): number =>
  state.results.filter((result) => isBigLanding(result.distance)).length;

/** How many landed on the painted target at all. */
export const onTargetCount = (state: RoundState): number =>
  state.results.filter((result) => isOnTarget(result.distance)).length;

/** The tightest landing of the round (m), or `null` before anything has landed. */
export const bestLanding = (state: RoundState): number | null =>
  state.results.reduce<number | null>(
    (best, result) => (best === null ? result.distance : Math.min(best, result.distance)),
    null,
  );

/** A ball has left your hand. Moves to `"settling"` once the rack is empty. */
export const recordThrow = (state: RoundState): void => {
  state.thrown = Math.min(state.thrown + 1, BALLS_PER_ROUND);
  state.phase = state.thrown >= BALLS_PER_ROUND ? "settling" : state.phase;
};

/** A ball has touched down `distance` metres from the centre. Silent by design. */
export const recordLanding = (state: RoundState, index: number, distance: number): ThrowResult => {
  const result: ThrowResult = {
    distance,
    index,
    label: labelFor(distance),
    points: pointsFor(distance),
    ring: ringFor(distance),
  };
  state.results.push(result);
  state.landed += 1;
  return result;
};

/**
 * Advance the settle pause. Returns true on the tick the scoreboard should appear:
 * every ball thrown, every ball landed, and the brief pause elapsed.
 */
export const settle = (state: RoundState): boolean => {
  const waiting = state.phase === "settling" && state.landed >= state.thrown;
  state.settleTicks = waiting ? state.settleTicks - 1 : state.settleTicks;
  const done = waiting && state.settleTicks <= 0;
  state.phase = done ? "results" : state.phase;
  state.best = done ? Math.max(state.best, totalScore(state)) : state.best;
  return done;
};
