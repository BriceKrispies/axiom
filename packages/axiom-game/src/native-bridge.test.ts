import assert from "node:assert/strict";
import { test } from "node:test";

import type { BodyKind, NativeBridge, PointerSample, Swipe, TweenCurve } from "./native-bridge.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";

// native-bridge.ts is TYPE-ONLY (it exports only `interface`/`type` declarations,
// the wasm seam the projections drive). It is erased at runtime and so does NOT
// appear in the coverage report. These checks are compile-time contract
// assertions that also run trivially: the literals must match the seam's shapes,
// and `FakeBridge` must satisfy the whole `NativeBridge` interface.

test("the small value shapes of the seam carry their declared fields", () => {
  const curve: TweenCurve = { durationTicks: 30, easeIndex: 0, from: 0, to: 1 };
  const pointer: PointerSample = { down: true, pos: { x: 4, y: 8 } };
  const kind: BodyKind = "dynamic";
  const swipe: Swipe = "left";
  assert.equal(curve.durationTicks, 30);
  assert.equal(pointer.pos.x, 4);
  assert.equal(pointer.down, true);
  assert.equal(kind, "dynamic");
  assert.equal(swipe, "left");
});

test("FakeBridge satisfies the NativeBridge interface and its methods are callable", () => {
  // Typing the fake as the interface is the compile-time proof it implements the
  // full seam; the calls confirm the shape is genuinely inhabited at runtime.
  const bridge: NativeBridge = new FakeBridge();
  const entity = bridge.worldSpawn([{ kind: "tag" }]);
  assert.equal(entity, 1);
  assert.equal(bridge.worldAlive(entity), true);
  assert.equal(typeof bridge.snapshot(), "object");
  assert.equal(typeof bridge.advance(0).steps, "number");
});
