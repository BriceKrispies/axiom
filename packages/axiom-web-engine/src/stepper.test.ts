/*
 * stepper.test.ts — `node --test` coverage for the pure `FixedStepper`
 * accumulator: exact step counts across fractional frames, the 100 ms stall
 * clamp, cap-excess dropping, negative-elapsed handling, and tick counting.
 * No DOM: the `raf-loop.ts` driver is exercised only in the browser. These are
 * the reference `platform.test.ts` FixedStepper assertions, ported
 * intact to the split-out `stepper.ts`.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { FixedStepper } from "./stepper.ts";

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
