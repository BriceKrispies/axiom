/*
 * cards/bloomtide.ts — the Swarm tribe. Identity: cheap `summon_token`
 * sprouts that widen the board, and units that grow whenever the grove
 * gains a new member. Eight cards spanning tier 1 (scouts that seed a
 * single sprout) through tier 6 (a build-around that showers sprouts once
 * the grove is wide enough). Token ids referenced here are defined in
 * `tokens.ts`.
 */

import type { CardDefinition } from "../schema.ts";

export const BLOOMTIDE_CARDS: readonly CardDefinition[] = [
  {
    id: "bloom_seed_scout",
    name: "Bloom Seed Scout",
    rulesText: "Casts a single root-sprout at the dawn of battle, widening the grove before the first blow lands.",
    tier: 1,
    cost: 2,
    baseAttack: 2,
    baseHealth: 2,
    groups: ["bloomtide"],
    keywords: [],
    normal: [
      {
        trigger: "combat_start",
        operations: [{ kind: "summon_token", token: "bloom_sprout", at: { kind: "empty_friendly_slot" }, count: 1 }],
      },
    ],
    forged: [
      {
        trigger: "combat_start",
        operations: [{ kind: "summon_token", token: "bloom_sprout", at: { kind: "empty_friendly_slot" }, count: 2 }],
      },
    ],
    forgedStats: { attack: 1, health: 2 },
    tokens: ["bloom_sprout"],
    visualProfile: "vp_bloomtide_normal",
    forgedVisualProfile: "vp_bloomtide_forged",
    poolCount: 18,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "bloom_pod_tender",
    name: "Bloom Pod Tender",
    rulesText: "A frail root-warden that seeds a sprout the instant it falls, so the grove outlives it.",
    tier: 1,
    cost: 2,
    baseAttack: 1,
    baseHealth: 3,
    groups: ["bloomtide"],
    keywords: [],
    normal: [
      {
        trigger: "on_death",
        operations: [{ kind: "summon_token", token: "bloom_sprout", at: { kind: "empty_friendly_slot" }, count: 1 }],
      },
    ],
    forged: [
      {
        trigger: "on_death",
        operations: [{ kind: "summon_token", token: "bloom_sprout", at: { kind: "empty_friendly_slot" }, count: 2 }],
      },
    ],
    forgedStats: { attack: 1, health: 2 },
    tokens: ["bloom_sprout"],
    visualProfile: "vp_bloomtide_normal",
    forgedVisualProfile: "vp_bloomtide_forged",
    poolCount: 18,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "bloom_vine_warden",
    name: "Bloom Vine Warden",
    rulesText: "Its vines lash tighter every time the grove gains a new sprout, thickening in step with the swarm.",
    tier: 2,
    cost: 3,
    baseAttack: 3,
    baseHealth: 3,
    groups: ["bloomtide"],
    keywords: [],
    normal: [
      {
        trigger: "on_friendly_summon",
        operations: [
          { kind: "modify_attack", target: { kind: "self" }, amount: 1 },
          { kind: "modify_health", target: { kind: "self" }, amount: 1 },
        ],
      },
    ],
    forged: [
      {
        trigger: "on_friendly_summon",
        operations: [
          { kind: "modify_attack", target: { kind: "self" }, amount: 2 },
          { kind: "modify_health", target: { kind: "self" }, amount: 2 },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 2 },
    visualProfile: "vp_bloomtide_normal",
    forgedVisualProfile: "vp_bloomtide_forged",
    poolCount: 15,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "bloom_thorned_guard",
    name: "Bloom Thorned Guard",
    rulesText: "A bramble-plated sentinel that shields the whole grove for as long as it stands in the fight.",
    tier: 2,
    cost: 3,
    baseAttack: 2,
    baseHealth: 5,
    groups: ["bloomtide"],
    keywords: ["guard"],
    normal: [
      {
        trigger: "passive_aura",
        operations: [{ kind: "modify_health", target: { kind: "friendly_in_group", group: "bloomtide" }, amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "passive_aura",
        operations: [
          { kind: "modify_health", target: { kind: "friendly_in_group", group: "bloomtide" }, amount: 2 },
          { kind: "modify_attack", target: { kind: "friendly_in_group", group: "bloomtide" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 3 },
    visualProfile: "vp_bloomtide_normal",
    forgedVisualProfile: "vp_bloomtide_forged",
    poolCount: 15,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "bloom_canopy_shaper",
    name: "Bloom Canopy Shaper",
    rulesText: "Bends the canopy over its neighbors every time the grove grows, so long as it is still standing.",
    tier: 3,
    cost: 4,
    baseAttack: 4,
    baseHealth: 5,
    groups: ["bloomtide"],
    keywords: [],
    normal: [
      {
        trigger: "on_friendly_summon",
        conditions: [{ kind: "source_health_at_least", value: 1 }],
        operations: [{ kind: "modify_health", target: { kind: "adjacent_friendly" }, amount: 1 }],
      },
    ],
    forged: [
      {
        trigger: "on_friendly_summon",
        conditions: [{ kind: "source_health_at_least", value: 1 }],
        operations: [
          { kind: "modify_health", target: { kind: "adjacent_friendly" }, amount: 2 },
          { kind: "modify_attack", target: { kind: "adjacent_friendly" }, amount: 1 },
        ],
      },
    ],
    forgedStats: { attack: 2, health: 3 },
    visualProfile: "vp_bloomtide_normal",
    forgedVisualProfile: "vp_bloomtide_forged",
    poolCount: 13,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "bloom_root_matriarch",
    name: "Bloom Root Matriarch",
    rulesText: "Once the grove has spread wide enough, she calls up reinforcements mid-battle before her next strike.",
    tier: 4,
    cost: 5,
    baseAttack: 5,
    baseHealth: 7,
    groups: ["bloomtide"],
    keywords: [],
    normal: [
      {
        trigger: "before_attack",
        conditions: [{ kind: "friendly_group_count_at_least", group: "bloomtide", value: 3 }],
        operations: [{ kind: "summon_token", token: "bloom_sprout", at: { kind: "empty_friendly_slot" }, count: 1 }],
      },
    ],
    forged: [
      {
        trigger: "before_attack",
        conditions: [{ kind: "friendly_group_count_at_least", group: "bloomtide", value: 3 }],
        operations: [{ kind: "summon_token", token: "bloom_sprout", at: { kind: "empty_friendly_slot" }, count: 2 }],
      },
    ],
    forgedStats: { attack: 3, health: 3 },
    tokens: ["bloom_sprout"],
    visualProfile: "vp_bloomtide_normal",
    forgedVisualProfile: "vp_bloomtide_forged",
    poolCount: 11,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "bloom_evergreen_colossus",
    name: "Bloom Evergreen Colossus",
    rulesText: "An ancient root-titan. When it finally falls, its trunk splits into a pair of sturdy seedlings.",
    tier: 5,
    cost: 6,
    baseAttack: 6,
    baseHealth: 9,
    groups: ["bloomtide"],
    keywords: [],
    normal: [
      {
        trigger: "on_death",
        operations: [{ kind: "summon_token", token: "bloom_seedling", at: { kind: "empty_friendly_slot" }, count: 2 }],
      },
    ],
    forged: [
      {
        trigger: "on_death",
        operations: [{ kind: "summon_token", token: "bloom_seedling", at: { kind: "empty_friendly_slot" }, count: 3 }],
      },
    ],
    forgedStats: { attack: 3, health: 4 },
    tokens: ["bloom_seedling"],
    visualProfile: "vp_bloomtide_normal",
    forgedVisualProfile: "vp_bloomtide_forged",
    poolCount: 9,
    collectible: true,
    contentVersion: 1,
  },
  {
    id: "bloom_worldroot_avatar",
    name: "Bloom Worldroot Avatar",
    rulesText:
      "The grove's living heart. Once four Bloomtide constructs stand together, it showers the field with sprouts as battle begins.",
    tier: 6,
    cost: 7,
    baseAttack: 7,
    baseHealth: 10,
    groups: ["bloomtide"],
    keywords: [],
    normal: [
      {
        trigger: "combat_start",
        conditions: [{ kind: "friendly_group_count_at_least", group: "bloomtide", value: 4 }],
        operations: [
          {
            kind: "repeat",
            times: 3,
            op: { kind: "summon_token", token: "bloom_sprout", at: { kind: "empty_friendly_slot" }, count: 1 },
          },
        ],
      },
    ],
    forged: [
      {
        trigger: "combat_start",
        conditions: [{ kind: "friendly_group_count_at_least", group: "bloomtide", value: 4 }],
        operations: [
          {
            kind: "repeat",
            times: 3,
            op: { kind: "summon_token", token: "bloom_seedling", at: { kind: "empty_friendly_slot" }, count: 1 },
          },
        ],
      },
    ],
    forgedStats: { attack: 4, health: 5 },
    tokens: ["bloom_sprout", "bloom_seedling"],
    visualProfile: "vp_bloomtide_normal",
    forgedVisualProfile: "vp_bloomtide_forged",
    poolCount: 7,
    collectible: true,
    contentVersion: 1,
  },
];
