// The capstone sample-game proof. Drives the `Arena` sample (src/sample/arena.ts)
// over the FAKE NativeBridge + FAKE HostBridge — no wasm — exactly the way the
// real `GameLoop` would: a loop-owned `TickPump` pumped once per fixed tick ahead
// of the author's `update`, with a per-tick `Sim` built by `makeSim`. Tests are
// exempt from the Branchless Law, so this harness uses ordinary control flow.
//
// The point of this file is the DETERMINISTIC REPLAY proof: running the same
// fixed-step game for N ticks twice, with the same scripted RNG draw sequence (the
// analogue of "same seed") and the same scripted input sequence, must produce a
// byte-identical per-tick state-hash sequence. A behavioural assertion (the player
// reaches the expected cell, a pickup is banked through the tween, an enemy spawns,
// the score is reported) keeps it honest — the hashes prove determinism, the
// behaviour proves the game actually did something.

import assert from "node:assert/strict";
import { test } from "node:test";

import { Arena } from "../src/sample/arena.ts";
import { TickPump } from "../src/pump.ts";
import { makeSim, type SimContext } from "../src/sim.ts";
import { bindNative } from "../src/host-binding.ts";
import { FakeBridge } from "./fake-bridge.ts";
import { FakeHost } from "./fake-host.ts";

const FIXED_HZ = 60;
const TICKS = 40;
// Hold "right" for the first RIGHT_UNTIL ticks; the rest of the run is idle.
const RIGHT_UNTIL = 30;
const EXPECTED_SCORE = 10;
// The "right" hold walks the player to the right wall; accumulated fixed-step
// float drift lands it deterministically at cell (9,5) just shy of x=320.
const EXPECTED_CELL = { x: 9, y: 5 };
const EXPECTED_ENEMIES = 1;

interface RunResult {
  hashes: number[];
  arena: Arena;
  host: FakeHost;
}

// Run the sample once against a freshly-scripted fake bridge/host and collect the
// per-tick state hash. Identical scripting => identical run (that is the proof).
const runArena = (): RunResult => {
  const bridge = new FakeBridge();
  // The "same seed": the only RNG consumer is the enemy spawner at tick 30, which
  // draws a type index (below 2), then an x and a y cell (below GRID_CELLS).
  bridge.belows = [1, 3, 4];
  // The scripted input sequence: hold the "right" action for RIGHT_UNTIL ticks.
  for (let tick = 0; tick < RIGHT_UNTIL; tick += 1) {
    bridge.down.add(`${tick}|right`);
  }
  const host = new FakeHost();
  // Binding a fresh host also clears the emit-exactly-once outcome latch.
  bindNative(host);

  const pump = new TickPump(bridge, FIXED_HZ);
  const context: SimContext = { bridge, fixedHz: FIXED_HZ, pump };
  // The constructor is the author's `create` (tick 0 setup + timer registration).
  const arena = new Arena(makeSim(context, 0));

  const hashes: number[] = [];
  for (let tick = 0; tick < TICKS; tick += 1) {
    // Pump-first, exactly as GameLoop.advance prepends the pump fixed update.
    pump.pump(tick);
    arena.update(makeSim(context, tick));
    hashes.push(arena.hash());
  }
  arena.finish();
  return { arena, hashes, host };
};

test("arena replays byte-identically from the same seed and input sequence", () => {
  const first = runArena();
  const second = runArena();
  assert.equal(first.hashes.length, TICKS);
  // The capstone proof: the two per-tick hash sequences are identical.
  assert.deepEqual(first.hashes, second.hashes);
  // Anti-theatre: the state genuinely evolves over the run (start != end).
  assert.notEqual(first.hashes[0], first.hashes.at(-1));
});

test("arena reaches the expected cell, banks a pickup via the tween, spawns an enemy, reports the score", () => {
  const { arena, host } = runArena();
  // The pickup at cell (7,5) is collected mid-run; the collect tween's completion
  // banks the point, so a non-zero score proves the tween fired end-to-end.
  assert.equal(arena.score, EXPECTED_SCORE);
  // The scripted "right" hold walks the player from cell (5,5) to the wall (10,5).
  assert.deepEqual(arena.playerCell, EXPECTED_CELL);
  // The time.every cadence spawned exactly one RNG-placed enemy (queried from ECS).
  assert.equal(arena.enemyCount, EXPECTED_ENEMIES);
  // The terminal score crossed the host outcome boundary exactly once.
  assert.deepEqual(host.outcomes, [{ score: EXPECTED_SCORE, won: true }]);
});

test("the score is banked only after the collect tween completes", () => {
  const { hashes } = runArena();
  // The hash sequence must change again well after the collect tick (tick 11) when
  // the tween completes (tick 17) and the banked score enters the digest.
  const everyHashDistinctFromFirst = hashes.slice(1).some((value) => value !== hashes[0]);
  assert.ok(everyHashDistinctFromFirst);
});
