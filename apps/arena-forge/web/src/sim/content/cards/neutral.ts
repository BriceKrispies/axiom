/*
 * cards/neutral.ts — the four groupless cards. No shared tribal identity:
 * plain economy support and flexible stat scaling that slots into any
 * warband regardless of which tribes it is built around.
 */

import type { CardDefinition } from "../schema.ts";

export const NEUTRAL_CARDS: readonly CardDefinition[] = [
  {
    id: "neutral_coinwright",
    name: "Coinwright",
    rulesText: "A tireless tinkerer who skims a little gold from every round's takings.",
    tier: 1,
    cost: 2,
    baseAttack: 2,
    baseHealth: 3,
    groups: [],
    keywords: [],
    normal: [
      {
        trigger: "round_end",
        operations: [{ kind: "add_gold", amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "round_end",
        operations: [{ kind: "add_gold", amount: 2 }],
      },
    ],
    forgedStats: { attack: 1, health: 2 },
    visualProfile: "vp_neutral_normal",
    forgedVisualProfile: "vp_neutral_forged",
    poolCount: 18,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "neutral_bargain_scout",
    name: "Bargain Scout",
    rulesText: "Haggles with the shopkeepers before the stall even opens, shaving coin off every offer.",
    tier: 2,
    cost: 3,
    baseAttack: 3,
    baseHealth: 3,
    groups: [],
    keywords: [],
    normal: [
      {
        trigger: "shop_start",
        operations: [{ kind: "discount_shop", amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "shop_start",
        operations: [{ kind: "discount_shop", amount: 2 }],
      },
    ],
    forgedStats: { attack: 2, health: 2 },
    visualProfile: "vp_neutral_normal",
    forgedVisualProfile: "vp_neutral_forged",
    poolCount: 15,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "neutral_journeyman_smith",
    name: "Journeyman Smith",
    rulesText: "Passes on its training the instant it is let go, tempering a random ally on its way out.",
    tier: 4,
    cost: 5,
    baseAttack: 5,
    baseHealth: 5,
    groups: [],
    keywords: [],
    normal: [
      {
        trigger: "on_sell",
        operations: [
          { kind: "modify_attack", target: { kind: "random_friendly" }, amount: 1 },
          { kind: "modify_health", target: { kind: "random_friendly" }, amount: 1 },
        ],
      },
    ],
    forged: [
      {
        trigger: "on_sell",
        operations: [
          { kind: "modify_attack", target: { kind: "random_friendly" }, amount: 2 },
          { kind: "modify_health", target: { kind: "random_friendly" }, amount: 2 },
        ],
      },
    ],
    forgedStats: { attack: 3, health: 3 },
    visualProfile: "vp_neutral_normal",
    forgedVisualProfile: "vp_neutral_forged",
    poolCount: 11,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "neutral_forgeheart_titan",
    name: "Forgeheart Titan",
    rulesText: "Belongs to no tribe and every warband alike — it lends its strength to whichever ally needs it most.",
    tier: 6,
    cost: 7,
    baseAttack: 7,
    baseHealth: 8,
    groups: [],
    keywords: [],
    normal: [
      {
        trigger: "combat_start",
        operations: [
          { kind: "modify_attack", target: { kind: "lowest_attack_friendly" }, amount: 2 },
          { kind: "modify_health", target: { kind: "lowest_attack_friendly" }, amount: 2 },
        ],
      },
    ],
    forged: [
      {
        trigger: "combat_start",
        operations: [
          { kind: "modify_attack", target: { kind: "lowest_attack_friendly" }, amount: 3 },
          { kind: "modify_health", target: { kind: "lowest_attack_friendly" }, amount: 3 },
          { kind: "grant_keyword", target: { kind: "lowest_attack_friendly" }, keyword: "armored" },
        ],
      },
    ],
    forgedStats: { attack: 4, health: 5 },
    visualProfile: "vp_neutral_normal",
    forgedVisualProfile: "vp_neutral_forged",
    poolCount: 7,
    collectible: true,
    contentVersion: 1,
  },
];
