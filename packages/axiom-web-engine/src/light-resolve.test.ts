/*
 * light-resolve.test.ts — `node --test` coverage for the authored-spec →
 * frame-light conversion in `light-resolve.ts`: the kind predicates, folding
 * intensity into color, direction normalization, and the degenerate-direction
 * fallback. Pure — no store, no backend.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import type { Light } from "./api.ts";
import { isDirectional, isPoint, litColor, resolveDirLight, resolvePointLight } from "./light-resolve.ts";

const dirLight = (x: number, y: number, z: number, intensity = 1): Extract<Light, { kind: "directional" }> => ({
  color: [1, 1, 1, 1],
  direction: { x, y, z },
  intensity,
  kind: "directional",
});

const pointLight = (intensity = 1): Extract<Light, { kind: "point" }> => ({
  color: [1, 0.5, 0.25, 1],
  intensity,
  kind: "point",
  position: { x: 2, y: 3, z: -4 },
});

/** Channel-wise comparison with a float tolerance — the resolved color is the
 * product of two floats, so exact equality would pin the test to IEEE noise. */
const assertRgbClose = (actual: readonly number[], expected: readonly number[], msg: string): void => {
  expected.forEach((want, i) => {
    assert.ok(Math.abs(actual[i]! - want) < 1e-9, `${msg} (channel ${i}): expected ${want}, got ${actual[i]}`);
  });
};

test("the kind predicates split the union both ways", () => {
  assert.equal(isDirectional(dirLight(0, -1, 0)), true);
  assert.equal(isDirectional(pointLight()), false);
  assert.equal(isPoint(pointLight()), true);
  assert.equal(isPoint(dirLight(0, -1, 0)), false);
});

test("intensity folds into the color channels", () => {
  assert.deepEqual(litColor(pointLight(2)), [2, 1, 0.5]);
  assert.deepEqual(litColor(pointLight(0)), [0, 0, 0], "a dark light contributes nothing");
});

test("a directional light's direction is normalized", () => {
  const { direction } = resolveDirLight(dirLight(0, 0, -5));
  assertRgbClose(direction, [0, 0, -1], "length 5 normalizes to unit");
  const diagonal = resolveDirLight(dirLight(3, 4, 0)).direction;
  assert.ok(Math.abs(Math.hypot(...diagonal) - 1) < 1e-9, "any input length resolves to unit length");
});

test("a degenerate direction falls back to straight down rather than NaN", () => {
  // A light authored with a zero direction must still render as a sane overhead
  // key; dividing by zero here would poison every lit pixel with NaN.
  const { direction } = resolveDirLight(dirLight(0, 0, 0));
  assert.deepEqual(direction, [0, -1, 0]);
  assert.ok(direction.every((c) => Number.isFinite(c)), "no NaN escapes into the frame");
});

test("a directional light carries its intensity-folded color", () => {
  const light: Extract<Light, { kind: "directional" }> = { ...dirLight(0, -1, 0, 3), color: [0.2, 0.4, 0.6, 1] };
  assertRgbClose(resolveDirLight(light).color, [0.6, 1.2, 1.8], "intensity 3 scales every channel");
});

test("a point light flattens its position and folds its intensity", () => {
  const resolved = resolvePointLight(pointLight(2));
  assert.deepEqual(resolved.position, [2, 3, -4], "the vector flattens to a triple");
  assert.deepEqual(resolved.color, [2, 1, 0.5]);
});
