import assert from "node:assert/strict";
import { test } from "node:test";

import { FakeBridge } from "./fake-bridge.testkit.ts";
import { GameLoop } from "./game-loop.ts";
import { GameRegistry } from "./registry.ts";
import { Scene } from "./scene.ts";
import type { StepBudget } from "./step-budget.ts";

const budget = (steps: number): StepBudget => ({ fixedStepNanos: 1000, remainderNanos: 0, steps });

test("GameLoop drives the bridge budget through the registry and tracks the tick", () => {
  const registry = new GameRegistry();
  const ticks: number[] = [];
  registry.onFixedUpdate((sim) => {
    ticks.push(sim.tick);
  });
  const fake = new FakeBridge();
  fake.budgets = [budget(2), budget(1)];
  const loop = new GameLoop(fake, 60, registry);

  const first = loop.advance(2000);
  assert.equal(first.steps, 2);
  assert.equal(loop.tick, 2);
  loop.advance(1000);
  assert.equal(loop.tick, 3);
  // The registered fixed update ran once per fixed tick across both frames.
  assert.deepEqual(ticks, [0, 1, 2]);
});

test("GameLoop snapshot forwards the bridge's opaque durable bytes", () => {
  const fake = new FakeBridge();
  fake.snap = Uint8Array.from([1, 2, 3]);
  const loop = new GameLoop(fake, 60, new GameRegistry());
  assert.deepEqual([...loop.snapshot()], [1, 2, 3]);
});

test("GameLoop runs the mounted scene: create once, update per tick, assets from preload", () => {
  const calls: string[] = [];
  class DrivenScene extends Scene {
    public override preload(): readonly string[] {
      return ["atlas"];
    }

    public override create(): readonly number[] {
      calls.push("create");
      return [];
    }

    public override update(tick: number): readonly number[] {
      calls.push(`u${tick}`);
      return [];
    }
  }
  const fake = new FakeBridge();
  fake.budgets = [budget(2), budget(1)];
  const loop = new GameLoop(fake, 60, new GameRegistry()).mount(new DrivenScene());

  // Assets are empty until the first advance runs `preload`.
  assert.deepEqual(loop.assets(), []);
  loop.advance(1000);
  assert.deepEqual(loop.assets(), ["atlas"]);
  loop.advance(1000);
  // `create` ran exactly once before the first fixed update; `update` runs per tick.
  assert.deepEqual(calls, ["create", "u0", "u1", "u2"]);
});

test("GameLoop pumps a timer an author registers in a fixed update so it fires deterministically", () => {
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

  const counts = fake.budgets.map(() => {
    loop.advance(1000);
    return fired.length;
  });
  // Ticks 0..4 run; the timer set at tick 1 (delay 2) fires when tick 3 runs.
  assert.deepEqual(counts, [0, 0, 0, 1, 1]);
});
