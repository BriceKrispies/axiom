/*
 * cards/tokens.ts — non-collectible token cards summoned by ability
 * operations (currently only Bloomtide's `summon_token`). Tokens carry no
 * abilities of their own (so they can never form a summon cycle), never
 * appear in a shop (`collectible: false`, `poolCount: 0`), and never forge
 * (`forgedStats` is zeroed and `forged` is empty).
 */

import type { CardDefinition } from "../schema.ts";

export const TOKEN_CARDS: readonly CardDefinition[] = [
  {
    id: "bloom_sprout",
    name: "Bloom Sprout",
    rulesText: "A single root-sprout, cast quickly and cheaply by the grove's larger constructs.",
    tier: 1,
    cost: 0,
    baseAttack: 1,
    baseHealth: 1,
    groups: ["bloomtide"],
    keywords: [],
    normal: [],
    forged: [],
    forgedStats: { attack: 0, health: 0 },
    visualProfile: "vp_bloomtide_normal",
    forgedVisualProfile: "vp_bloomtide_forged",
    poolCount: 0,
    collectible: false,
    contentVersion: 1,
  },
  {
    id: "bloom_seedling",
    name: "Bloom Seedling",
    rulesText: "A sturdier growth, seeded by the grove's oldest constructs when they finally fall.",
    tier: 3,
    cost: 0,
    baseAttack: 3,
    baseHealth: 3,
    groups: ["bloomtide"],
    keywords: [],
    normal: [],
    forged: [],
    forgedStats: { attack: 0, health: 0 },
    visualProfile: "vp_bloomtide_normal",
    forgedVisualProfile: "vp_bloomtide_forged",
    poolCount: 0,
    collectible: false,
    contentVersion: 1,
  },
];
