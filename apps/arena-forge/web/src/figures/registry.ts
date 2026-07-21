/*
 * registry.ts — maps any card id to its procedural `FigureDefinition`, built from
 * the card's group + tier via the body-plan grammar and memoized. Every collectible
 * card and token therefore has a real, group-coherent, seed-varied figure (never a
 * placeholder). Bespoke per-card overrides can be registered here later to take
 * precedence over the procedural default. Pure/SDK-free (reads `LoadedContent`).
 */

import type { LoadedContent } from "../sim/content/load.ts";
import type { CardId } from "../sim/ids.ts";
import type { FigureDefinition } from "./grammar.ts";
import { buildFigureDefinition } from "./bodyplans.ts";

type Group = "ironbound" | "emberkin" | "bloomtide" | "echowisp" | "neutral";
const GROUPS: readonly Group[] = ["ironbound", "emberkin", "bloomtide", "echowisp", "neutral"];

const groupOf = (content: LoadedContent, cardId: CardId): Group => {
  const first = content.card(cardId).groups[0];
  return first !== undefined && (GROUPS as readonly string[]).includes(first) ? (first as Group) : "neutral";
};

const cache = new Map<CardId, FigureDefinition>();

/** Bespoke overrides win over the procedural grammar (none yet — extension point). */
const OVERRIDES = new Map<CardId, (content: LoadedContent) => FigureDefinition>();

export const registerFigureOverride = (cardId: CardId, build: (content: LoadedContent) => FigureDefinition): void => {
  OVERRIDES.set(cardId, build);
};

export const figureForCard = (content: LoadedContent, cardId: CardId): FigureDefinition => {
  const hit = cache.get(cardId);
  if (hit !== undefined) {
    return hit;
  }
  const override = OVERRIDES.get(cardId);
  const def = override ? override(content) : buildFigureDefinition(cardId, groupOf(content, cardId), content.card(cardId).tier, 0x9e37);
  cache.set(cardId, def);
  return def;
};

/** Every card + token has a figure — the coverage guarantee (used by tests). */
export const allCardFigures = (content: LoadedContent): FigureDefinition[] => content.cards.map((c) => figureForCard(content, c.id));
