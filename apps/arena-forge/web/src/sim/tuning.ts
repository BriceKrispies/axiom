/*
 * tuning.ts — the data-driven rules of Arena Forge. Every economic and pacing
 * number lives here as plain data (no code paths, no card-specific values), so
 * the balance of the whole game is one auditable object. The match reads
 * `DEFAULT_RULES`; tests and the accelerated dev harness may pass a modified copy
 * to change pacing WITHOUT touching production rules. Everything is integer.
 */

import type { ForgeReward, Tier } from "./content/schema.ts";

/** The simulation fixed step. Shop deadlines are expressed in these ticks. */
export const FIXED_HZ = 30;

/** The full, data-driven rule set. */
export interface Rules {
  readonly rulesVersion: number;
  readonly startingHealth: number;
  readonly startingGold: number;
  readonly startingForgeRank: number;
  readonly maxForgeRank: number;
  readonly maxGold: number;
  /** Gold granted at the start of round N (1-indexed); rounds past the array
   * length use the last value. Clamped to `maxGold`. */
  readonly goldByRound: readonly number[];
  readonly rerollCost: number;
  readonly sellValue: number;
  /** Cost to upgrade FROM rank `r` to `r+1`, indexed by `r-1`. */
  readonly forgeRankUpgradeCosts: readonly number[];
  /** Shop card count at forge rank `r`, indexed by `r-1`. */
  readonly shopSizeByRank: readonly number[];
  /** Roll weight of each tier (index 0 = tier 1 … 5 = tier 6) at forge rank `r`,
   * indexed by `r-1`. Weight 0 means that tier cannot appear at that rank. */
  readonly tierWeightsByRank: readonly (readonly number[])[];
  readonly handLimit: number;
  readonly warbandLimit: number;
  readonly copiesToForge: number;
  readonly defaultForgeReward: ForgeReward;
  readonly shopTimerSeconds: number;
  readonly combatPlaybackSeconds: number;
  readonly maxConsequence: number;
  /** Round at which anti-stalemate escalation begins. */
  readonly escalationStartRound: number;
  /** Extra loser damage added per round past `escalationStartRound`. Added AFTER
   * the base-formula clamp so a late game cannot stalemate — it guarantees the
   * termination invariant (every match ends within a bounded number of rounds). */
  readonly consequenceEscalation: number;
  /** Hard ceiling on rounds before the harness declares a runaway match. */
  readonly maxRounds: number;
  /** Arena-stage thresholds (see stage.ts): each stage needs forgeRank >= rank,
   * forgedUnits >= forged, and warbandPower >= power. */
  readonly stageThresholds: readonly {
    readonly stage: "workshop" | "kindled" | "tempered" | "masterwork";
    readonly rank: number;
    readonly forged: number;
    readonly power: number;
  }[];
}

const tierArray = (...w: number[]): readonly number[] => w;

/** The production rules. */
export const DEFAULT_RULES: Rules = {
  rulesVersion: 1,
  startingHealth: 30,
  startingGold: 3,
  startingForgeRank: 1,
  maxForgeRank: 6,
  maxGold: 10,
  goldByRound: [3, 4, 5, 6, 7, 8, 9, 10],
  rerollCost: 1,
  sellValue: 1,
  forgeRankUpgradeCosts: [5, 6, 7, 8, 9],
  shopSizeByRank: [3, 4, 4, 5, 5, 6],
  tierWeightsByRank: [
    tierArray(100, 0, 0, 0, 0, 0),
    tierArray(70, 30, 0, 0, 0, 0),
    tierArray(45, 35, 20, 0, 0, 0),
    tierArray(30, 33, 25, 12, 0, 0),
    tierArray(20, 28, 27, 18, 7, 0),
    tierArray(14, 22, 25, 22, 12, 5),
  ],
  handLimit: 6,
  warbandLimit: 7,
  copiesToForge: 3,
  defaultForgeReward: { kind: "gold", amount: 1 },
  shopTimerSeconds: 45,
  combatPlaybackSeconds: 12,
  maxConsequence: 15,
  escalationStartRound: 8,
  consequenceEscalation: 2,
  maxRounds: 60,
  stageThresholds: [
    { stage: "masterwork", rank: 5, forged: 3, power: 60 },
    { stage: "tempered", rank: 4, forged: 2, power: 36 },
    { stage: "kindled", rank: 2, forged: 1, power: 16 },
    { stage: "workshop", rank: 1, forged: 0, power: 0 },
  ],
};

/** The shop card count at a forge rank (clamped to the rank table). */
export const shopSizeForRank = (rules: Rules, rank: number): number => {
  const idx = Math.max(0, Math.min(rules.shopSizeByRank.length - 1, rank - 1));
  return rules.shopSizeByRank[idx] ?? 3;
};

/** The tier roll weights at a forge rank. */
export const tierWeightsForRank = (rules: Rules, rank: number): readonly number[] => {
  const idx = Math.max(0, Math.min(rules.tierWeightsByRank.length - 1, rank - 1));
  return rules.tierWeightsByRank[idx] ?? [100, 0, 0, 0, 0, 0];
};

/** Gold granted at the start of a round, clamped to `maxGold`. */
export const goldForRound = (rules: Rules, round: number): number => {
  const idx = Math.max(0, Math.min(rules.goldByRound.length - 1, round - 1));
  return Math.min(rules.maxGold, rules.goldByRound[idx] ?? rules.maxGold);
};

/** Cost to upgrade from a forge rank, or null if already at max. */
export const forgeUpgradeCost = (rules: Rules, rank: number): number | null => {
  if (rank >= rules.maxForgeRank) {
    return null;
  }
  return rules.forgeRankUpgradeCosts[rank - 1] ?? null;
};

/** All six tiers as a typed list. */
export const ALL_TIERS: readonly Tier[] = [1, 2, 3, 4, 5, 6];
