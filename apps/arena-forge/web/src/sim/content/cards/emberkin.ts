/*
 * cards/emberkin.ts — the Aggression tribe. Identity: attack that compounds
 * every time it swings or gets hit, and deliberate self-damage traded for
 * burst power. Eight cards spanning tier 1 (a stoker that grows with every
 * attack) through tier 6 (a build-around that heats the whole warband as its
 * champions strike).
 */

import type { CardDefinition } from "../schema.ts";

export const EMBERKIN_CARDS: readonly CardDefinition[] = [
  {
    id: "ember_stoker",
    name: "Ember Stoker",
    rulesText: "A kindling-blooded duelist. Every swing feeds its own fire, so its attack climbs the longer it fights.",
    tier: 1,
    cost: 2,
    baseAttack: 3,
    baseHealth: 2,
    groups: ["emberkin"],
    keywords: [],
    normal: [
      {
        trigger: "after_attack",
        operations: [{ kind: "modify_attack", target: { kind: "self" }, amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "after_attack",
        operations: [
          { kind: "modify_attack", target: { kind: "self" }, amount: 2 },
          { kind: "modify_health", target: { kind: "self" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 1, health: 2 },
    visualProfile: "vp_emberkin_normal",
    forgedVisualProfile: "vp_emberkin_forged",
    poolCount: 18,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "ember_hotblood",
    name: "Ember Hotblood",
    rulesText:
      "Cuts itself open at the start of the fight to burn hotter. The wound costs it health, but the flare is worth it.",
    tier: 1,
    cost: 2,
    baseAttack: 2,
    baseHealth: 2,
    groups: ["emberkin"],
    keywords: [],
    normal: [
      {
        trigger: "combat_start",
        operations: [
          { kind: "deal_damage", target: { kind: "self" }, amount: 1 },
          { kind: "modify_attack", target: { kind: "self" }, amount: 2 },
        ],
      },
    ],
    forged: [
      {
        trigger: "combat_start",
        operations: [
          { kind: "deal_damage", target: { kind: "self" }, amount: 1 },
          { kind: "modify_attack", target: { kind: "self" }, amount: 4 },
        ],
      },
    ],
    forgedStats: { attack: 1, health: 2 },
    visualProfile: "vp_emberkin_normal",
    forgedVisualProfile: "vp_emberkin_forged",
    poolCount: 18,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "ember_duelist",
    name: "Ember Duelist",
    rulesText: "The pain of a landed blow only sharpens its focus — every hit taken makes its next swing harder.",
    tier: 2,
    cost: 3,
    baseAttack: 4,
    baseHealth: 3,
    groups: ["emberkin"],
    keywords: [],
    normal: [
      {
        trigger: "on_damage",
        operations: [{ kind: "modify_attack", target: { kind: "self" }, amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "on_damage",
        operations: [
          { kind: "modify_attack", target: { kind: "self" }, amount: 2 },
          { kind: "modify_health", target: { kind: "self" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 2 },
    visualProfile: "vp_emberkin_normal",
    forgedVisualProfile: "vp_emberkin_forged",
    poolCount: 15,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "ember_pyroclast",
    name: "Ember Pyroclast",
    rulesText:
      "Once its own flame burns hot enough, it hurls molten sparks at the enemy line before it even swings.",
    tier: 2,
    cost: 3,
    baseAttack: 3,
    baseHealth: 3,
    groups: ["emberkin"],
    keywords: [],
    normal: [
      {
        trigger: "before_attack",
        conditions: [{ kind: "source_attack_at_least", value: 5 }],
        operations: [{ kind: "repeat", times: 2, op: { kind: "deal_damage", target: { kind: "random_enemy" }, amount: 1 } }],
      },
    ],
    forged: [
      {
        trigger: "before_attack",
        conditions: [{ kind: "source_attack_at_least", value: 5 }],
        operations: [{ kind: "repeat", times: 2, op: { kind: "deal_damage", target: { kind: "random_enemy" }, amount: 2 } }],
      },
    ],
    forgedStats: { attack: 2, health: 2 },
    visualProfile: "vp_emberkin_normal",
    forgedVisualProfile: "vp_emberkin_forged",
    poolCount: 15,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "ember_warbrand",
    name: "Ember Warbrand",
    rulesText: "Shrugs off a blow that should have ended it, and comes back swinging harder and steadier.",
    tier: 3,
    cost: 4,
    baseAttack: 5,
    baseHealth: 4,
    groups: ["emberkin"],
    keywords: [],
    normal: [
      {
        trigger: "on_survive_damage",
        operations: [
          { kind: "modify_attack", target: { kind: "self" }, amount: 2 },
          { kind: "modify_health", target: { kind: "self" }, amount: 1 },
        ],
      },
    ],
    forged: [
      {
        trigger: "on_survive_damage",
        operations: [
          { kind: "modify_attack", target: { kind: "self" }, amount: 3 },
          { kind: "modify_health", target: { kind: "self" }, amount: 2 },
          { kind: "grant_keyword", target: { kind: "self" }, keyword: "armored" },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 3 },
    visualProfile: "vp_emberkin_normal",
    forgedVisualProfile: "vp_emberkin_forged",
    poolCount: 13,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "ember_ashbringer",
    name: "Ember Ashbringer",
    rulesText: "Once its own blaze has grown large enough, it hunts the strongest foe on the field and burns it down.",
    tier: 4,
    cost: 5,
    baseAttack: 6,
    baseHealth: 5,
    groups: ["emberkin"],
    keywords: [],
    normal: [
      {
        trigger: "after_attack",
        conditions: [{ kind: "source_attack_at_least", value: 8 }],
        operations: [{ kind: "deal_damage", target: { kind: "highest_attack_enemy" }, amount: 3 }],
      },
    ],
    forged: [
      {
        trigger: "after_attack",
        conditions: [{ kind: "source_attack_at_least", value: 8 }],
        operations: [{ kind: "deal_damage", target: { kind: "highest_attack_enemy" }, amount: 5 }],
      },
    ],
    forgedStats: { attack: 3, health: 3 },
    visualProfile: "vp_emberkin_normal",
    forgedVisualProfile: "vp_emberkin_forged",
    poolCount: 11,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "ember_infernal_champion",
    name: "Ember Infernal Champion",
    rulesText: "Opens the fight by burning its own flesh for fuel, trading a wound for an enormous surge of power.",
    tier: 5,
    cost: 6,
    baseAttack: 7,
    baseHealth: 6,
    groups: ["emberkin"],
    keywords: [],
    normal: [
      {
        trigger: "combat_start",
        operations: [
          { kind: "deal_damage", target: { kind: "self" }, amount: 2 },
          { kind: "modify_attack", target: { kind: "self" }, amount: 5 },
        ],
      },
    ],
    forged: [
      {
        trigger: "combat_start",
        operations: [
          { kind: "deal_damage", target: { kind: "self" }, amount: 2 },
          { kind: "modify_attack", target: { kind: "self" }, amount: 8 },
          { kind: "grant_keyword", target: { kind: "self" }, keyword: "armored" },
        ],
      },
    ],
    forgedStats: { attack: 3, health: 4 },
    visualProfile: "vp_emberkin_normal",
    forgedVisualProfile: "vp_emberkin_forged",
    poolCount: 9,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "ember_pyrarch_ascendant",
    name: "Ember Pyrarch Ascendant",
    rulesText:
      "The Emberkin's living bonfire. Once three of its kin fight beside it, every one of its strikes fans the whole warband's flame.",
    tier: 6,
    cost: 7,
    baseAttack: 8,
    baseHealth: 7,
    groups: ["emberkin"],
    keywords: [],
    normal: [
      {
        trigger: "after_attack",
        conditions: [{ kind: "friendly_group_count_at_least", group: "emberkin", value: 3 }],
        operations: [{ kind: "modify_attack", target: { kind: "all_friendly" }, amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "after_attack",
        conditions: [{ kind: "friendly_group_count_at_least", group: "emberkin", value: 3 }],
        operations: [
          { kind: "modify_attack", target: { kind: "all_friendly" }, amount: 2 },
          { kind: "modify_health", target: { kind: "all_friendly" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 4, health: 5 },
    visualProfile: "vp_emberkin_normal",
    forgedVisualProfile: "vp_emberkin_forged",
    poolCount: 7,
    collectible: true,
    contentVersion: 1,
  },
];
