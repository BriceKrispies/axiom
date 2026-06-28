import assert from "node:assert/strict";
import { test } from "node:test";

import { GameLoop } from "../src/game-loop.ts";
import { stepFrame } from "../src/loop-core.ts";
import type { NativeBridge } from "../src/native-bridge.ts";
import { GameRegistry } from "../src/registry.ts";
import { makeFrame, makeSim } from "../src/sim.ts";
import { interpolationAlpha, type StepBudget } from "../src/step-budget.ts";

const budget = (steps: number, remainderNanos: number, fixedStepNanos: number): StepBudget => ({
  fixedStepNanos,
  remainderNanos,
  steps,
});

// A fake native bridge: replays a scripted budget per advance() and a fixed
// snapshot — no wasm needed, exactly how @axiom/client tests against a fake socket.
class FakeBridge implements NativeBridge {
  private readonly budgets: readonly StepBudget[];
  private readonly snap: Uint8Array;
  private index = 0;

  public constructor(budgets: readonly StepBudget[], snap: Uint8Array) {
    this.budgets = budgets;
    this.snap = snap;
  }

  public advance(): StepBudget {
    const next = this.budgets[this.index]!;
    this.index += 1;
    return next;
  }

  public snapshot(): Uint8Array {
    return this.snap;
  }
}

// A bitwise-free polynomial rolling hash over a tick sequence — the per-tick
// "state hash" the determinism proof compares across two runs.
const hashSeq = (values: readonly number[]): number =>
  values.reduce((hash, value) => (hash * 131 + value + 7) % 2_000_000_011, 17);

test("makeSim derives constant dt and exposes the subsystem stubs", () => {
  const sim = makeSim(60, 7);
  assert.equal(sim.tick, 7);
  assert.equal(sim.dt, 1 / 60);
  assert.equal(sim.rng.subsystem, "rng");
  assert.equal(sim.input.subsystem, "input");
  assert.equal(sim.world.subsystem, "world");
});

test("makeFrame carries the latest completed tick", () => {
  assert.equal(makeFrame(42).tick, 42);
});

test("interpolationAlpha is remainder over fixed step", () => {
  assert.equal(interpolationAlpha(budget(0, 250, 1000)), 0.25);
});

test("stepFrame runs N fixed updates then one render with alpha", () => {
  const sims: number[] = [];
  const alphas: number[] = [];
  const next = stepFrame({
    budget: budget(3, 500, 1000),
    fixedUpdates: [
      (sim): void => {
        sims.push(sim.tick);
      },
    ],
    makeFrame,
    makeSim: (tick) => makeSim(50, tick),
    renders: [
      (_frame, alpha): void => {
        alphas.push(alpha);
      },
    ],
    startTick: 10,
  });
  assert.deepEqual(sims, [10, 11, 12]);
  assert.deepEqual(alphas, [0.5]);
  assert.equal(next, 13);
});

test("stepFrame with zero steps renders once and advances no tick", () => {
  const sims: number[] = [];
  let renders = 0;
  const next = stepFrame({
    budget: budget(0, 0, 1000),
    fixedUpdates: [
      (sim): void => {
        sims.push(sim.tick);
      },
    ],
    makeFrame,
    makeSim: (tick) => makeSim(50, tick),
    renders: [
      (): void => {
        renders += 1;
      },
    ],
    startTick: 5,
  });
  assert.deepEqual(sims, []);
  assert.equal(renders, 1);
  assert.equal(next, 5);
});

test("a GameRegistry collects fixed-update and render callbacks in order", () => {
  const registry = new GameRegistry();
  const order: string[] = [];
  registry.onFixedUpdate(() => {
    order.push("f1");
  });
  registry.onFixedUpdate(() => {
    order.push("f2");
  });
  registry.onRender(() => {
    order.push("r1");
  });
  assert.equal(registry.fixedUpdates().length, 2);
  assert.equal(registry.renders().length, 1);
  registry.reset();
  assert.equal(registry.fixedUpdates().length, 0);
  assert.equal(registry.renders().length, 0);
});

test("GameLoop drives the bridge budget through the registry and tracks the tick", () => {
  const registry = new GameRegistry();
  const ticks: number[] = [];
  registry.onFixedUpdate((sim) => {
    ticks.push(sim.tick);
  });
  const snap = Uint8Array.from([1, 2, 3]);
  const loop = new GameLoop(
    new FakeBridge([budget(2, 0, 1000), budget(1, 0, 1000)], snap),
    60,
    registry,
  );

  const first = loop.advance(2000);
  assert.equal(first.steps, 2);
  assert.equal(loop.tick, 2);
  loop.advance(1000);
  assert.equal(loop.tick, 3);
  assert.deepEqual(ticks, [0, 1, 2]);
  assert.deepEqual([...loop.snapshot()], [1, 2, 3]);
});

test("a headless game reproduces its tick count and per-tick state-hash on replay", () => {
  // SPEC-00 §7: register onFixedUpdate/onRender, run N ticks headless, and assert
  // the tick count and a per-tick state-hash sequence reproduce on a second run.
  const run = (): { ticks: number; renders: number; hashes: number[] } => {
    const registry = new GameRegistry();
    const seen: number[] = [];
    const hashes: number[] = [];
    let renders = 0;
    registry.onFixedUpdate((sim) => {
      seen.push(sim.tick);
      hashes.push(hashSeq(seen));
    });
    registry.onRender(() => {
      renders += 1;
    });
    const budgets = Array.from({ length: 8 }, () => budget(1, 0, 1000));
    const loop = new GameLoop(new FakeBridge(budgets, Uint8Array.of()), 30, registry);
    const stepped = budgets.map(() => loop.advance(1000));
    assert.equal(stepped.length, budgets.length);
    return { hashes, renders, ticks: loop.tick };
  };
  const a = run();
  const b = run();
  assert.equal(a.ticks, 8);
  assert.equal(a.renders, 8);
  assert.deepEqual(a.hashes, b.hashes);
  // The fingerprints genuinely evolve tick to tick (no constant sequence).
  assert.notEqual(a.hashes[0], a.hashes[7]);
});
