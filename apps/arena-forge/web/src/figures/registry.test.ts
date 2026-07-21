import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "../sim/content/bundle.ts";
import { languageFor } from "./languages/index.ts";
import { expandFigure } from "./generator.ts";
import { allCardFigures, figureForCard } from "./registry.ts";

const content = loadDefaultContent();

test("every collectible card and token has a figure definition", () => {
  const figs = allCardFigures(content);
  assert.equal(figs.length, content.cards.length);
  for (const card of content.cards) {
    const fig = figureForCard(content, card.id);
    assert.ok(fig.parts.length > 0, `${card.id} has no parts`);
  }
});

test("every figure's part hierarchy is acyclic and parent-before-child", () => {
  for (const card of content.cards) {
    const fig = figureForCard(content, card.id);
    const seen = new Set<string>();
    for (const part of fig.parts) {
      if (part.parent !== null) {
        assert.ok(seen.has(part.parent), `${card.id}: part ${part.id} references forward/missing parent ${part.parent}`);
      }
      seen.add(part.id);
    }
  }
});

test("figure generation is deterministic (same card ⇒ identical expanded parts)", () => {
  for (const card of content.cards.slice(0, 12)) {
    const a = expandFigure(figureForCard(content, card.id), "med", false);
    const b = expandFigure(figureForCard(content, card.id), "med", false);
    assert.deepEqual(a.parts, b.parts, `${card.id} not deterministic`);
  }
});

test("group-mates share their group visual language; cards differ by seed", () => {
  const iron = content.cards.filter((c) => c.groups[0] === "ironbound");
  const langs = new Set(iron.map((c) => figureForCard(content, c.id).language));
  assert.deepEqual([...langs], ["ironbound"]);
  // Distinctness: at least two ironbound cards differ in part count or weapon.
  const shapes = new Set(iron.map((c) => JSON.stringify(figureForCard(content, c.id).parts.map((p) => `${p.tag}:${p.primitive}`))));
  assert.ok(shapes.size >= 2, "ironbound cards should not all be identical");
});

test("forged figures add augmentation parts within a bounded budget", () => {
  for (const card of content.cards.slice(0, 8)) {
    const def = figureForCard(content, card.id);
    const normal = expandFigure(def, "high", false).parts.length;
    const forged = expandFigure(def, "high", true).parts.length;
    assert.ok(forged >= normal, `${card.id}: forged should not shrink`);
    assert.ok(forged <= 40, `${card.id}: forged part count ${forged} exceeds budget`);
  }
});

test("every language covers the material roles its palette needs", () => {
  for (const card of content.cards) {
    const def = figureForCard(content, card.id);
    const lang = languageFor(def.language);
    for (const part of def.parts) {
      assert.ok(lang.palette[part.material] !== undefined, `${def.language} missing role ${part.material}`);
    }
  }
});
