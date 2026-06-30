import assert from "node:assert/strict";
import { test } from "node:test";

import { overlapBox, overlapCircle, raycast } from "./query.ts";
import type { RayHit } from "./vocabulary.ts";
import { bindNative } from "./host-binding.ts";
import { FakeHost } from "./fake-host.testkit.ts";

test("overlapCircle queries the scene by center components and radius", () => {
  const host = new FakeHost();
  host.overlapReturn = [3, 7];
  bindNative(host);
  assert.deepEqual(overlapCircle({ x: 2, y: 4 }, 6), [3, 7]);
  assert.deepEqual(host.overlapCalls, [[2, 4, 6]]);
});

test("overlapBox forwards the box center and half-extents and returns the entities", () => {
  const host = new FakeHost();
  host.overlapBoxReturn = [5, 9];
  bindNative(host);
  const center = { x: 1, y: 2, z: 3 };
  const halfExtents = { x: 0.5, y: 0.5, z: 0.5 };
  assert.deepEqual(overlapBox(center, halfExtents), [5, 9]);
  assert.deepEqual(host.overlapBoxCalls, [{ center, halfExtents }]);
});

test("raycast forwards the ray and returns the nearest hit", () => {
  const host = new FakeHost();
  const hit: RayHit = { distance: 2.5, entity: 4, point: { x: 0, y: 0, z: -2.5 } };
  host.raycastReturn = hit;
  bindNative(host);
  const origin = { x: 0, y: 0, z: 0 };
  const direction = { x: 0, y: 0, z: -1 };
  assert.deepEqual(raycast(origin, direction, 100), hit);
  assert.deepEqual(host.raycastCalls, [{ direction, maxDistance: 100, origin }]);
});

test("raycast returns the empty value on a miss", () => {
  const host = new FakeHost();
  bindNative(host);
  assert.equal(raycast({ x: 0, y: 0, z: 0 }, { x: 0, y: 1, z: 0 }, 100), undefined);
});
