import { strict as assert } from "node:assert";
import { test } from "node:test";

import { idleDeltaForTag } from "./idle.ts";

test("idle is deterministic (same tag/tick/phase ⇒ identical delta)", () => {
  assert.deepEqual(idleDeltaForTag("torso", 42, 0.3), idleDeltaForTag("torso", 42, 0.3));
  assert.deepEqual(idleDeltaForTag("upper_arm", 7, 1.1), idleDeltaForTag("upper_arm", 7, 1.1));
});

test("static parts hold still (no delta)", () => {
  assert.equal(idleDeltaForTag("base", 30, 0), undefined);
  assert.equal(idleDeltaForTag("eye", 30, 0), undefined);
  assert.equal(idleDeltaForTag("weapon", 30, 0), undefined);
});

test("at the breath zero-crossing the torso rests at neutral scale", () => {
  const d = idleDeltaForTag("torso", 0, 0);
  assert.ok(d !== undefined);
  assert.ok(Math.abs((d?.scale ?? 0) - 1) < 1e-9, "scale should be 1 at sin(0)");
  assert.ok(Math.abs(d?.pos?.y ?? 1) < 1e-9, "no lift at sin(0)");
});

test("the torso breathes (scale pulses off neutral over the cycle)", () => {
  // A quarter cycle in, the breath is near its peak, so the chest is expanded.
  const d = idleDeltaForTag("torso", 42, 0);
  assert.ok((d?.scale ?? 0) > 1, "torso should expand at a positive breath phase");
});

test("arms and head carry a rotation delta (weapon-ready sway / nod)", () => {
  assert.ok(idleDeltaForTag("upper_arm", 12, 0)?.rot !== undefined, "arms sway");
  assert.ok(idleDeltaForTag("head", 12, 0)?.rot !== undefined, "head nods");
});
