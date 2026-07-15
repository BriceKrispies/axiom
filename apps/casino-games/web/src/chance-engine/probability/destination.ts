/*
 * destination.ts — the DESTINATION probability adapter, used by games that
 * resolve into a visible location (drop slot, wheel segment, planet, elevator
 * floor, fountain basin, capsule, lantern color band).
 *
 * The game describes its destinations once (id, tier or losing, relative
 * mass); the adapter compiles them so the WINNING destinations share exactly
 * `targetWinRate` of the probability, proportioned by mass, and the losing
 * destinations share the rest. One gameplay-stream draw commits the
 * destination; the game then animates a plausible trajectory TOWARD it and
 * must finish there — never snap at the last frame.
 */

import { sample01 } from "../randomness/streams.ts";
import { compileWeights, pickCompiledAt } from "./weights.ts";

/** One visible destination. `tierId` null marks a losing destination. */
export interface DestinationSlot {
  readonly id: string;
  readonly tierId: string | null;
  /** Relative mass among its group (winning vs losing). Must be > 0. */
  readonly mass: number;
}

export interface DestinationPlan {
  readonly index: number;
  readonly slot: DestinationSlot;
  readonly win: boolean;
}

/** The compiled per-slot probability, exposed for wheel arc mass + tests. */
export const destinationProbabilities = (slots: readonly DestinationSlot[], targetWinRate: number): readonly number[] => {
  const winMass = slots.filter((s) => s.tierId !== null).reduce((sum, s) => sum + s.mass, 0);
  const lossMass = slots.filter((s) => s.tierId === null).reduce((sum, s) => sum + s.mass, 0);
  return slots.map((slot) =>
    slot.tierId !== null
      ? (winMass > 0 ? (targetWinRate * slot.mass) / winMass : 0)
      : (lossMass > 0 ? ((1 - targetWinRate) * slot.mass) / lossMass : 0),
  );
};

/**
 * Commit the destination with ONE gameplay-stream draw: `u < p` lands in the
 * winning group (sub-picked by mass at `u/p`), otherwise the losing group at
 * `(u−p)/(1−p)`. `targetWinRate` 0 can never win; 1 always wins (when a
 * winning slot exists — validation guarantees it).
 */
export const planDestination = (
  slots: readonly DestinationSlot[],
  targetWinRate: number,
  rootSeed: number,
  round: number,
): DestinationPlan => {
  const winning = slots.filter((slot) => slot.tierId !== null);
  const losing = slots.filter((slot) => slot.tierId === null);
  const u = sample01(rootSeed, "gameplay", round, 1);
  const winSide = winning.length > 0 && (u < targetWinRate || losing.length === 0);
  const group = winSide ? winning : losing;
  const denominator = winSide ? Math.max(targetWinRate, Number.EPSILON) : Math.max(1 - targetWinRate, Number.EPSILON);
  const sub = winSide ? u / denominator : (u - targetWinRate) / denominator;
  const slot = pickCompiledAt(
    compileWeights(group, (entry) => entry.mass),
    Math.min(Math.max(sub, 0), 0.999_999_9),
  );
  return { index: slots.indexOf(slot), slot, win: slot.tierId !== null };
};
