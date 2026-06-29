import assert from "node:assert/strict";
import { test } from "node:test";

import { GameLoop } from "../src/game-loop.ts";
import { GameRegistry } from "../src/registry.ts";
import { TickPump } from "../src/pump.ts";
import { makeTime } from "../src/time.ts";
import type { StepBudget } from "../src/step-budget.ts";
import { FakeBridge } from "./fake-bridge.ts";

const budget = (steps: number): StepBudget => ({ fixedStepNanos: 1000, remainderNanos: 0, steps });

// Pump ticks 0..maxTick one at a time, recording the tick at which `fired` grows.
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

test("a one-shot timer set at tick T with delay D fires exactly at T+D", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const fired = { count: 0 };
  // Registered at tick 0 with delay 3 -> due at tick 3.
  makeTime(pump, 0).after(3, () => {
    fired.count += 1;
  });
  assert.deepEqual(firingTicks(pump, fired, 6), [3]);
  assert.equal(fired.count, 1);
});

test("an every(n) timer re-fires on each multiple of the interval", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const fired = { count: 0 };
  // Registered at tick 0, interval 2 -> fires at 2, 4, 6.
  makeTime(pump, 0).every(2, () => {
    fired.count += 1;
  });
  assert.deepEqual(firingTicks(pump, fired, 6), [2, 4, 6]);
});

test("cancel stops a timer from ever firing", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const fired = { count: 0 };
  const time = makeTime(pump, 0);
  const id = time.after(3, () => {
    fired.count += 1;
  });
  time.cancel(id);
  assert.deepEqual(firingTicks(pump, fired, 6), []);
  assert.equal(fired.count, 0);
});

test("a timer registered in onFixedUpdate fires deterministically through the loop", () => {
  const registry = new GameRegistry();
  const fired: number[] = [];
  registry.onFixedUpdate((sim) => {
    // Register a delay-2 timer once, during the tick-1 update.
    if (sim.tick === 1) {
      sim.time.after(2, () => {
        fired.push(sim.tick);
      });
    }
  });
  const fake = new FakeBridge();
  fake.budgets = Array.from({ length: 5 }, () => budget(1));
  const loop = new GameLoop(fake, 60, registry);

  // Advance one tick at a time; record how many fires have happened after each.
  const counts = fake.budgets.map(() => {
    loop.advance(1000);
    return fired.length;
  });
  // Ticks run are 0,1,2,3,4; the timer registered at tick 1 (delay 2) fires when
  // tick 3 runs — the 4th advance — and never again.
  assert.deepEqual(counts, [0, 0, 0, 1, 1]);
});

test("a state machine fires onEnter at creation and exposes its current state", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const events: string[] = [];
  const machine = makeTime(pump, 0).createMachine(
    [
      {
        name: "idle",
        onEnter: (): void => {
          events.push("enter:idle");
        },
        onExit: (): void => {
          events.push("exit:idle");
        },
        onUpdate: (): void => {
          events.push("update:idle");
        },
      },
      {
        name: "run",
        onEnter: (): void => {
          events.push("enter:run");
        },
      },
    ],
    "idle",
  );
  assert.equal(machine.current, "idle");
  assert.equal(machine.ticksInState, 0);
  assert.deepEqual(events, ["enter:idle"]);
});

test("a state machine runs onUpdate each pumped tick and tracks ticksInState", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const events: string[] = [];
  const machine = makeTime(pump, 0).createMachine(
    [
      {
        name: "idle",
        onUpdate: (sm): void => {
          events.push(`update:${sm.current}`);
        },
      },
      { name: "run" },
    ],
    "idle",
  );
  pump.pump(1);
  pump.pump(2);
  assert.deepEqual(events, ["update:idle", "update:idle"]);
  // Entered at tick 0, last pumped at tick 2 -> 2 ticks in state.
  assert.equal(machine.ticksInState, 2);
});

test("transition fires the old state's onExit then the new state's onEnter, and resets ticksInState", () => {
  const pump = new TickPump(new FakeBridge(), 60);
  const events: string[] = [];
  const machine = makeTime(pump, 0).createMachine(
    [
      {
        name: "idle",
        onEnter: (): void => {
          events.push("enter:idle");
        },
        onExit: (): void => {
          events.push("exit:idle");
        },
      },
      // `run` has no onExit/onUpdate — exercises the absent-handler path.
      {
        name: "run",
        onEnter: (): void => {
          events.push("enter:run");
        },
      },
    ],
    "idle",
  );
  pump.pump(1);
  pump.pump(2);
  machine.transition("run");
  assert.equal(machine.current, "run");
  assert.equal(machine.ticksInState, 0);
  // run.onUpdate is absent: pumping does not add an event.
  pump.pump(3);
  // Back to idle: run.onExit is absent (no exit:run), idle.onEnter fires again.
  machine.transition("idle");
  assert.equal(machine.current, "idle");
  assert.deepEqual(events, ["enter:idle", "exit:idle", "enter:run", "enter:idle"]);
});
