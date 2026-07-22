import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "../../sim/content/bundle.ts";
import type { CatalogEntry } from "./catalog.ts";
import { buildCatalog, filterCatalog, groupOfCard, queryCatalog, sectionOf } from "./catalog.ts";

const content = loadDefaultContent();
const catalog = buildCatalog(content);

test("the catalog contains every card and token", () => {
  assert.equal(catalog.length, content.cards.length);
});

test("group filtering returns the right cards (8 each, 4 neutral, 2 tokens)", () => {
  for (const g of ["ironbound", "emberkin", "bloomtide", "echowisp"] as const) {
    assert.equal(filterCatalog(catalog, g).length, 8, `${g} should have 8`);
  }
  assert.equal(filterCatalog(catalog, "neutral").length, 4);
  assert.equal(filterCatalog(catalog, "tokens").length, 2);
  assert.equal(filterCatalog(catalog, "all").length, 36, "all = 36 collectibles (tokens excluded)");
});

test("catalog order is stable: group then tier then id", () => {
  const a = buildCatalog(content).map((e) => e.card.id);
  const b = buildCatalog(content).map((e) => e.card.id);
  assert.deepEqual(a, b);
  const iron = filterCatalog(catalog, "ironbound");
  for (let i = 1; i < iron.length; i += 1) {
    assert.ok((iron[i - 1] as { card: { tier: number } }).card.tier <= (iron[i] as { card: { tier: number } }).card.tier, "tier non-decreasing");
  }
});

test("neutral cards have no group and map to the neutral label", () => {
  const neutral = filterCatalog(catalog, "neutral");
  for (const e of neutral) {
    assert.equal(groupOfCard(e.card), "neutral");
    assert.deepEqual(e.card.groups, []);
  }
});

test("tokens are non-collectible", () => {
  for (const e of filterCatalog(catalog, "tokens")) {
    assert.equal(e.card.collectible, false);
  }
});

// ── gallery query: search + sort ───────────────────────────────────────────────

test("search matches name, id, tribe and keywords, case-insensitively", () => {
  const first = catalog[0] as { card: { name: string } };
  const byName = queryCatalog(catalog, { group: "all", search: first.card.name.toUpperCase(), sort: "name" });
  assert.ok(byName.some((e) => e.card.name === first.card.name), "found by its own name");
  const byTribe = queryCatalog(catalog, { group: "all", search: "ironbound", sort: "name" });
  assert.equal(byTribe.length, 8, "the tribe name finds the whole tribe");
});

test("multi-term search requires every term", () => {
  const both = queryCatalog(catalog, { group: "all", search: "ironbound zzzznotathing", sort: "name" });
  assert.equal(both.length, 0);
});

test("an empty search returns the whole filtered group", () => {
  assert.equal(queryCatalog(catalog, { group: "all", search: "", sort: "tribe" }).length, 36);
  assert.equal(queryCatalog(catalog, { group: "all", search: "   ", sort: "tribe" }).length, 36);
});

test("each sort mode orders the gallery as advertised", () => {
  const named = queryCatalog(catalog, { group: "all", search: "", sort: "name" });
  for (let i = 1; i < named.length; i += 1) {
    assert.ok((named[i - 1] as CatalogEntry).card.name <= (named[i] as CatalogEntry).card.name, "name ascending");
  }
  const tiered = queryCatalog(catalog, { group: "all", search: "", sort: "tier" });
  for (let i = 1; i < tiered.length; i += 1) {
    assert.ok((tiered[i - 1] as CatalogEntry).card.tier <= (tiered[i] as CatalogEntry).card.tier, "tier ascending");
  }
  const atk = queryCatalog(catalog, { group: "all", search: "", sort: "attack" });
  for (let i = 1; i < atk.length; i += 1) {
    assert.ok((atk[i - 1] as CatalogEntry).card.baseAttack >= (atk[i] as CatalogEntry).card.baseAttack, "attack descending");
  }
  const hp = queryCatalog(catalog, { group: "all", search: "", sort: "health" });
  for (let i = 1; i < hp.length; i += 1) {
    assert.ok((hp[i - 1] as CatalogEntry).card.baseHealth >= (hp[i] as CatalogEntry).card.baseHealth, "health descending");
  }
});

test("the tribe sort groups every tribe into one contiguous run", () => {
  const sorted = queryCatalog(catalog, { group: "all", search: "", sort: "tribe" });
  const runs: string[] = [];
  for (const e of sorted) {
    if (runs[runs.length - 1] !== e.group) {
      runs.push(e.group);
    }
  }
  assert.deepEqual(runs, [...new Set(runs)], "no tribe appears in two separate runs");
  assert.deepEqual(runs, ["ironbound", "emberkin", "bloomtide", "echowisp", "neutral"]);
});

test("only the tribe sort is sectioned", () => {
  const e = catalog[0] as CatalogEntry;
  assert.equal(sectionOf(e, "tribe"), "Ironbound");
  assert.equal(sectionOf(e, "name"), null);
});

test("querying is pure and stable", () => {
  const q = { group: "all", search: "e", sort: "tribe" } as const;
  assert.deepEqual(queryCatalog(catalog, q).map((e) => e.card.id), queryCatalog(catalog, q).map((e) => e.card.id));
  assert.equal(catalog.length, content.cards.length, "the source catalog is never mutated");
});
