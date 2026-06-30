import assert from "node:assert/strict";
import { test } from "node:test";

import { FakeBridge } from "./fake-bridge.testkit.ts";
import { TickPump } from "./pump.ts";
import type { StateNode } from "./state-machine.ts";

// Pump ticks 0..maxTick one at a time, recording the ticks at which `count` grows.
const firingTicks = (pump: TickPump, fired: { count: number }, maxTick: number): number[] => {
  const ticks: number[] = [];
  for (let tick = 0; tick <= maxTick; tick += 1) {
    const before = fired.count;
    pump.pump(tick);
    if (fired.count > before) {
      ticks.push(tick);
    }
  }
  return ticks;
};

test("scheduleAfter holds a one-shot whose callback fires once on its due tick", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const fired = { count: 0 };
  // Registered at tick 0 with delay 3 -> due at tick 3, fires exactly once.
  pump.scheduleAfter(0, 3, () => {
    fired.count += 1;
  });
  assert.deepEqual(firingTicks(pump, fired, 6), [3]);
  assert.equal(fired.count, 1);
});

test("scheduleEvery holds a repeating timer that re-fires each interval", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const fired = { count: 0 };
  // Registered at tick 0, interval 2 -> fires at 2, 4, 6.
  pump.scheduleEvery(0, 2, () => {
    fired.count += 1;
  });
  assert.deepEqual(firingTicks(pump, fired, 6), [2, 4, 6]);
  assert.equal(fired.count, 3);
});

test("cancelTimer drops the held callback so it never fires", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const fired = { count: 0 };
  const id = pump.scheduleAfter(0, 3, () => {
    fired.count += 1;
  });
  pump.cancelTimer(id);
  assert.deepEqual(firingTicks(pump, fired, 6), []);
  assert.equal(fired.count, 0);
});

test("addTween samples onUpdate while active and fires onComplete at the end", () => {
  // fixedHz 10 + duration 0.3s -> 3 whole ticks; explicit ease + onComplete.
  const pump = new TickPump(new FakeBridge(), 10);
  const values: number[] = [];
  let completed = 0;
  pump.addTween(0, {
    duration: 0.3,
    ease: "linear",
    from: 0,
    onComplete: (): void => {
      completed += 1;
    },
    onUpdate: (value): void => {
      values.push(value);
    },
    to: 30,
  });
  // Active on ticks 1..3, sampled each (linear: 10, 20, 30); completes at tick 3.
  for (let tick = 0; tick <= 4; tick += 1) {
    pump.pump(tick);
  }
  assert.deepEqual(values, [10, 20, 30]);
  assert.equal(completed, 1);
});

test("addTween defaults the ease to linear and the completion sink to a no-op", () => {
  // No `ease` (orElse -> "linear") and no `onComplete` (orElse -> NO_COMPLETE):
  // the tween still samples and completes without an author completion sink.
  const pump = new TickPump(new FakeBridge(), 10);
  const values: number[] = [];
  pump.addTween(0, {
    duration: 0.2,
    from: 0,
    onUpdate: (value): void => {
      values.push(value);
    },
    to: 20,
  });
  for (let tick = 0; tick <= 3; tick += 1) {
    pump.pump(tick);
  }
  // Active on ticks 1..2, linear samples 10, 20; completion is the silent no-op.
  assert.deepEqual(values, [10, 20]);
});

test("cancelTween drops the held sinks so it stops sampling", () => {
  const pump = new TickPump(new FakeBridge(), 10);
  const values: number[] = [];
  const id = pump.addTween(0, {
    duration: 0.3,
    from: 0,
    onUpdate: (value): void => {
      values.push(value);
    },
    to: 30,
  });
  pump.cancelTween(id);
  for (let tick = 0; tick <= 3; tick += 1) {
    pump.pump(tick);
  }
  assert.deepEqual(values, []);
});

test("createMachine fires the initial onEnter and advances onUpdate each pumped tick", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const events: string[] = [];
  const states: readonly StateNode<"idle" | "run">[] = [
    {
      name: "idle",
      onEnter: (): void => {
        events.push("enter:idle");
      },
      onUpdate: (machine): void => {
        events.push(`update:${machine.current}`);
      },
    },
    { name: "run" },
  ];
  const machine = pump.createMachine(0, states, "idle");
  // onEnter fired at creation; current is the initial state.
  assert.equal(machine.current, "idle");
  assert.deepEqual(events, ["enter:idle"]);
  pump.pump(1);
  pump.pump(2);
  // The machine is advanced once per pumped tick, running onUpdate each time.
  assert.deepEqual(events, ["enter:idle", "update:idle", "update:idle"]);
  assert.equal(machine.ticksInState, 2);
});
