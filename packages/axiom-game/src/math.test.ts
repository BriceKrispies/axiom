import assert from "node:assert/strict";
import { test } from "node:test";

import { clamp, lerp, normalizeAngle } from "./math.ts";
import { bindNative } from "./host-binding.ts";
import { FakeHost } from "./fake-host.testkit.ts";

test("clamp forwards to the native MathApi and returns its result", () => {
  const host = new FakeHost();
  host.clampReturn = 5;
  bindNative(host);
  assert.equal(clamp(12, 0, 5), 5);
  assert.deepEqual(host.clampCalls, [[12, 0, 5]]);
});

test("lerp blends locally without crossing the bridge", () => {
  // A fresh fake proves lerp never consults the host: no clamp/normalize calls.
  const host = new FakeHost();
  bindNative(host);
  assert.equal(lerp(0, 10, 0.5), 5);
  assert.equal(lerp(0, 10, 0), 0);
  assert.equal(lerp(0, 10, 1), 10);
  assert.equal(lerp(-4, 4, 0.25), -2);
  assert.deepEqual(host.clampCalls, []);
  assert.deepEqual(host.normalizeCalls, []);
});

test("normalizeAngle forwards to the native MathApi", () => {
  const host = new FakeHost();
  host.normalizeReturn = 1;
  bindNative(host);
  assert.equal(normalizeAngle(99), 1);
  assert.deepEqual(host.normalizeCalls, [99]);
});
