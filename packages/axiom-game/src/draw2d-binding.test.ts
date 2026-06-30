import assert from "node:assert/strict";
import { test } from "node:test";

import { type EmitterConfig, rangeOf } from "./draw2d-binding.ts";

const emitterConfig: EmitterConfig = {
  colorEnd: [0, 0, 0, 0],
  colorStart: [1, 1, 1, 1],
  count: 8,
  gravity: { x: 0, y: -4 },
  layer: 5,
  lifetimeSeconds: 2,
  size: 0.5,
  speed: 10,
  spread: 0.25,
};

test("rangeOf resolves a scalar to the degenerate [v, v] and a tuple to [min, max]", () => {
  // SPEC-04 §10.1: a scalar emitter field is the backward-compatible degenerate
  // range; a [min, max] tuple passes both endpoints through unchanged.
  assert.deepEqual(rangeOf(5), [5, 5]);
  assert.deepEqual(rangeOf(0), [0, 0]);
  assert.deepEqual(rangeOf([0.2, 0.8]), [0.2, 0.8]);
  // A ranged emitter config type-checks and resolves each field to a pair.
  const ranged: EmitterConfig = { ...emitterConfig, lifetimeSeconds: [1, 3], size: [0.2, 0.8], speed: [5, 15] };
  assert.deepEqual(rangeOf(ranged.lifetimeSeconds), [1, 3]);
  assert.deepEqual(rangeOf(ranged.speed), [5, 15]);
  assert.deepEqual(rangeOf(ranged.size), [0.2, 0.8]);
});
