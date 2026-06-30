import assert from "node:assert/strict";
import { test } from "node:test";

import { aabbOverlap, circleOverlap, clamp, lerp, normalizeAngle, pointInRect, v2 } from "./math.ts";
import { bindNative } from "./host-binding.ts";
import { FakeHost } from "./fake-host.testkit.ts";

test("clamp forwards to the native MathApi and returns its result", () => {
  const host = new FakeHost();
  host.clampReturn = 5;
  bindNative(host);
  assert.equal(clamp(12, 0, 5), 5);
  assert.deepEqual(host.clampCalls, [[12, 0, 5]]);
});

test("lerp forwards to the native MathApi (one deterministic source of truth)", () => {
  // lerp must cross the bridge, never blend locally: the host records the call and
  // returns the authoritative blend, so the SDK is proven to forward, not re-derive.
  const host = new FakeHost();
  bindNative(host);
  assert.equal(lerp(0, 10, 0.5), 5);
  assert.equal(lerp(-4, 4, 0.25), -2);
  assert.deepEqual(host.lerpCalls, [
    [0, 10, 0.5],
    [-4, 4, 0.25],
  ]);
});

test("normalizeAngle forwards to the native MathApi", () => {
  const host = new FakeHost();
  host.normalizeReturn = 1;
  bindNative(host);
  assert.equal(normalizeAngle(99), 1);
  assert.deepEqual(host.normalizeCalls, [99]);
});

test("the v2 namespace forwards every op to the native MathApi", () => {
  const host = new FakeHost();
  bindNative(host);
  assert.deepEqual(v2.add({ x: 1, y: 2 }, { x: 3, y: 4 }), { x: 4, y: 6 });
  assert.deepEqual(v2.sub({ x: 5, y: 7 }, { x: 1, y: 2 }), { x: 4, y: 5 });
  assert.deepEqual(v2.scale({ x: 1, y: -2 }, 3), { x: 3, y: -6 });
  assert.equal(v2.dot({ x: 2, y: 3 }, { x: 4, y: 5 }), 23);
  assert.equal(v2.len({ x: 3, y: 4 }), 5);
  assert.deepEqual(v2.normalize({ x: 0, y: 2 }), { x: 0, y: 1 });
  assert.equal(v2.dist({ x: 0, y: 0 }, { x: 3, y: 4 }), 5);
  assert.deepEqual(v2.lerp({ x: 0, y: 0 }, { x: 2, y: 8 }, 0.5), { x: 1, y: 4 });
});

test("aabbOverlap forwards to the native Aabb overlap test", () => {
  const host = new FakeHost();
  bindNative(host);
  assert.equal(aabbOverlap({ height: 2, width: 2, x: 0, y: 0 }, { height: 2, width: 2, x: 1, y: 1 }), true);
  assert.equal(aabbOverlap({ height: 2, width: 2, x: 0, y: 0 }, { height: 1, width: 1, x: 5, y: 5 }), false);
});

test("pointInRect forwards to the native Aabb containment test", () => {
  const host = new FakeHost();
  bindNative(host);
  assert.equal(pointInRect({ x: 2, y: 2 }, { height: 4, width: 4, x: 0, y: 0 }), true);
  assert.equal(pointInRect({ x: 5, y: 2 }, { height: 4, width: 4, x: 0, y: 0 }), false);
});

test("circleOverlap forwards to the native Sphere overlap test", () => {
  const host = new FakeHost();
  bindNative(host);
  assert.equal(circleOverlap({ center: { x: 0, y: 0 }, radius: 2 }, { center: { x: 3, y: 0 }, radius: 2 }), true);
  assert.equal(circleOverlap({ center: { x: 0, y: 0 }, radius: 2 }, { center: { x: 10, y: 0 }, radius: 1 }), false);
});
