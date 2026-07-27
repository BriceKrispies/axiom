/*
 * tier.test.ts — the ladder's rules.
 *
 * These are the claims the harness reads off `window.__axiomTier`, so they are
 * worth pinning: the page reports the ENGINE's render tier rather than a guess
 * of its own, a stripped stylesheet pins the page at the form, a rung that
 * cannot be mounted costs exactly one rung, and the walk down always terminates
 * at the document — which is the only rung that cannot fail.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { FORM_TIER, chooseTier, demoteRender, postsInPlace, rungFor, type PageProbe, type PageTier } from "./tier.ts";

const probe = (overrides: Partial<PageProbe> = {}): PageProbe => ({
  cssApplied: true,
  renderTier: "webgl2",
  ...overrides,
});

const ENGINE_RUNGS: readonly PageTier[] = ["webgpu", "webgl2", "webgl1", "canvas2d"];

test("the render tier is the engine's verdict, not a re-derivation of it", () => {
  (["webgpu", "webgl2", "webgl1", "canvas2d", "css3d"] as const).forEach((tier) => {
    assert.equal(chooseTier(probe({ renderTier: tier })), tier);
  });
});

test("a stripped stylesheet pins the page at the form, however capable the machine", () => {
  assert.equal(chooseTier(probe({ cssApplied: false, renderTier: "webgpu" })), FORM_TIER);
  assert.equal(chooseTier(probe({ cssApplied: false, renderTier: "css3d" })), FORM_TIER);
});

test("each rung declares how it is drawn", () => {
  ENGINE_RUNGS.forEach((tier) => assert.equal(rungFor(tier), "engine"));
  assert.equal(rungFor("css3d"), "css3d");
  assert.equal(rungFor(FORM_TIER), "none");
});

test("a rung that will not mount costs one rung, and the walk terminates", () => {
  // Every engine rung falls to the CSS 3D chests: a canvas that would not come
  // up says nothing about whether the DOM can composite a transform tree.
  ENGINE_RUNGS.forEach((tier) => assert.equal(demoteRender(tier), "css3d"));
  assert.equal(demoteRender("css3d"), FORM_TIER);
  assert.equal(demoteRender(FORM_TIER), FORM_TIER);
  // ...and walking down from the very top reaches the form in a bounded number
  // of steps, so the mount loop can never spin.
  const walk = (from: PageTier, steps = 0): number =>
    rungFor(from) === "none" ? steps : walk(demoteRender(from), steps + 1);
  assert.equal(walk("webgpu"), 2);
});

test("every rung above the form posts in place; the form navigates", () => {
  [...ENGINE_RUNGS, "css3d" as const].forEach((tier) => assert.equal(postsInPlace(tier), true));
  assert.equal(postsInPlace(FORM_TIER), false);
});
