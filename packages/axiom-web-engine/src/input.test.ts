/*
 * input.test.ts — `node --test` coverage for the pure `InputState` snapshot
 * semantics: one-tick pressed/released edges, key-repeat immunity, multi-code
 * action OR, unbound-action inertness, blur-driven release-all, look-delta
 * draining, and pointer latest-sample behavior. No DOM: the `dom-input.ts`
 * binding is exercised only in the browser. These are the reference
 * `platform.test.ts` InputState assertions, ported intact to the split-out
 * `input.ts`, plus a `releaseAllKeys` case for the blur path.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { InputState, sampleInput } from "./input.ts";

// ---------------------------------------------------------------------------
// InputState — key edges
// ---------------------------------------------------------------------------

test("InputState: pressed fires only on the tick after keydown, released after keyup", () => {
  const input = new InputState();
  input.bindAction("jump", ["Space"]);

  input.beginTick();
  assert.equal(input.isDown("jump"), false);
  assert.equal(input.pressed("jump"), false);
  assert.equal(input.released("jump"), false);

  input.keyEvent("Space", true);
  input.beginTick();
  assert.equal(input.isDown("jump"), true);
  assert.equal(input.pressed("jump"), true);
  assert.equal(input.released("jump"), false);

  // Held: still down, but no new edge.
  input.beginTick();
  assert.equal(input.isDown("jump"), true);
  assert.equal(input.pressed("jump"), false);

  input.keyEvent("Space", false);
  input.beginTick();
  assert.equal(input.isDown("jump"), false);
  assert.equal(input.pressed("jump"), false);
  assert.equal(input.released("jump"), true);

  // The release edge lasts exactly one tick.
  input.beginTick();
  assert.equal(input.released("jump"), false);
});

test("InputState: key auto-repeat does not re-trigger pressed", () => {
  const input = new InputState();
  input.bindAction("shoot", ["KeyF"]);

  input.keyEvent("KeyF", true);
  input.beginTick();
  assert.equal(input.pressed("shoot"), true);

  // OS auto-repeat: more keydowns while held.
  input.keyEvent("KeyF", true);
  input.keyEvent("KeyF", true);
  input.beginTick();
  assert.equal(input.isDown("shoot"), true);
  assert.equal(input.pressed("shoot"), false);

  input.keyEvent("KeyF", true);
  input.beginTick();
  assert.equal(input.pressed("shoot"), false);
});

test("InputState: an action bound to two codes ORs them together", () => {
  const input = new InputState();
  input.bindAction("left", ["KeyA", "ArrowLeft"]);

  input.keyEvent("KeyA", true);
  input.beginTick();
  assert.equal(input.isDown("left"), true);
  assert.equal(input.pressed("left"), true);

  // The second code joins while the first is held: still down, no new edge.
  input.keyEvent("ArrowLeft", true);
  input.beginTick();
  assert.equal(input.isDown("left"), true);
  assert.equal(input.pressed("left"), false);

  // Releasing one code keeps the action down (the other still holds it).
  input.keyEvent("KeyA", false);
  input.beginTick();
  assert.equal(input.isDown("left"), true);
  assert.equal(input.released("left"), false);

  // Releasing the last code releases the action.
  input.keyEvent("ArrowLeft", false);
  input.beginTick();
  assert.equal(input.isDown("left"), false);
  assert.equal(input.released("left"), true);
});

test("InputState: unbound actions are never down and never edge", () => {
  const input = new InputState();
  input.keyEvent("KeyZ", true);
  input.beginTick();
  assert.equal(input.isDown("mystery"), false);
  assert.equal(input.pressed("mystery"), false);
  assert.equal(input.released("mystery"), false);
});

test("InputState: releaseAllKeys drops every held key as a normal release edge", () => {
  const input = new InputState();
  input.bindAction("charge", ["Space"]);

  input.keyEvent("Space", true);
  input.beginTick();
  assert.equal(input.isDown("charge"), true);

  // Blur / pointer-lock loss releases everything; the next tick sees a clean
  // release edge, and the key stays up thereafter (regaining focus fabricates
  // nothing).
  input.releaseAllKeys();
  input.beginTick();
  assert.equal(input.isDown("charge"), false);
  assert.equal(input.released("charge"), true);

  input.beginTick();
  assert.equal(input.isDown("charge"), false);
  assert.equal(input.released("charge"), false);
});

// ---------------------------------------------------------------------------
// InputState — look accumulation
// ---------------------------------------------------------------------------

test("InputState: look() drains the delta accumulated since the previous beginTick", () => {
  const input = new InputState();

  input.lookEvent(2, 3);
  input.lookEvent(1, -1);
  input.beginTick();
  assert.deepEqual(input.look(), { x: 3, y: 2 });
  // Stable within the tick.
  assert.deepEqual(input.look(), { x: 3, y: 2 });

  // Nothing accumulated → zero delta next tick.
  input.beginTick();
  assert.deepEqual(input.look(), { x: 0, y: 0 });

  // Accumulation restarts after each drain.
  input.lookEvent(-4, 5);
  input.beginTick();
  assert.deepEqual(input.look(), { x: -4, y: 5 });
});

// ---------------------------------------------------------------------------
// InputState — pointer sampling
// ---------------------------------------------------------------------------

test("InputState: pointer() returns the latest sample and undefined after clear", () => {
  const input = new InputState();
  input.beginTick();
  assert.equal(input.pointer(), undefined);

  input.pointerEvent(10, 20, true);
  input.beginTick();
  assert.deepEqual(input.pointer(), { down: true, pos: { x: 10, y: 20 } });

  // Latest sample wins.
  input.pointerEvent(15, 25, true);
  input.pointerEvent(18, 30, false);
  input.beginTick();
  assert.deepEqual(input.pointer(), { down: false, pos: { x: 18, y: 30 } });

  input.pointerClear();
  input.beginTick();
  assert.equal(input.pointer(), undefined);
});

// ---------------------------------------------------------------------------
// sampleInput — the immutable InputFrame snapshot a pure update reads
// ---------------------------------------------------------------------------

test("sampleInput resolves down/pressed/released action sets plus look and pointer", () => {
  const input = new InputState();
  input.bindAction("jump", ["Space"]);
  input.bindAction("left", ["ArrowLeft", "KeyA"]);
  input.bindAction("idle", ["KeyZ"]);
  const actions = ["jump", "left", "idle"];

  // First press of Space + A: both are edges this tick, both down, none released.
  input.keyEvent("Space", true);
  input.keyEvent("KeyA", true);
  input.lookEvent(3, -4);
  input.pointerEvent(7, 8, true);
  input.beginTick();
  const first = sampleInput(input, actions);
  assert.deepEqual([...first.down].toSorted(), ["jump", "left"]);
  assert.deepEqual([...first.pressed].toSorted(), ["jump", "left"]);
  assert.equal(first.released.size, 0);
  assert.deepEqual(first.look, { x: 3, y: -4 });
  assert.deepEqual(first.pointer, { down: true, pos: { x: 7, y: 8 } });

  // Hold Space, release A: nothing pressed (held ≠ edge), left released, look drained.
  input.keyEvent("KeyA", false);
  input.beginTick();
  const second = sampleInput(input, actions);
  assert.deepEqual([...second.down], ["jump"]);
  assert.equal(second.pressed.size, 0);
  assert.deepEqual([...second.released], ["left"]);
  assert.deepEqual(second.look, { x: 0, y: 0 });
});
