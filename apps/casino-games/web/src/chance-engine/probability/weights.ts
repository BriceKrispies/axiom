/*
 * weights.ts — weighted-tier compilation shared by every probability adapter.
 * Compiles a config's reward tiers into a cumulative table and maps one
 * uniform draw onto it; used for the conditional (given-a-win) tier pick and
 * for wheel-style arc mass.
 */

import type { RewardTier } from "../configuration/schema.ts";

/** The winning tiers a resolved win may grant (usable weight only). */
export const winnableTiers = (tiers: readonly RewardTier[]): readonly RewardTier[] =>
  tiers.filter((tier) => tier.countsAsWin && Number.isFinite(tier.weight) && tier.weight > 0);

export interface CompiledWeights<T> {
  readonly entries: readonly T[];
  /** Cumulative upper bounds in (0, 1], aligned with `entries`. */
  readonly cumulative: readonly number[];
  readonly total: number;
}

/** Compile weighted entries into a normalized cumulative table. */
export const compileWeights = <T>(entries: readonly T[], weightOf: (entry: T) => number): CompiledWeights<T> => {
  const total = entries.reduce((sum, entry) => sum + weightOf(entry), 0);
  let running = 0;
  const cumulative = entries.map((entry) => {
    running += weightOf(entry) / total;
    return running;
  });
  return { cumulative, entries, total };
};

/** Map one uniform draw in [0, 1) onto the compiled table. */
export const pickCompiledAt = <T>(compiled: CompiledWeights<T>, unit: number): T => {
  const index = compiled.cumulative.findIndex((bound) => unit < bound);
  return (index >= 0 ? compiled.entries[index] : compiled.entries[compiled.entries.length - 1]) as T;
};

/** Pick a winning tier from one conditional-on-win uniform draw. */
export const pickTierAt = (tiers: readonly RewardTier[], unit: number): RewardTier =>
  pickCompiledAt(compileWeights(winnableTiers(tiers), (tier) => tier.weight), unit);
