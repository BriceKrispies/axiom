import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "../../sim/content/bundle.ts";
import { buildCatalog, filterCatalog, groupOfCard } from "./catalog.ts";

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
