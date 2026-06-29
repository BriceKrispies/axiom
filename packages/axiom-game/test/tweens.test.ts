import assert from "node:assert/strict";
import { test } from "node:test";

import { TickPump } from "../src/pump.ts";
import { makeTweens } from "../src/tweens.ts";
import { FakeBridge } from "./fake-bridge.ts";

// At 60 Hz a 2/60-second tween lasts round(2/60 * 60) = 2 fixed ticks.
const TWO_TICKS_SECONDS = 2 / 60;

test("a linear tween is sampled each tick and applies the eased value to its target", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const samples: number[] = [];
  let target = 0;
  let completed = 0;
  makeTweens(pump, 0).add({
    duration: TWO_TICKS_SECONDS,
    from: 0,
    onComplete: (): void => {
      completed += 1;
    },
    onUpdate: (value): void => {
      samples.push(value);
      target = value;
    },
    to: 10,
  });
  pump.pump(1);
  pump.pump(2);
  pump.pump(3);
  // Linear over 2 ticks: tick 1 -> 0.5 progress -> 5, tick 2 -> end -> 10.
  assert.deepEqual(samples, [5, 10]);
  assert.equal(target, 10);
  assert.equal(completed, 1);
});

test("a tween's ease selects the native curve by its dense index", () => {
  const fake = new FakeBridge();
  const pump = new TickPump(fake, 60);
  makeTweens(pump, 0).add({
    duration: TWO_TICKS_SECONDS,
    ease: "quadIn",
    from: 0,
    onUpdate: (): void => {
      // value sink
    },
    to: 8,
  });
  // quadIn at progress 0.5 is 0.25 -> 8 * 0.25 = 2 (the fake mirrors EASES order).
  assert.equal(fake.tweenValue(1, 1), 2);
});

test("a tween with no onComplete completes via the no-op sink", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const samples: number[] = [];
  makeTweens(pump, 0).add({
    duration: TWO_TICKS_SECONDS,
    from: 0,
    onUpdate: (value): void => {
      samples.push(value);
    },
    to: 4,
  });
  pump.pump(1);
  pump.pump(2);
  // Reaching the end with no onComplete must not throw; samples still arrive.
  assert.deepEqual(samples, [2, 4]);
});

test("cancel stops a tween from sampling further", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const samples: number[] = [];
  const tweens = makeTweens(pump, 0);
  const id = tweens.add({
    duration: TWO_TICKS_SECONDS,
    from: 0,
    onUpdate: (value): void => {
      samples.push(value);
    },
    to: 10,
  });
  tweens.cancel(id);
  pump.pump(1);
  pump.pump(2);
  assert.deepEqual(samples, []);
});
