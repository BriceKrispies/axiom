/*
 * combination.ts — the COMBINATION probability adapter (Dice Vault, Safe
 * Cracker). The game declares its combination space (reel count, symbols per
 * reel) and which exact combinations win which tier. The adapter resolves the
 * win state with one gameplay draw at `targetWinRate`, then commits a CONCRETE
 * combination: a winning one picked through the tier weights, or a losing one
 * picked uniformly from the enumerated non-winning combinations. The dice /
 * dials then animate toward exactly that combination.
 */

import type { CasinoGameConfig } from "../configuration/schema.ts";
import { sample01 } from "../randomness/streams.ts";
import { compileWeights, pickCompiledAt, winnableTiers } from "./weights.ts";

export interface WinningCombination {
  readonly combo: readonly number[];
  readonly tierId: string;
}

/** The declared combination space. Total combinations = symbolsPerReel^reels;
 * keep it bounded (dice: 6^n, dials: symbols^3) — it is enumerated once. */
export interface CombinationSpace {
  readonly reels: number;
  readonly symbolsPerReel: number;
  readonly winningCombos: readonly WinningCombination[];
}

export interface CombinationPlan {
  readonly combination: readonly number[];
  readonly tierId: string | null;
  readonly win: boolean;
}

const comboKey = (combo: readonly number[]): string => combo.join(",");

/** Enumerate every combination NOT in the winning set (bounded, done once). */
export const losingCombinations = (space: CombinationSpace): readonly (readonly number[])[] => {
  const winning = new Set(space.winningCombos.map((w) => comboKey(w.combo)));
  const total = space.symbolsPerReel ** space.reels;
  const out: (readonly number[])[] = [];
  for (let index = 0; index < total; index += 1) {
    let rest = index;
    const combo = Array.from({ length: space.reels }, () => {
      const symbol = rest % space.symbolsPerReel;
      rest = Math.floor(rest / space.symbolsPerReel);
      return symbol;
    });
    if (!winning.has(comboKey(combo))) {
      out.push(combo);
    }
  }
  return out;
};

/**
 * Commit the round's exact combination. Winning rounds pick the tier by the
 * config's conditional weights, then a uniform combination of that tier;
 * losing rounds pick uniformly among the enumerated losing combinations.
 */
export const planCombination = (
  config: CasinoGameConfig<unknown>,
  space: CombinationSpace,
  rootSeed: number,
  round: number,
): CombinationPlan => {
  const winnable = winnableTiers(config.rewardTiers).filter((tier) =>
    space.winningCombos.some((w) => w.tierId === tier.id),
  );
  const canWin = winnable.length > 0 && space.winningCombos.length > 0;
  const win = canWin && sample01(rootSeed, "gameplay", round, 2) < config.targetWinRate;

  if (win) {
    const tier = pickCompiledAt(
      compileWeights(winnable, (t) => t.weight),
      sample01(rootSeed, "tier", round, 0),
    );
    const ofTier = space.winningCombos.filter((w) => w.tierId === tier.id);
    const chosen = ofTier[Math.floor(sample01(rootSeed, "tier", round, 1) * ofTier.length)] as WinningCombination;
    return { combination: chosen.combo, tierId: chosen.tierId, win: true };
  }

  const losers = losingCombinations(space);
  const combo = losers[Math.floor(sample01(rootSeed, "placement", round, 0) * losers.length)] as readonly number[];
  return { combination: combo, tierId: null, win: false };
};
