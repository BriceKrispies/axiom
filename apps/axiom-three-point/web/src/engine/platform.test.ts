/*
 * engine/platform.test.ts — `node --test` coverage for the pure platform cores:
 * the `FixedStepper` accumulator (exact step counts across fractional frames,
 * the 100 ms stall clamp, cap-excess dropping, tick counting) and the
 * `InputState` snapshot semantics (one-tick pressed/released edges, key-repeat
 * immunity, multi-code action OR, look-delta draining, pointer latest-sample
 * behavior). No DOM: `attachDomInput` and `audio.ts` are exercised only in the
 * browser.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { FixedStepper } from "./loop.ts";
import { InputState } from "./input.ts";

// ---------------------------------------------------------------------------
// FixedStepper
// ---------------------------------------------------------------------------

test("FixedStepper: 16.67 ms frames at 60 Hz average exactly one step", () => {
  const stepper = new FixedStepper(60, 8);
  let total = 0;
  let maxPerFrame = 0;
  for (let i = 0; i < 600; i += 1) {
    const steps = stepper.advance(16.67);
    total += steps;
    maxPerFrame = Math.max(maxPerFrame, steps);
  }
  // 600 × 16.67 ms = 10002 ms → exactly 600 whole 16.6-ms steps due.
  assert.equal(total, 600);
  assert.equal(stepper.tick, 600);
  // The fractional surplus never bursts: each frame yields 1 step (±0/2 never needed).
  assert.ok(maxPerFrame <= 2);
});

test("FixedStepper: uneven frames never lose or duplicate steps", () => {
  const stepper = new FixedStepper(60, 8);
  // 8 ms + 25 ms = 33 ms per pair ≈ 2 steps/pair, delivered in whole steps only.
  let total = 0;
  for (let i = 0; i < 30; i += 1) {
    total += stepper.advance(8);
    total += stepper.advance(25.3334);
  }
  // 30 × 33.3334 ms = 1000.002 ms → exactly 60 steps.
  assert.equal(total, 60);
  assert.equal(stepper.tick, 60);
});

test("FixedStepper: a 500 ms stall clamps to 100 ms and caps at maxCatchUpSteps", () => {
  // 100 Hz → 10 ms steps; 100 ms clamped stall is 10 whole steps due.
  const capped = new FixedStepper(100, 3);
  assert.equal(capped.advance(500), 3);
  assert.equal(capped.tick, 3);

  // With a generous cap the clamp itself is visible: 500 ms yields only the
  // 10 steps of the 100 ms clamp, not 50.
  const uncapped = new FixedStepper(100, 30);
  assert.equal(uncapped.advance(500), 10);
  assert.equal(uncapped.tick, 10);
});

test("FixedStepper: excess time beyond the cap is dropped, not banked", () => {
  const stepper = new FixedStepper(100, 3);
  assert.equal(stepper.advance(100), 3); // 10 steps due, 3 run, 7 dropped
  // Nothing banked: with no new time there are no leftover steps to drain.
  assert.equal(stepper.advance(0), 0);
  // Exactly one new step's worth of time yields exactly one step.
  assert.equal(stepper.advance(10), 1);
  assert.equal(stepper.advance(5), 0);
  assert.equal(stepper.tick, 4);
});

test("FixedStepper: tick increments once per returned step", () => {
  const stepper = new FixedStepper(100, 5);
  assert.equal(stepper.tick, 0);
  assert.equal(stepper.advance(25), 2);
  assert.equal(stepper.tick, 2);
  assert.equal(stepper.advance(5), 1); // 5 ms banked fraction + 5 ms = 10 ms
  assert.equal(stepper.tick, 3);
  assert.equal(stepper.advance(0), 0);
  assert.equal(stepper.tick, 3);
});

test("FixedStepper: negative elapsed is treated as zero", () => {
  const stepper = new FixedStepper(100, 5);
  assert.equal(stepper.advance(-50), 0);
  assert.equal(stepper.advance(10), 1);
});

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
  assert.deepEqual(input.pointer(), { pos: { x: 10, y: 20 }, down: true });

  // Latest sample wins.
  input.pointerEvent(15, 25, true);
  input.pointerEvent(18, 30, false);
  input.beginTick();
  assert.deepEqual(input.pointer(), { pos: { x: 18, y: 30 }, down: false });

  input.pointerClear();
  input.beginTick();
  assert.equal(input.pointer(), undefined);
});
