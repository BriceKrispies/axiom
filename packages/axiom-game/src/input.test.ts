import assert from "node:assert/strict";
import { test } from "node:test";

import { bindAction, makeInput } from "./input.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";
import { FakeHost } from "./fake-host.testkit.ts";
import { bindNative } from "./host-binding.ts";

const TICK = 5;

test("isDown / pressed / released read the running tick's snapshot", () => {
  const fake = new FakeBridge();
  fake.down.add(`${TICK}|fire`);
  fake.pressedEdges.add(`${TICK}|fire`);
  fake.releasedEdges.add(`${TICK}|jump`);
  const input = makeInput(fake, TICK);
  assert.equal(input.isDown("fire"), true);
  assert.equal(input.isDown("jump"), false);
  assert.equal(input.pressed("fire"), true);
  assert.equal(input.released("jump"), true);
  assert.equal(input.released("fire"), false);
});

test("axis collapses an action pair to -1, 0, or +1 (each table arm)", () => {
  // Neither held -> difference 0 -> index 1 -> AXIS_ZERO.
  const neither = makeInput(new FakeBridge(), TICK);
  assert.equal(neither.axis("left", "right"), 0);

  // Positive held -> difference +1 -> index 2 -> AXIS_POSITIVE.
  const positive = new FakeBridge();
  positive.down.add(`${TICK}|right`);
  assert.equal(makeInput(positive, TICK).axis("left", "right"), 1);

  // Negative held -> difference -1 -> index 0 -> AXIS_NEGATIVE.
  const negative = new FakeBridge();
  negative.down.add(`${TICK}|left`);
  assert.equal(makeInput(negative, TICK).axis("left", "right"), -1);
});

test("pointer / pointerPressed / swipe forward the snapshot or its empty value", () => {
  const fake = new FakeBridge();
  fake.pointers.set(TICK, { down: true, pos: { x: 3, y: 4 } });
  fake.pressedStarts.set(TICK, { x: 1, y: 2 });
  fake.swipes.set(TICK, "left");
  const input = makeInput(fake, TICK);
  assert.deepEqual(input.pointer(), { down: true, pos: { x: 3, y: 4 } });
  assert.deepEqual(input.pointerPressed(), { x: 1, y: 2 });
  assert.equal(input.swipe(), "left");

  const empty = makeInput(new FakeBridge(), TICK);
  assert.equal(empty.pointer(), undefined);
  assert.equal(empty.pointerPressed(), undefined);
  assert.equal(empty.swipe(), undefined);
});

test("pressedAtTick reports the recorded tick, or the empty value if never", () => {
  const fake = new FakeBridge();
  fake.pressedAt.set(`${TICK}|fire`, 2);
  const input = makeInput(fake, TICK);
  assert.equal(input.pressedAtTick("fire"), 2);
  assert.equal(input.pressedAtTick("jump"), undefined);
});

test("bindAction forwards the action and keys to the bound host", () => {
  const host = new FakeHost();
  bindNative(host);
  bindAction("jump", ["Space", "KeyW"]);
  assert.deepEqual(host.bindings, [["jump", ["Space", "KeyW"]]]);
});
