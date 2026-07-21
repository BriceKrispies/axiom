/*
 * catalog.ts — the Figure Lab's view of the real content catalog. It enumerates
 * every collectible card AND token from `LoadedContent`, tags each with its group
 * (neutral = no group), and orders them by group → tier → stable id. Group and
 * card lists come straight from content data (no hardcoded second list), so a new
 * card or group appears in the Lab automatically. Pure/SDK-free.
 */

import type { LoadedContent } from "../../sim/content/load.ts";
import type { CardDefinition } from "../../sim/content/schema.ts";

export type LabGroup = "all" | "ironbound" | "emberkin" | "bloomtide" | "echowisp" | "neutral" | "tokens";

export const LAB_GROUPS: readonly LabGroup[] = ["all", "ironbound", "emberkin", "bloomtide", "echowisp", "neutral", "tokens"];

export const GROUP_LABEL: Readonly<Record<LabGroup, string>> = {
  all: "All",
  ironbound: "Ironbound",
  emberkin: "Emberkin",
  bloomtide: "Bloomtide",
  echowisp: "Echowisp",
  neutral: "Neutral",
  tokens: "Tokens",
};

export interface CatalogEntry {
  readonly card: CardDefinition;
  readonly group: string;
  readonly token: boolean;
}

const GROUP_ORDER: Readonly<Record<string, number>> = { ironbound: 0, emberkin: 1, bloomtide: 2, echowisp: 3, neutral: 4 };

/** The group label a card belongs to (first group, or "neutral"). */
export const groupOfCard = (card: CardDefinition): string => card.groups[0] ?? "neutral";

/** Every card + token, ordered group → tier → id. */
export const buildCatalog = (content: LoadedContent): CatalogEntry[] =>
  content.cards
    .map((card) => ({ card, group: groupOfCard(card), token: !card.collectible }))
    .sort(
      (a, b) =>
        (GROUP_ORDER[a.group] ?? 9) - (GROUP_ORDER[b.group] ?? 9) ||
        a.card.tier - b.card.tier ||
        (a.card.id < b.card.id ? -1 : a.card.id > b.card.id ? 1 : 0),
    );

/** Filter the catalog by a lab group selector. */
export const filterCatalog = (catalog: readonly CatalogEntry[], group: LabGroup): CatalogEntry[] => {
  if (group === "all") {
    return catalog.filter((e) => !e.token);
  }
  if (group === "tokens") {
    return catalog.filter((e) => e.token);
  }
  return catalog.filter((e) => !e.token && e.group === group);
};
