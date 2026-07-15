/*
 * choice-population.ts — the CHOICE-POPULATION probability adapter, used by
 * every "pick one object from a visible set" game (chests, cards, doors,
 * presents, map digs, portals, rocks).
 *
 * For `n` selectable objects at target win rate `p`, the round realizes
 * exactly `stochasticRound(n·p)` winning objects, assigned BEFORE the player
 * chooses: floor(n·p) always win, plus one more when the fractional part
 * succeeds against the gameplay stream. Placement is a deterministic shuffle
 * on the placement stream; tiers come from the tier stream. A single round's
 * realized probability is winnerCount/n; repeated rounds converge to `p`.
 * The selection only LOOKS UP its preassigned slot — never rerolls it.
 */

import type { CasinoGameConfig } from "../configuration/schema.ts";
import { sample01, sampleChance, shuffled } from "../randomness/streams.ts";
import { pickTierAt, winnableTiers } from "./weights.ts";

/** The committed population: `winnersByIndex[i]` is the tier id object `i`
 * reveals, or null for an empty object. */
export interface ChoicePopulation {
  readonly winnersByIndex: readonly (string | null)[];
  readonly winnerCount: number;
  /** The exact expectation `n·p` the stochastic rounding converges to. */
  readonly expectedWinners: number;
}

/**
 * Assign the round's winning objects. Draw keys are (round, label) so the
 * population is a pure function of (seed, round, config).
 */
export const planChoicePopulation = (
  config: CasinoGameConfig<unknown>,
  choiceCount: number,
  rootSeed: number,
  round: number,
): ChoicePopulation => {
  const p = config.targetWinRate;
  const expectedWinners = choiceCount * p;
  const base = Math.floor(expectedWinners);
  const fraction = expectedWinners - base;
  const extra = sampleChance(fraction, rootSeed, "gameplay", round, 0) ? 1 : 0;
  const winnerCount = Math.max(0, Math.min(choiceCount, base + extra));

  const indices = shuffled(
    Array.from({ length: choiceCount }, (_, i) => i),
    rootSeed,
    "placement",
    round,
  );
  const winners = new Set(indices.slice(0, winnerCount));

  const anyWinnable = winnableTiers(config.rewardTiers).length > 0;
  const winnersByIndex = Array.from({ length: choiceCount }, (_, i) =>
    winners.has(i) && anyWinnable ? pickTierAt(config.rewardTiers, sample01(rootSeed, "tier", round, i)).id : null,
  );

  return { expectedWinners, winnerCount: anyWinnable ? winnerCount : 0, winnersByIndex };
};
