/*
 * round.ts — LAYER 3 of the CSS3D build: the game rules.
 *
 * This file does NOT reimplement the chest game's chance logic. It imports the
 * real thing:
 *
 *   - `baseConfig` / `CasinoGameConfig` — the same versioned config schema
 *   - `validateConfig`                  — the same validation gate
 *   - `planChoicePopulation`            — the SAME probability adapter the
 *                                         shipped Treasure Chest Pick uses
 *   - `sample01` / streams              — the same seeded, purpose-separated RNG
 *
 * That is what makes this a second PRESENTATION of the game rather than a
 * lookalike: click a chest and the outcome is decided by exactly the code the
 * engine build runs. The whole chance-engine is pure TypeScript with no renderer
 * dependency, which is precisely why it can be reused by a canvas-free front end.
 *
 * FAIRNESS, unchanged from the source: `planChoicePopulation` assigns which
 * chests hold prizes BEFORE the player picks (a deterministic shuffle on the
 * `placement` stream); picking only LOOKS UP a preassigned slot and can never
 * reroll it. For `n` chests at rate `p`, exactly `floor(n·p) + Bernoulli(frac)`
 * of them win, so one round realizes `winners/n` and repeated rounds converge
 * to `p`.
 *
 * The shipped default config is `targetWinRate: 1` with a single 5-point
 * consolation tier — every chest wins the same small prize. That is a fine
 * arcade default but shows nothing about chance, so this build defaults to the
 * documented four-tier ladder from the app README at a 0.44 win rate, and
 * exposes the rate as a live control. The MECHANISM is identical either way.
 */

import type { CasinoGameConfig, RewardTier } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { validateConfig } from "../../chance-engine/configuration/validation.ts";
import { planChoicePopulation } from "../../chance-engine/probability/choice-population.ts";
import { sample01 } from "../../chance-engine/randomness/streams.ts";

/** How many chests sit on the board. */
export const CHEST_COUNT = 9;

/** The README's documented reward ladder — weights are CONDITIONAL ON WINNING. */
const TIERS: readonly RewardTier[] = [
  { countsAsWin: true, id: "common", label: "Star Token", rarity: "common", reward: { amount: 25, kind: "stars", label: "25 stars" }, weight: 60 },
  { countsAsWin: true, id: "uncommon", label: "Ticket Bundle", rarity: "uncommon", reward: { amount: 120, kind: "tickets", label: "120 tickets" }, weight: 28 },
  { countsAsWin: true, id: "rare", label: "Gem Trophy", rarity: "rare", reward: { amount: 1, kind: "gems", label: "Radiant gem" }, weight: 10 },
  { countsAsWin: true, id: "jackpot", label: "Golden Capsule", rarity: "jackpot", reward: { amount: 1, kind: "capsules", label: "Golden capsule" }, weight: 2 },
];

export interface ChestSpec {
  readonly brand: string;
  readonly danceLiveliness: number;
}

/** Build the round config through the REAL schema helper, then validate it
 * through the REAL gate — an invalid config must never reach a session. */
export const buildConfig = (targetWinRate: number): CasinoGameConfig<ChestSpec> =>
  baseConfig<ChestSpec>("treasure-chest-pick", "Treasure Chest Pick", "tabletop", { brand: "ACME", danceLiveliness: 0.7 }, {
    choiceCount: CHEST_COUNT,
    rewardTiers: TIERS,
    targetWinRate,
  });

export interface PickResult {
  readonly index: number;
  readonly won: boolean;
  readonly tier: RewardTier | null;
  /** What the chest shows when it opens. */
  readonly label: string;
}

export interface Round {
  readonly config: CasinoGameConfig<ChestSpec>;
  readonly seed: number;
  readonly round: number;
  /** Which chests hold a prize — decided before any pick. */
  readonly winnersByIndex: readonly (string | null)[];
  readonly winnerCount: number;
  readonly issues: readonly string[];
  /** Look up a chest's preassigned slot. Never rerolls. */
  readonly reveal: (index: number) => PickResult;
  /** A deterministic decoration value on the `ambient` stream — used for idle
   * motion, so no sparkle can ever perturb the outcome streams. */
  readonly ambient: (key: number) => number;
}

/** Open a round: plan the population up front, exactly as the source does. */
export const startRound = (seed: number, round: number, targetWinRate: number): Round => {
  const config = buildConfig(targetWinRate);
  const issues = validateConfig(config).map((issue) => `${issue.path}: ${issue.message}`);
  const population = planChoicePopulation(config, CHEST_COUNT, seed, round);
  const tierById = new Map(config.rewardTiers.map((tier) => [tier.id, tier]));

  return {
    ambient: (key: number): number => sample01(seed, "ambient", round, key),
    config,
    issues,
    reveal: (index: number): PickResult => {
      const tierId = population.winnersByIndex[index] ?? null;
      const tier = tierId === null ? null : tierById.get(tierId) ?? null;
      return {
        index,
        label: tier === null ? "" : tier.reward.label,
        tier,
        won: tier !== null,
      };
    },
    round,
    seed,
    winnerCount: population.winnerCount,
    winnersByIndex: population.winnersByIndex,
  };
};
