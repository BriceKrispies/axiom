import assert from "node:assert/strict";
import { test } from "node:test";

import { BridgeStateMachine, type StateNode } from "./state-machine.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";

type State = "idle" | "run";

// Construct a machine directly over the fake bridge at tick 0: mint the native id,
// then bind the projection to the declared nodes (exactly what the pump does).
const machineOf = (
  bridge: FakeBridge,
  nodes: readonly StateNode<State>[],
  initial: State,
): BridgeStateMachine<State> => {
  const tick = 0;
  const initialIndex = nodes.map((node): State => node.name).indexOf(initial);
  const id = bridge.machineCreate(tick, nodes.length, initialIndex);
  return new BridgeStateMachine<State>({ bridge, id, nodes, tick });
};

test("enterInitial fires the initial state's onEnter and exposes current + ticksInState", () => {
  const events: string[] = [];
  const machine = machineOf(
    new FakeBridge(),
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
  machine.enterInitial();
  assert.equal(machine.current, "idle");
  assert.equal(machine.ticksInState, 0);
  assert.deepEqual(events, ["enter:idle"]);
});

test("advance runs the current state's onUpdate each tick and tracks ticksInState", () => {
  const events: string[] = [];
  const machine = machineOf(
    new FakeBridge(),
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
  machine.advance(1);
  machine.advance(2);
  assert.deepEqual(events, ["update:idle", "update:idle"]);
  // Entered at tick 0, last advanced to tick 2 -> 2 ticks in state.
  assert.equal(machine.ticksInState, 2);
});

test("transition fires the old onExit then the new onEnter and resets ticksInState", () => {
  const events: string[] = [];
  const machine = machineOf(
    new FakeBridge(),
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
      // `run` has no onUpdate/onExit -> exercises the absent-handler path.
      {
        name: "run",
        onEnter: (): void => {
          events.push("enter:run");
        },
      },
    ],
    "idle",
  );
  machine.enterInitial();
  machine.advance(1);
  machine.advance(2);
  // Transition at tick 2: idle.onExit, then run.onEnter; ticksInState resets.
  machine.transition("run");
  assert.equal(machine.current, "run");
  assert.equal(machine.ticksInState, 0);
  // run has no onUpdate: advancing adds no event.
  machine.advance(3);
  // Back to idle at tick 3: run has no onExit (absent, silent), idle.onEnter fires.
  machine.transition("idle");
  assert.equal(machine.current, "idle");
  assert.deepEqual(events, ["enter:idle", "exit:idle", "enter:run", "enter:idle"]);
});
