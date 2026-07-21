/*
 * cards/ironbound.ts — the Formation tribe. Identity: adjacency bonuses,
 * `armored`/`guard` bulk, and permanent stat growth earned by standing in the
 * right position at the right time. Eight cards spanning tier 1 (an
 * adjacency-triggered recruit and a guard body) through tier 6 (a
 * build-around that hardens the whole line once the formation is wide
 * enough).
 */

import type { CardDefinition } from "../schema.ts";

export const IRONBOUND_CARDS: readonly CardDefinition[] = [
  {
    id: "iron_recruit",
    name: "Iron Recruit",
    rulesText:
      "A first casting from the Ironbound foundry, drilled to stand shoulder-to-shoulder. When bought while beside another Ironbound unit, its plating is tightened on the spot.",
    tier: 1,
    cost: 2,
    baseAttack: 2,
    baseHealth: 3,
    groups: ["ironbound"],
    keywords: [],
    normal: [
      {
        trigger: "on_buy",
        conditions: [{ kind: "adjacent_in_group", group: "ironbound" }],
        operations: [
          { kind: "modify_attack", target: { kind: "self" }, amount: 1 },
          { kind: "modify_health", target: { kind: "self" }, amount: 1 },
        ],
      },
    ],
    forged: [
      {
        trigger: "on_buy",
        conditions: [{ kind: "adjacent_in_group", group: "ironbound" }],
        operations: [
          { kind: "modify_attack", target: { kind: "self" }, amount: 2 },
          { kind: "modify_health", target: { kind: "self" }, amount: 2 },
          { kind: "grant_keyword", target: { kind: "self" }, keyword: "armored" },
        ],
      },
    ],
    forgedStats: { attack: 1, health: 2 },
    visualProfile: "vp_ironbound_normal",
    forgedVisualProfile: "vp_ironbound_forged",
    poolCount: 18,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "iron_shieldling",
    name: "Iron Shieldling",
    rulesText:
      "A squat guard-construct built to eat the first blow. Whenever it takes the field, it braces its neighbors' plating.",
    tier: 1,
    cost: 2,
    baseAttack: 1,
    baseHealth: 4,
    groups: ["ironbound"],
    keywords: ["guard"],
    normal: [
      {
        trigger: "on_play",
        operations: [{ kind: "modify_health", target: { kind: "adjacent_friendly" }, amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "on_play",
        operations: [
          { kind: "modify_health", target: { kind: "adjacent_friendly" }, amount: 2 },
          { kind: "modify_attack", target: { kind: "adjacent_friendly" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 1, health: 2 },
    visualProfile: "vp_ironbound_normal",
    forgedVisualProfile: "vp_ironbound_forged",
    poolCount: 18,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "iron_wallwright",
    name: "Iron Wallwright",
    rulesText:
      "Reinforces itself between rounds, so long as the foundry has cast at least one more Ironbound unit beside it in the warband.",
    tier: 2,
    cost: 3,
    baseAttack: 3,
    baseHealth: 5,
    groups: ["ironbound"],
    keywords: ["armored"],
    normal: [
      {
        trigger: "shop_start",
        conditions: [{ kind: "friendly_group_count_at_least", group: "ironbound", value: 2 }],
        operations: [{ kind: "modify_health", target: { kind: "self" }, amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "shop_start",
        conditions: [{ kind: "friendly_group_count_at_least", group: "ironbound", value: 2 }],
        operations: [
          { kind: "modify_health", target: { kind: "self" }, amount: 2 },
          { kind: "modify_attack", target: { kind: "self" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 2 },
    visualProfile: "vp_ironbound_normal",
    forgedVisualProfile: "vp_ironbound_forged",
    poolCount: 15,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "iron_drillmaster",
    name: "Iron Drillmaster",
    rulesText:
      "Mentors whoever is cast beside it. On purchase, it drills its Ironbound neighbors' plating tighter.",
    tier: 2,
    cost: 3,
    baseAttack: 3,
    baseHealth: 4,
    groups: ["ironbound"],
    keywords: [],
    normal: [
      {
        trigger: "on_buy",
        conditions: [{ kind: "adjacent_in_group", group: "ironbound" }],
        operations: [
          { kind: "modify_attack", target: { kind: "adjacent_friendly" }, amount: 1 },
          { kind: "modify_health", target: { kind: "adjacent_friendly" }, amount: 1 },
        ],
      },
    ],
    forged: [
      {
        trigger: "on_buy",
        operations: [
          { kind: "modify_attack", target: { kind: "friendly_in_group", group: "ironbound" }, amount: 1 },
          { kind: "modify_health", target: { kind: "friendly_in_group", group: "ironbound" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 2 },
    visualProfile: "vp_ironbound_normal",
    forgedVisualProfile: "vp_ironbound_forged",
    poolCount: 15,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "iron_bulwark",
    name: "Iron Bulwark",
    rulesText:
      "A standing wall of plate that continuously reinforces whoever fights beside it for as long as the battle lasts.",
    tier: 3,
    cost: 4,
    baseAttack: 4,
    baseHealth: 7,
    groups: ["ironbound"],
    keywords: ["guard", "armored"],
    normal: [
      {
        trigger: "passive_aura",
        operations: [{ kind: "modify_health", target: { kind: "adjacent_friendly" }, amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "passive_aura",
        operations: [
          { kind: "modify_health", target: { kind: "adjacent_friendly" }, amount: 2 },
          { kind: "modify_attack", target: { kind: "adjacent_friendly" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 3 },
    visualProfile: "vp_ironbound_normal",
    forgedVisualProfile: "vp_ironbound_forged",
    poolCount: 13,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "iron_vanguard",
    name: "Iron Vanguard",
    rulesText:
      "Leads the charge from the front. When it stands at the head of the line, it throws its full weight into its next strike.",
    tier: 4,
    cost: 5,
    baseAttack: 5,
    baseHealth: 6,
    groups: ["ironbound"],
    keywords: [],
    normal: [
      {
        trigger: "before_attack",
        conditions: [{ kind: "source_position_leftmost" }],
        operations: [{ kind: "modify_attack", target: { kind: "self" }, amount: 2 }],
      },
    ],
    forged: [
      {
        trigger: "before_attack",
        conditions: [{ kind: "source_position_leftmost" }],
        operations: [
          { kind: "modify_attack", target: { kind: "self" }, amount: 3 },
          { kind: "grant_keyword", target: { kind: "self" }, keyword: "armored" },
        ],
      },
    ],
    forgedStats: { attack: 3, health: 3 },
    visualProfile: "vp_ironbound_normal",
    forgedVisualProfile: "vp_ironbound_forged",
    poolCount: 11,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "iron_colossus",
    name: "Iron Colossus",
    rulesText:
      "A towering foundry-titan. When it finally falls, its collapsing plating reinforces whoever stood at its side.",
    tier: 5,
    cost: 6,
    baseAttack: 6,
    baseHealth: 9,
    groups: ["ironbound"],
    keywords: ["armored"],
    normal: [
      {
        trigger: "on_death",
        operations: [
          { kind: "modify_attack", target: { kind: "adjacent_friendly" }, amount: 2 },
          { kind: "modify_health", target: { kind: "adjacent_friendly" }, amount: 2 },
        ],
      },
    ],
    forged: [
      {
        trigger: "on_death",
        operations: [
          { kind: "modify_attack", target: { kind: "adjacent_friendly" }, amount: 3 },
          { kind: "modify_health", target: { kind: "adjacent_friendly" }, amount: 3 },
          { kind: "grant_keyword", target: { kind: "adjacent_friendly" }, keyword: "armored" },
        ],
      },
    ],
    forgedStats: { attack: 3, health: 4 },
    visualProfile: "vp_ironbound_normal",
    forgedVisualProfile: "vp_ironbound_forged",
    poolCount: 9,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "iron_bastion_prime",
    name: "Iron Bastion Prime",
    rulesText:
      "The foundry's masterwork. Once the line is at least three Ironbound strong, it drills the entire warband into a single armored formation as battle begins.",
    tier: 6,
    cost: 7,
    baseAttack: 7,
    baseHealth: 10,
    groups: ["ironbound"],
    keywords: ["guard", "armored"],
    normal: [
      {
        trigger: "combat_start",
        conditions: [{ kind: "friendly_group_count_at_least", group: "ironbound", value: 3 }],
        operations: [
          { kind: "modify_health", target: { kind: "all_friendly" }, amount: 2 },
          { kind: "grant_keyword", target: { kind: "all_friendly" }, keyword: "armored" },
        ],
      },
    ],
    forged: [
      {
        trigger: "combat_start",
        conditions: [{ kind: "friendly_group_count_at_least", group: "ironbound", value: 3 }],
        operations: [
          { kind: "modify_health", target: { kind: "all_friendly" }, amount: 3 },
          { kind: "modify_attack", target: { kind: "all_friendly" }, amount: 1 },
          { kind: "grant_keyword", target: { kind: "all_friendly" }, keyword: "armored" },
        ],
      },
    ],
    forgedStats: { attack: 4, health: 5 },
    visualProfile: "vp_ironbound_normal",
    forgedVisualProfile: "vp_ironbound_forged",
    poolCount: 7,
    collectible: true,
    contentVersion: 1,
  },
];
