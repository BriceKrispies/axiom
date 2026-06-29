import assert from "node:assert/strict";
import { test } from "node:test";

import { clamp, lerp, normalizeAngle } from "../src/math.ts";
import { bindNative } from "../src/host-binding.ts";
import { FakeHost } from "./fake-host.ts";

test("clamp forwards to the native MathApi and returns its result", () => {
  const host = new FakeHost();
  host.clampReturn = 5;
  bindNative(host);
  assert.equal(clamp(12, 0, 5), 5);
  assert.deepEqual(host.clampCalls, [[12, 0, 5]]);
});

test("lerp blends locally without crossing the bridge", () => {
  assert.equal(lerp(0, 10, 0.5), 5);
  assert.equal(lerp(0, 10, 0), 0);
  assert.equal(lerp(0, 10, 1), 10);
});

test("normalizeAngle forwards to the native MathApi", () => {
  const host = new FakeHost();
  host.normalizeReturn = 1;
  bindNative(host);
  assert.equal(normalizeAngle(99), 1);
  assert.deepEqual(host.normalizeCalls, [99]);
});
