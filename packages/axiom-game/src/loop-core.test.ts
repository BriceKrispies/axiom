import assert from "node:assert/strict";
import { test } from "node:test";

import { type SimContext, makeFrame, makeSim } from "./sim.ts";
import type { StepBudget } from "./step-budget.ts";
import { FakeBridge } from "./fake-bridge.testkit.ts";
import { TickPump } from "./pump.ts";
import { stepFrame } from "./loop-core.ts";

const budget = (steps: number, remainderNanos: number, fixedStepNanos: number): StepBudget => ({
  fixedStepNanos,
  remainderNanos,
  steps,
});

// A SimContext over a fresh fake bridge at `fixedHz`, with its own standalone pump.
const contextOf = (bridge: FakeBridge, fixedHz: number): SimContext => ({
  bridge,
  fixedHz,
  pump: new TickPump(bridge, fixedHz),
});

test("stepFrame runs budget.steps fixed updates at consecutive ticks, then one render", () => {
  const fake = new FakeBridge();
  const sims: number[] = [];
  const alphas: number[] = [];
  let renders = 0;
  const next = stepFrame({
    budget: budget(3, 500, 1000),
    fixedUpdates: [
      (sim): void => {
        sims.push(sim.tick);
      },
    ],
    makeFrame,
    makeSim: (tick): ReturnType<typeof makeSim> => makeSim(contextOf(fake, 50), tick),
    renders: [
      (_frame, alpha): void => {
        alphas.push(alpha);
        renders += 1;
      },
    ],
    startTick: 10,
  });
  // Three updates at 10,11,12; one render with alpha 0.5; next start tick = 13.
  assert.deepEqual(sims, [10, 11, 12]);
  assert.deepEqual(alphas, [0.5]);
  assert.equal(renders, 1);
  assert.equal(next, 13);
});

test("stepFrame runs every registered fixed update for each step", () => {
  const fake = new FakeBridge();
  const log: string[] = [];
  stepFrame({
    budget: budget(2, 0, 1000),
    fixedUpdates: [
      (sim): void => {
        log.push(`a${sim.tick}`);
      },
      (sim): void => {
        log.push(`b${sim.tick}`);
      },
    ],
    makeFrame,
    makeSim: (tick): ReturnType<typeof makeSim> => makeSim(contextOf(fake, 50), tick),
    renders: [],
    startTick: 0,
  });
  // Both callbacks run, in registration order, at each of the two ticks.
  assert.deepEqual(log, ["a0", "b0", "a1", "b1"]);
});

test("stepFrame with zero steps renders once and advances no tick", () => {
  const fake = new FakeBridge();
  const sims: number[] = [];
  const frameTicks: number[] = [];
  const next = stepFrame({
    budget: budget(0, 0, 1000),
    fixedUpdates: [
      (sim): void => {
        sims.push(sim.tick);
      },
    ],
    makeFrame,
    makeSim: (tick): ReturnType<typeof makeSim> => makeSim(contextOf(fake, 50), tick),
    renders: [
      (frame): void => {
        frameTicks.push(frame.tick);
      },
    ],
    startTick: 5,
  });
  // No fixed update runs; the render still fires once at the unchanged tick.
  assert.deepEqual(sims, []);
  assert.deepEqual(frameTicks, [5]);
  assert.equal(next, 5);
});
