/*
 * catalog.ts — the Figure Lab's view of the real content catalog. It enumerates
 * every collectible card AND token from `LoadedContent`, tags each with its group
 * (neutral = no group), and orders them by group → tier → stable id. Group and
 * card lists come straight from content data (no hardcoded second list), so a new
 * card or group appears in the Lab automatically. Pure/SDK-free.
 *
 * It also owns the gallery QUERY: the group filter, the free-text search, and the
 * sort order behind the Lab's all-figures gallery. `queryCatalog` is a pure
 * function of (catalog, query) so the gallery's contents are unit-testable with no
 * DOM, no renderer, and no screen.
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

// ── gallery query: search + sort ───────────────────────────────────────────────

export type SortMode = "tribe" | "name" | "tier" | "attack" | "health";

export const SORT_MODES: readonly SortMode[] = ["tribe", "name", "tier", "attack", "health"];

export const SORT_LABEL: Readonly<Record<SortMode, string>> = {
  tribe: "TRIBE",
  name: "NAME",
  tier: "TIER",
  attack: "ATK",
  health: "HP",
};

export interface GalleryQuery {
  readonly group: LabGroup;
  /** Free text; matched case-insensitively against name, id, tribe and keywords. */
  readonly search: string;
  readonly sort: SortMode;
}

/** The searchable text of an entry (name, id, tribe label, keywords, rules text). */
const haystack = (entry: CatalogEntry): string =>
  `${entry.card.name} ${entry.card.id} ${entry.group} ${entry.card.keywords.join(" ")} ${entry.card.rulesText}`.toLowerCase();

/** Every whitespace-separated term must appear somewhere in the entry's text. */
export const matchesSearch = (entry: CatalogEntry, search: string): boolean => {
  const terms = search.toLowerCase().split(/\s+/u).filter((t) => t.length > 0);
  const text = haystack(entry);
  return terms.every((t) => text.includes(t));
};

const byName = (a: CatalogEntry, b: CatalogEntry): number => (a.card.name < b.card.name ? -1 : a.card.name > b.card.name ? 1 : 0);

const tribeThen = (a: CatalogEntry, b: CatalogEntry): number =>
  (GROUP_ORDER[a.group] ?? 9) - (GROUP_ORDER[b.group] ?? 9) || a.card.tier - b.card.tier || byName(a, b);

const COMPARATORS: Readonly<Record<SortMode, (a: CatalogEntry, b: CatalogEntry) => number>> = {
  tribe: tribeThen,
  name: byName,
  tier: (a, b) => a.card.tier - b.card.tier || tribeThen(a, b),
  attack: (a, b) => b.card.baseAttack - a.card.baseAttack || byName(a, b),
  health: (a, b) => b.card.baseHealth - a.card.baseHealth || byName(a, b),
};

/**
 * The gallery's contents: the group filter, then the search, then the sort. Pure
 * and stable — equal queries always produce the same array of the same entries.
 */
export const queryCatalog = (catalog: readonly CatalogEntry[], query: GalleryQuery): CatalogEntry[] =>
  filterCatalog(catalog, query.group)
    .filter((e) => matchesSearch(e, query.search))
    .sort(COMPARATORS[query.sort]);

/**
 * The section label a sorted entry belongs under, or `null` when the active sort
 * has no sections. Only the TRIBE sort is sectioned — that is the ordering the
 * gallery exists to show off.
 */
export const sectionOf = (entry: CatalogEntry, sort: SortMode): string | null =>
  sort === "tribe" ? (GROUP_LABEL[entry.group as LabGroup] ?? "Neutral") : null;
