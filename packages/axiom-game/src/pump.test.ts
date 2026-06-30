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

// Run the SPEC-07 §7 reentrancy scenario once and return the firing event log.
// The pump reads the due id set ONCE per tick (`each(timersDue(tick), …)` snapshots
// the array), so a timer scheduled from inside a dispatched callback cannot fire
// within the same pass — even when its delay would put it due THIS tick.
const runReentrancyScenario = (): readonly string[] => {
  const pump = new TickPump(new FakeBridge(), 60);
  const events: string[] = [];
  let now = -1;
  let reenteredSamePass = false;

  // The outer one-shot is due at tick 2. When the pump dispatches it, it schedules
  // two inner one-shots: one with delay 0 (its `due === now`, the soonest a live
  // re-read could fire same-pass) and one with delay 1 (the canonical now+1).
  // Neither may run before the outer callback returns.
  pump.scheduleAfter(0, 2, (): void => {
    events.push(`outer@${now}`);
    const innerStart = now;
    pump.scheduleAfter(innerStart, 0, (): void => {
      events.push(`inner0@${now}`);
    });
    pump.scheduleAfter(innerStart, 1, (): void => {
      events.push(`inner1@${now}`);
    });
    // Inside this dispatch nothing inner has run: scheduling does not dispatch.
    reenteredSamePass = events.some((event) => event.startsWith("inner"));
  });

  const afterOuterTick: string[] = [];
  for (let tick = 0; tick <= 6; tick += 1) {
    now = tick;
    pump.pump(tick);
    // Snapshot events right after the outer's own due tick to prove the same-pass
    // guarantee survives the rest of pump(2), not just the callback.
    afterOuterTick.push(...[`tick2=${events.join(",")}`].filter(() => tick === 2));
  }
  assert.equal(reenteredSamePass, false);
  // After the whole of pump(2) only the outer has run — no inner re-entry.
  assert.deepEqual(afterOuterTick, ["tick2=outer@2"]);
  return events;
};

test("a timer scheduling another during its dispatch never re-enters the same due pass; the new timer comes due no earlier than now+1 (deterministic on replay)", () => {
  const first = runReentrancyScenario();
  const second = runReentrancyScenario();
  // inner1 (delay 1) lands at exactly now+1 (tick 3); inner0 (delay 0 -> due===now)
  // is skipped by the snapshot and never observed — the earliest a reentrant timer
  // can run is now+1. The sequence is byte-identical on replay.
  assert.deepEqual(first, ["outer@2", "inner1@3"]);
  assert.deepEqual(second, first);
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
