/*
 * backend.test.ts — coverage for the shared backend contract module. backend.ts
 * is types plus the shared numeric constants (the DEFAULT ambient floor and its
 * grey triple, the default clear color, and the directional/point light caps both
 * backends honor); this pins their values so a drift in either backend can be
 * caught against one source. Ambient itself is per-frame scene data now
 * (`SceneFrame.ambient`) — `AMBIENT`/`DEFAULT_AMBIENT` are only its default.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { AMBIENT, CLEAR_COLOR, DEFAULT_AMBIENT, MAX_DIR_LIGHTS, MAX_POINT_LIGHTS } from "./backend.ts";

test("ambient floor is the shared low fill both backends apply", () => {
  assert.equal(AMBIENT, 0.12);
});

// The store seeds SceneFrame.ambient with this, so a scene that never authors an
// ambient renders byte-identically to the pre-field engine.
test("the default ambient triple is the historical grey floor", () => {
  assert.deepEqual(DEFAULT_AMBIENT, [0.12, 0.12, 0.12]);
});

test("default clear color is the near-black void (RGB 0..1)", () => {
  assert.deepEqual(CLEAR_COLOR, [5 / 255, 6 / 255, 10 / 255]);
});

test("light caps match the forward path's fixed uniform slots", () => {
  assert.equal(MAX_DIR_LIGHTS, 8);
  assert.equal(MAX_POINT_LIGHTS, 8);
});
