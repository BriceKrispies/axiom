/*
 * active-catalog.test.ts — regression guardrails for the arcade-cabinet
 * redesign: the thirteen retired games stay OUT of the repository (their
 * source directories were deleted, not merely disabled), the active count
 * doesn't drift silently, every active game keeps a distinct arcade identity
 * (glyph), and the retired modern-dashboard components (card grid, pill
 * buttons, glass panels) don't creep back into the source that replaced
 * them.
 */

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { ALL_GAMES } from "../games/index.ts";

const WEB_ROOT = join(import.meta.dirname, "..", "..");
const GAMES_ROOT = join(WEB_ROOT, "src", "games");

/** The thirteen games retired from the original twenty-game catalog. Their
 * source directories were deleted outright — never re-add these without an
 * explicit product decision. */
const INACTIVE_IDS = [
  "prize-drop",
  "card-flip",
  "mystery-doors",
  "ball-machine",
  "safe-cracker",
  "rocket-launch",
  "claw-grab",
  "prize-elevator",
  "coin-fountain",
  "treasure-map",
  "mystery-portal",
  "capsule-conveyor",
  "lucky-lanterns",
] as const;

test("inactive games never appear in the registered catalog", () => {
  const activeIds = new Set(ALL_GAMES.map((definition) => definition.id));
  for (const inactive of INACTIVE_IDS) {
    assert.ok(!activeIds.has(inactive), `${inactive} must stay out of games/index.ts`);
  }
});

test("retired game directories stay deleted from the repository", () => {
  for (const inactive of INACTIVE_IDS) {
    assert.ok(!existsSync(join(GAMES_ROOT, inactive)), `src/games/${inactive}/ must not be restored`);
  }
});

test("the active catalog count matches the currently supported set", () => {
  assert.equal(ALL_GAMES.length, 7);
});

test("every active game has a distinct arcade marquee glyph", () => {
  const glyphs = ALL_GAMES.map((definition) => definition.thumbnail.glyph);
  assert.equal(new Set(glyphs).size, glyphs.length, "two active games must not share one marquee glyph");
});

const sourceOf = (relativePath: string): string => readFileSync(join(WEB_ROOT, relativePath), "utf8");

test("the retired card-grid catalog markup is not reused", () => {
  const catalog = sourceOf("src/catalog/catalog.ts");
  assert.ok(!catalog.includes("game-card"), "catalog.ts must build cabinet machines, not the old .game-card grid");
  assert.ok(!catalog.includes("card-actions"), "catalog.ts must not reintroduce the old card-actions row");
  assert.ok(catalog.includes("cab-machine"), "catalog.ts must build the arcade-cabinet machine markup");
});

test("no pill-shaped chip button classes remain in the app shell", () => {
  for (const file of ["index.html", "src/application/shell.ts", "src/workbench/workbench.ts"]) {
    assert.ok(!sourceOf(file).includes("chip-btn"), `${file} must not reference the retired chip-btn pill button`);
  }
});

test("the game screen frame reads as a cabinet, not a floating glass panel", () => {
  const html = sourceOf("index.html");
  assert.ok(html.includes("cab-cabinet"), "index.html must wrap the game screen in the cabinet frame");
  assert.ok(!/class="panel"/.test(html), "index.html must not reintroduce the old generic .panel dashboard shell");
});
