import assert from "node:assert/strict";
import { test } from "node:test";

import { FakeBridge } from "./fake-bridge.testkit.ts";
import { TickPump } from "./pump.ts";
import { makeTime } from "./time.ts";

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

test("Time.after schedules a one-shot at T+delay on the running tick", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const fired = { count: 0 };
  // Built for tick 0; delay 3 -> due at tick 3.
  makeTime(pump, 0).after(3, () => {
    fired.count += 1;
  });
  assert.deepEqual(firingTicks(pump, fired, 6), [3]);
});

test("Time.every schedules a repeating timer on the running tick", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const fired = { count: 0 };
  makeTime(pump, 0).every(2, () => {
    fired.count += 1;
  });
  assert.deepEqual(firingTicks(pump, fired, 6), [2, 4, 6]);
});

test("Time.cancel stops a scheduled timer from firing", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const fired = { count: 0 };
  const time = makeTime(pump, 0);
  const id = time.after(3, () => {
    fired.count += 1;
  });
  time.cancel(id);
  assert.deepEqual(firingTicks(pump, fired, 6), []);
});

test("Time.createMachine mints a tick-driven machine and fires its initial onEnter", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const events: string[] = [];
  const machine = makeTime(pump, 0).createMachine(
    [
      {
        name: "idle",
        onEnter: (): void => {
          events.push("enter:idle");
        },
      },
      { name: "run" },
    ],
    "idle",
  );
  assert.equal(machine.current, "idle");
  assert.equal(machine.ticksInState, 0);
  assert.deepEqual(events, ["enter:idle"]);
});
