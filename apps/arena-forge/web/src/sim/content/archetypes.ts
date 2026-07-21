/*
 * archetypes.ts — the strategic identity each group expresses, plus a
 * `neutral` entry for the four groupless cards. Purely descriptive: an
 * archetype carries no rules of its own, only the prose the UI surfaces
 * alongside a group's cards.
 */

import type { ArchetypeDefinition } from "./schema.ts";

export const ARCHETYPES: readonly ArchetypeDefinition[] = [
  {
    id: "formation",
    name: "Formation",
    description:
      "Ironbound identity: adjacency bonuses, armored bulk, and deliberate positional drilling that rewards a disciplined line.",
  },
  {
    id: "aggression",
    name: "Aggression",
    description:
      "Emberkin identity: attack that compounds with every swing, and risk-reward self-damage traded for burst power.",
  },
  {
    id: "swarm",
    name: "Swarm",
    description:
      "Bloomtide identity: cheap tokens that widen the board, growing stronger the more the grove is populated.",
  },
  {
    id: "trickery",
    name: "Trickery",
    description:
      "Echowisp identity: repositioning, borrowed abilities, and misdirection that turns board order into an advantage.",
  },
  {
    id: "neutral",
    name: "Neutral",
    description: "No shared tribal identity: flexible economy and stat support that slots into any warband.",
  },
];
