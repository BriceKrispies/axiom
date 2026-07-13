/*
 * backend.test.ts — coverage for the shared backend contract module. backend.ts
 * is types plus four shared numeric constants (the ambient floor, the default
 * clear color, and the directional/point light caps both backends honor); this
 * pins their values so a drift in either backend can be caught against one source.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { AMBIENT, CLEAR_COLOR, MAX_DIR_LIGHTS, MAX_POINT_LIGHTS } from "./backend.ts";

test("ambient floor is the shared low fill both backends apply", () => {
  assert.equal(AMBIENT, 0.12);
});

test("default clear color is the near-black void (RGB 0..1)", () => {
  assert.deepEqual(CLEAR_COLOR, [5 / 255, 6 / 255, 10 / 255]);
});

test("light caps match the forward path's fixed uniform slots", () => {
  assert.equal(MAX_DIR_LIGHTS, 8);
  assert.equal(MAX_POINT_LIGHTS, 8);
});
