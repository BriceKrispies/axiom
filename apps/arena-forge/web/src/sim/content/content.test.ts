/*
 * content.test.ts — validates the authored Arena Forge content set: the
 * bundle passes `validateContent` with zero errors, and the roster shape
 * matches the spec (36 collectible cards: 8 per group × 4 groups, plus 4
 * groupless neutrals).
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { CONTENT } from "./bundle.ts";
import { validateContent } from "./validate.ts";

test("content bundle passes validation", () => {
  const errors = validateContent(CONTENT);
  assert.deepEqual(errors, [], `content validation errors:\n  ${errors.join("\n  ")}`);
});

test("exactly 36 collectible cards", () => {
  const collectible = CONTENT.cards.filter((c) => c.collectible);
  assert.equal(collectible.length, 36);
});

test("exactly 4 groups", () => {
  assert.equal(CONTENT.groups.length, 4);
});

test("each group has exactly 8 collectible members", () => {
  for (const group of CONTENT.groups) {
    const members = CONTENT.cards.filter((c) => c.collectible && c.groups.includes(group.id));
    assert.equal(members.length, 8, `group '${group.id}' has ${members.length} collectible members, expected 8`);
  }
});

test("exactly 4 neutral collectible cards (belonging to no group)", () => {
  const neutrals = CONTENT.cards.filter((c) => c.collectible && c.groups.length === 0);
  assert.equal(neutrals.length, 4);
});
