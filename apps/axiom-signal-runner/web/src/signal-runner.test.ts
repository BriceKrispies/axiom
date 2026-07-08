/*
 * Deterministic game-logic tests for Signal Runner. These run in bare Node
 * (`node --test`, native TS type-stripping) because the whole core imports nothing
 * from `@axiom/game` — no wasm, no browser. They pin the 15 behaviors the brief
 * requires: seeded generation, objective counts, collection, plates, the beacon
 * gate, the storm timer, every ability, restart, the HUD model, and replay equality.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { BASE_SPEED, PLATE_GOAL, SHARD_GOAL } from "./constants.ts";
import { type Intent, IDLE_INTENT } from "./types.ts";
import { SignalRunnerGame } from "./game.ts";
import { formatTimer } from "./hud.ts";
import { generateLevel } from "./level.ts";
import { initialState } from "./state.ts";
import { stepSim } from "./sim.ts";

const intent = (over: Partial<Intent>): Intent => ({ ...IDLE_INTENT, ...over });

test("1. seeded level generation is deterministic", () => {
  assert.deepEqual(generateLevel(7), generateLevel(7));
  assert.notDeepEqual(generateLevel(7).nodes, generateLevel(8).nodes);
});

test("2. generated level has exactly 20 shards and 3 plates", () => {
  const level = generateLevel(1);
  assert.equal(level.shards.length, SHARD_GOAL);
  assert.equal(level.plates.length, PLATE_GOAL);
});

test("3. generated level has a final beacon at the end", () => {
  const level = generateLevel(1);
  assert.ok(level.beaconZ > 0);
  assert.ok(level.beaconZ <= level.length);
  assert.ok(level.beaconZ > level.length * 0.9);
});

test("4. collecting a shard increments count and charge", () => {
  const level = generateLevel(1);
  const s = initialState(level);
  const shard = level.shards[0]!;
  s.runner.dist = shard.z - 1;
  s.runner.lateral = shard.lateral;
  s.runner.speed = 200;
  const chargeBefore = s.runner.charge;
  stepSim(s, IDLE_INTENT);
  assert.equal(s.shardsCollected, 1);
  assert.equal(level.shards[0]!.collected, true);
  assert.ok(s.runner.charge > chargeBefore);
});

test("5. activating a plate increments the plate count exactly once", () => {
  const level = generateLevel(1);
  const s = initialState(level);
  const plate = level.plates[0]!;
  s.runner.dist = plate.z - 1;
  s.runner.lateral = 0;
  s.runner.speed = 200;
  stepSim(s, IDLE_INTENT);
  assert.equal(s.platesActivated, 1);
  stepSim(s, IDLE_INTENT);
  assert.equal(s.platesActivated, 1);
});

test("6. the beacon cannot be restored before shard/plate requirements are met", () => {
  const g = new SignalRunnerGame(1);
  g.state.runner.dist = g.state.level.beaconZ - 100;
  g.step(intent({ confirm: true }));
  assert.equal(g.state.phase, "run");
  assert.equal(g.state.beaconRestored, false);
});

test("7. the beacon restores once requirements are met and Enter is pressed", () => {
  const g = new SignalRunnerGame(1);
  g.state.runner.dist = g.state.level.beaconZ - 100;
  g.state.shardsCollected = SHARD_GOAL;
  g.state.platesActivated = PLATE_GOAL;
  g.step(intent({ confirm: true }));
  assert.equal(g.state.phase, "win");
  assert.equal(g.state.beaconRestored, true);
});

test("8. the timer reaching zero causes a game over", () => {
  const s = initialState(generateLevel(1));
  s.timeLeft = 0.005;
  stepSim(s, IDLE_INTENT);
  assert.equal(s.phase, "lose");
  assert.equal(s.loseReason, "time");
});

test("9. boost consumes charge and increases speed", () => {
  const boosted = initialState(generateLevel(1));
  boosted.runner.charge = 1;
  boosted.runner.dist = 500;
  boosted.runner.speed = BASE_SPEED;
  stepSim(boosted, intent({ boost: true }));
  assert.ok(boosted.runner.charge < 1);
  assert.ok(boosted.runner.boostTicks > 0);

  const plain = initialState(generateLevel(1));
  plain.runner.dist = 500;
  plain.runner.speed = BASE_SPEED;
  stepSim(plain, IDLE_INTENT);
  assert.ok(boosted.runner.speed > plain.runner.speed);
});

test("10. shield absorbs one crash", () => {
  const level = generateLevel(1);
  assert.ok(level.obstacles.length > 0);
  const o = level.obstacles[0]!;

  const shielded = initialState(level);
  shielded.runner.dist = o.z - 1;
  shielded.runner.lateral = o.lateral;
  shielded.runner.speed = 200;
  shielded.runner.shieldTicks = 120;
  stepSim(shielded, IDLE_INTENT);
  assert.equal(shielded.runner.crashes, 0);
  assert.equal(shielded.runner.shieldTicks, 0);

  const level2 = generateLevel(1);
  const o2 = level2.obstacles[0]!;
  const bare = initialState(level2);
  bare.runner.dist = o2.z - 1;
  bare.runner.lateral = o2.lateral;
  bare.runner.speed = 200;
  stepSim(bare, IDLE_INTENT);
  assert.equal(bare.runner.crashes, 1);
});

test("11. pulse disables nearby drone hazards", () => {
  const level = generateLevel(1);
  assert.ok(level.drones.length > 0);
  const s = initialState(level);
  const d = level.drones[0]!;
  s.runner.dist = d.z - 50;
  s.runner.lateral = d.baseLateral;
  s.runner.charge = 1;
  stepSim(s, intent({ pulse: true }));
  assert.equal(level.drones[0]!.disabled, true);
});

test("12. the helper drone collects a nearby shard deterministically", () => {
  const base = generateLevel(1);
  // Isolate the helper: no obstacles/drones so the (parked) runner can't crash.
  const level = { ...base, drones: [], obstacles: [] };
  const s = initialState(level);
  const shard = level.shards[6]!;
  s.runner.dist = shard.z - 350;
  s.runner.lateral = shard.lateral + 100; // far enough that the runner itself never collects it
  s.runner.speed = 0;
  s.runner.charge = 1;
  stepSim(s, intent({ drone: true }));
  for (let i = 0; i < 90; i += 1) {
    stepSim(s, IDLE_INTENT);
  }
  assert.equal(shard.collected, true);
});

test("13. restart returns to the exact initial deterministic state", () => {
  const g = new SignalRunnerGame(3);
  const start = g.hash();
  for (let i = 0; i < 150; i += 1) {
    g.step(intent({ boost: i === 10, steer: (i % 2) * 2 - 1 }));
  }
  g.restart();
  assert.equal(g.hash(), start);
  assert.equal(g.hash(), new SignalRunnerGame(3).hash());
});

test("14. the HUD model reflects the current game state", () => {
  const g = new SignalRunnerGame(1);
  g.state.shardsCollected = 8;
  g.state.platesActivated = 1;
  g.state.timeLeft = 83.4;
  const hud = g.hud();
  assert.equal(hud.shards, 8);
  assert.equal(hud.plates, 1);
  assert.equal(hud.timer, "01:23.4");
  assert.equal(hud.abilities.length, 4);
  assert.equal(hud.nodes.filter((n) => n.kind === "shard").length, SHARD_GOAL);
  assert.equal(hud.nodes.filter((n) => n.kind === "beacon").length, 1);
});

test("15. collision + off-path behavior is deterministic and replayable", () => {
  const run = (seed: number): number[] => {
    const g = new SignalRunnerGame(seed);
    const hs: number[] = [];
    for (let i = 0; i < 300; i += 1) {
      g.step(intent({ boost: i === 50, brake: i % 7 === 0, drone: i === 60, pulse: i === 120, steer: (i % 3) - 1 }));
      hs.push(g.hash());
    }
    return hs;
  };
  assert.deepEqual(run(5), run(5));

  const s = initialState(generateLevel(1));
  s.runner.lateral = 5000;
  stepSim(s, IDLE_INTENT);
  assert.equal(s.phase, "lose");
  assert.equal(s.loseReason, "fell");
});
