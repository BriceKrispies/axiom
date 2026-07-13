/*
 * shading.test.ts — `node --test` coverage for the pure Lambert shading term in
 * shading.ts. It reproduces the reference render.test.ts parity assertions (the
 * ambient floor, a full-strength directional light, a back-facing directional
 * light contributing nothing, a point light's exact 1/(1 + 0.08·d²) falloff, and
 * determinism) plus the multi-light accumulation case, driving both fold callbacks
 * so the whole file is exercised. No DOM and no WebGL — this is the shared math.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { AMBIENT } from "./backend.ts";
import { lambertLight } from "./shading.ts";

const EPS = 1e-5;
const assertClose = (actual: number, expected: number, msg: string): void => {
  assert.ok(Math.abs(actual - expected) <= EPS, `${msg}: expected ${expected}, got ${actual}`);
};

// No lights: the fold over each empty list returns the ambient seed unchanged.
test("no lights leaves a pure ambient floor", () => {
  const dark = lambertLight(0, 1, 0, 0, 0, 0, { dirLights: [], pointLights: [] });
  assert.deepEqual(dark, [AMBIENT, AMBIENT, AMBIENT]);
});

// A directional light traveling straight down fully lights an up-facing surface:
// ambient + color·intensity, per channel.
test("a directional light fully lights a facing surface, per channel", () => {
  const sun = lambertLight(0, 1, 0, 0, 0, 0, {
    dirLights: [{ color: [1, 0.5, 0.25], direction: [0, -1, 0] }],
    pointLights: [],
  });
  assertClose(sun[0], AMBIENT + 1, "sun r");
  assertClose(sun[1], AMBIENT + 0.5, "sun g");
  assertClose(sun[2], AMBIENT + 0.25, "sun b");
});

// The same light striking the back of the surface contributes nothing (max(0, ·)).
test("a back-facing directional light contributes nothing", () => {
  const back = lambertLight(0, -1, 0, 0, 0, 0, {
    dirLights: [{ color: [1, 1, 1], direction: [0, -1, 0] }],
    pointLights: [],
  });
  assert.deepEqual(back, [AMBIENT, AMBIENT, AMBIENT]);
});

// A point light 2 m overhead: lambert 1, falloff 1/(1 + 0.08·4) — the exact
// per-fragment expression.
test("a point light applies the soft distance falloff", () => {
  const point = lambertLight(0, 1, 0, 0, 0, 0, {
    dirLights: [],
    pointLights: [{ color: [1, 1, 1], position: [0, 2, 0] }],
  });
  assertClose(point[0], AMBIENT + 1 / (1 + 0.08 * 4), "point falloff");
});

// A point light coincident with the surface stays finite (the MIN_DISTANCE floor
// keeps 1/max(d, 1e-5) from dividing by zero; at d=0 the N·L term is also zero).
test("a coincident point light stays finite", () => {
  const here = lambertLight(0, 1, 0, 0, 0, 0, {
    dirLights: [],
    pointLights: [{ color: [1, 1, 1], position: [0, 0, 0] }],
  });
  assert.ok(Number.isFinite(here[0]), "coincident point light r is finite");
  assertClose(here[0], AMBIENT, "coincident point light adds no directional term");
});

// Both lists fold into the same running channel sums: ambient + directional +
// point, in that order.
test("directional and point contributions accumulate together", () => {
  const both = lambertLight(0, 1, 0, 0, 0, 0, {
    dirLights: [{ color: [0.2, 0.2, 0.2], direction: [0, -1, 0] }],
    pointLights: [{ color: [1, 1, 1], position: [0, 2, 0] }],
  });
  assertClose(both[0], AMBIENT + 0.2 + 1 / (1 + 0.08 * 4), "combined r");
});

// Deterministic: identical inputs, identical output.
test("identical inputs give identical output", () => {
  const seed = { dirLights: [], pointLights: [{ color: [1, 1, 1] as const, position: [0, 2, 0] as const }] };
  const first = lambertLight(0, 1, 0, 0, 0, 0, seed);
  const again = lambertLight(0, 1, 0, 0, 0, 0, seed);
  assert.deepEqual(first, again);
});
