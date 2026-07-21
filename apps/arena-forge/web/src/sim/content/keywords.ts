/*
 * keywords.ts — the only two keywords the combat engine gives mechanical
 * meaning: `guard` (the defender must target guard units before others) and
 * `armored` (reduces each incoming hit by 1, floored at 0). Every other unit
 * identity in this content set comes from authored abilities, never from an
 * invented keyword.
 */

import type { KeywordDefinition } from "./schema.ts";

export const KEYWORDS: readonly KeywordDefinition[] = [
  {
    id: "guard",
    name: "Guard",
    description: "Enemies must target this unit before any non-guard ally while it lives.",
  },
  {
    id: "armored",
    name: "Armored",
    description: "Each incoming hit against this unit is reduced by 1, floored at 0.",
  },
];
