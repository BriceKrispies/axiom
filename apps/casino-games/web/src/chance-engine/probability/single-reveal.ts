/*
 * single-reveal.ts — the SINGLE-REVEAL probability adapter (Scratch Reveal,
 * Ball Machine, Fishing Cast, Claw Grab). One Bernoulli gameplay draw at
 * `targetWinRate` commits the win state; the tier stream picks the conditional
 * reward tier. Player context (targeted prize, cast region) may choose WHICH
 * visual object or reward family manifests, but never rerolls the outcome.
 */

import type { CasinoGameConfig } from "../configuration/schema.ts";
import { sample01 } from "../randomness/streams.ts";
import { pickTierAt, winnableTiers } from "./weights.ts";

export interface SingleRevealPlan {
  readonly win: boolean;
  readonly tierId: string | null;
}

export const planSingleReveal = (
  config: CasinoGameConfig<unknown>,
  rootSeed: number,
  round: number,
): SingleRevealPlan => {
  const canWin = winnableTiers(config.rewardTiers).length > 0;
  const win = canWin && sample01(rootSeed, "gameplay", round, 3) < config.targetWinRate;
  const tierId = win ? pickTierAt(config.rewardTiers, sample01(rootSeed, "tier", round, 2)).id : null;
  return { tierId, win };
};
