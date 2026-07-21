/*
 * cards/echowisp.ts — the Trickery tribe. Identity: repositioning
 * (`move_unit`/`swap_with`), borrowed abilities (`copy_ability`), and
 * conditions keyed on board order. Eight cards spanning tier 1 (a sprite
 * that darts to the front) through tier 6 (a build-around that channels the
 * whole illusion-circle's trick).
 */

import type { CardDefinition } from "../schema.ts";

export const ECHOWISP_CARDS: readonly CardDefinition[] = [
  {
    id: "echo_flicker_sprite",
    name: "Echo Flicker Sprite",
    rulesText: "Darts to the front of the line the instant it finds itself at the back, striking before it can be struck.",
    tier: 1,
    cost: 2,
    baseAttack: 2,
    baseHealth: 2,
    groups: ["echowisp"],
    keywords: [],
    normal: [
      {
        trigger: "before_attack",
        conditions: [{ kind: "source_position_rightmost" }],
        operations: [{ kind: "move_unit", target: { kind: "self" }, to: "leftmost" }],
      },
    ],
    forged: [
      {
        trigger: "before_attack",
        conditions: [{ kind: "source_position_rightmost" }],
        operations: [
          { kind: "move_unit", target: { kind: "self" }, to: "leftmost" },
          { kind: "modify_attack", target: { kind: "self" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 1, health: 2 },
    visualProfile: "vp_echowisp_normal",
    forgedVisualProfile: "vp_echowisp_forged",
    poolCount: 18,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "echo_mirror_initiate",
    name: "Echo Mirror Initiate",
    rulesText: "Trades places with whoever holds the back line the instant battle begins, if it is standing at the front.",
    tier: 1,
    cost: 2,
    baseAttack: 3,
    baseHealth: 1,
    groups: ["echowisp"],
    keywords: [],
    normal: [
      {
        trigger: "combat_start",
        conditions: [{ kind: "source_position_leftmost" }],
        operations: [{ kind: "swap_with", target: { kind: "self" }, other: { kind: "rightmost_friendly" } }],
      },
    ],
    forged: [
      {
        trigger: "combat_start",
        conditions: [{ kind: "source_position_leftmost" }],
        operations: [
          { kind: "swap_with", target: { kind: "self" }, other: { kind: "rightmost_friendly" } },
          { kind: "modify_attack", target: { kind: "self" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 1, health: 2 },
    visualProfile: "vp_echowisp_normal",
    forgedVisualProfile: "vp_echowisp_forged",
    poolCount: 18,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "echo_wisp_dancer",
    name: "Echo Wisp Dancer",
    rulesText: "Strikes from the front, then slips to the back of the line before any reprisal can land.",
    tier: 2,
    cost: 3,
    baseAttack: 3,
    baseHealth: 3,
    groups: ["echowisp"],
    keywords: [],
    normal: [
      {
        trigger: "after_attack",
        conditions: [{ kind: "source_position_leftmost" }],
        operations: [{ kind: "move_unit", target: { kind: "self" }, to: "rightmost" }],
      },
    ],
    forged: [
      {
        trigger: "after_attack",
        conditions: [{ kind: "source_position_leftmost" }],
        operations: [
          { kind: "move_unit", target: { kind: "self" }, to: "rightmost" },
          { kind: "modify_health", target: { kind: "self" }, amount: 2 },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 2 },
    visualProfile: "vp_echowisp_normal",
    forgedVisualProfile: "vp_echowisp_forged",
    poolCount: 15,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "echo_veil_trickster",
    name: "Echo Veil Trickster",
    rulesText: "Reads the enemy's every reinforcement, growing sharper each time the opposing line gains a new face.",
    tier: 2,
    cost: 3,
    baseAttack: 2,
    baseHealth: 4,
    groups: ["echowisp"],
    keywords: [],
    normal: [
      {
        trigger: "on_enemy_summon",
        operations: [{ kind: "modify_attack", target: { kind: "self" }, amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "on_enemy_summon",
        operations: [
          { kind: "modify_attack", target: { kind: "self" }, amount: 2 },
          { kind: "modify_health", target: { kind: "self" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 3 },
    visualProfile: "vp_echowisp_normal",
    forgedVisualProfile: "vp_echowisp_forged",
    poolCount: 15,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "echo_duplicate_weaver",
    name: "Echo Duplicate Weaver",
    rulesText: "Weaves a copy of a neighboring wisp's trick into its own the instant battle begins.",
    tier: 3,
    cost: 4,
    baseAttack: 4,
    baseHealth: 4,
    groups: ["echowisp"],
    keywords: [],
    normal: [
      {
        trigger: "combat_start",
        conditions: [{ kind: "adjacent_in_group", group: "echowisp" }],
        operations: [{ kind: "copy_ability", from: { kind: "adjacent_friendly" } }],
      },
    ],
    forged: [
      {
        trigger: "combat_start",
        conditions: [{ kind: "adjacent_in_group", group: "echowisp" }],
        operations: [
          { kind: "copy_ability", from: { kind: "adjacent_friendly" } },
          { kind: "modify_attack", target: { kind: "self" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 3 },
    visualProfile: "vp_echowisp_normal",
    forgedVisualProfile: "vp_echowisp_forged",
    poolCount: 13,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "echo_riftwalker",
    name: "Echo Riftwalker",
    rulesText: "Once it has weathered a few rounds of the campaign, it rift-steps to the front and strikes with new weight.",
    tier: 4,
    cost: 5,
    baseAttack: 5,
    baseHealth: 5,
    groups: ["echowisp"],
    keywords: [],
    normal: [
      {
        trigger: "before_attack",
        conditions: [{ kind: "round_at_least", value: 3 }],
        operations: [
          { kind: "move_unit", target: { kind: "self" }, to: "leftmost" },
          { kind: "modify_attack", target: { kind: "self" }, amount: 2 },
        ],
      },
    ],
    forged: [
      {
        trigger: "before_attack",
        conditions: [{ kind: "round_at_least", value: 3 }],
        operations: [
          { kind: "move_unit", target: { kind: "self" }, to: "leftmost" },
          { kind: "modify_attack", target: { kind: "self" }, amount: 3 },
          { kind: "grant_keyword", target: { kind: "self" }, keyword: "armored" },
        ],
      },
    ],
    forgedStats: { attack: 3, health: 3 },
    visualProfile: "vp_echowisp_normal",
    forgedVisualProfile: "vp_echowisp_forged",
    poolCount: 11,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "echo_paradox_construct",
    name: "Echo Paradox Construct",
    rulesText: "Stationed at the very back, it throws every blow it takes straight back at whoever landed it — twice over.",
    tier: 5,
    cost: 6,
    baseAttack: 6,
    baseHealth: 6,
    groups: ["echowisp"],
    keywords: [],
    normal: [
      {
        trigger: "on_damage",
        conditions: [{ kind: "source_position_rightmost" }],
        operations: [{ kind: "repeat", times: 2, op: { kind: "deal_damage", target: { kind: "attacker" }, amount: 1 } }],
      },
    ],
    forged: [
      {
        trigger: "on_damage",
        conditions: [{ kind: "source_position_rightmost" }],
        operations: [{ kind: "repeat", times: 3, op: { kind: "deal_damage", target: { kind: "attacker" }, amount: 1 } }],
      },
    ],
    forgedStats: { attack: 3, health: 4 },
    visualProfile: "vp_echowisp_normal",
    forgedVisualProfile: "vp_echowisp_forged",
    poolCount: 9,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "echo_infinite_reflection",
    name: "Echo Infinite Reflection",
    rulesText:
      "Once the illusion-circle numbers three or more, it channels the front-line wisp's whole trick into a single devastating echo.",
    tier: 6,
    cost: 7,
    baseAttack: 7,
    baseHealth: 7,
    groups: ["echowisp"],
    keywords: [],
    normal: [
      {
        trigger: "combat_start",
        conditions: [{ kind: "friendly_group_count_at_least", group: "echowisp", value: 3 }],
        operations: [
          { kind: "copy_ability", from: { kind: "leftmost_friendly" } },
          { kind: "modify_attack", target: { kind: "self" }, amount: 2 },
        ],
      },
    ],
    forged: [
      {
        trigger: "combat_start",
        conditions: [{ kind: "friendly_group_count_at_least", group: "echowisp", value: 3 }],
        operations: [
          { kind: "copy_ability", from: { kind: "leftmost_friendly" } },
          { kind: "modify_attack", target: { kind: "self" }, amount: 3 },
          { kind: "modify_health", target: { kind: "self" }, amount: 2 },
        ],
      },
    ],
    forgedStats: { attack: 4, health: 5 },
    visualProfile: "vp_echowisp_normal",
    forgedVisualProfile: "vp_echowisp_forged",
    poolCount: 7,
    collectible: true,
    contentVersion: 1,
  },
];
