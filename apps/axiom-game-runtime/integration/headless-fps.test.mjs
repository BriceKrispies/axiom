/*
 * Out-of-gate INTEGRATION proof for the SDK's FPS primitives, driving the REAL
 * wasm engine in Node (no browser). It is the non-browser analogue of the
 * `web/` harness that proves `boot.ts`: here we prove `headless.ts` — the
 * hand-cranked loop+input driver — composed with the new look-at camera seam
 * (`setCamera3D(position, target, …)`), end to end through the live
 * `axiom-game-runtime` wasm.
 *
 * This is NOT part of the @axiom/game coverage gate (it needs a wasm-bindgen
 * build, which node:test's unit suite never loads). It is an app-tier
 * integration test — see README.md for how to (re)build the bindings and run it.
 *
 * What it proves:
 *   1. Programmatic input injection reaches the native input system and surfaces
 *      to the author (`sim.input.axis` tracks the injected key holds).
 *   2. The headless driver steps the deterministic loop (`stepTicks` ⇒ tick count).
 *   3. The look-at camera call (`setCamera3D` with a moving target) executes live
 *      across the real wasm boundary every tick without throwing.
 *   4. Two identical runs are byte-for-byte deterministic (axis sequence + the
 *      native sim snapshot).
 */

import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";

import { bindAction, createGame, onFixedUpdate, setCamera3D } from "../../../packages/axiom-game/src/index.ts";
import { headless } from "../../../packages/axiom-game/src/headless.ts";

const require = createRequire(import.meta.url);

/*
 * The browser-targeted wasm opens a Web Audio `AudioContext` on the first
 * `advance` to realize its (here empty) sound batch — a browser sink Node lacks
 * and the headless path never uses. Stub the minimal surface `realize_into`
 * touches (createGain / gain.value / connect / destination / currentTime) so
 * driving the deterministic loop in Node doesn't trip on the audio side effect.
 * This shim proves nothing and asserts nothing — it just stands in for the
 * absent browser audio output.
 */
class StubAudioParam {
  value = 0;
}
class StubAudioNode {
  gain = new StubAudioParam();
  frequency = new StubAudioParam();
  connect(node) {
    return node;
  }
  setType() {}
  start() {}
  stop() {}
}
globalThis.AudioContext = class StubAudioContext {
  currentTime = 0;
  destination = new StubAudioNode();
  createGain() {
    return new StubAudioNode();
  }
  createOscillator() {
    return new StubAudioNode();
  }
};

const { WasmGame } = require("./pkg/axiom_game_runtime.js");

const FIXED_HZ = 60;
const FIXED_STEP_NANOS = BigInt(Math.round(1_000_000_000 / FIXED_HZ));
const MAX_STEPS = 8;
const TICKS = 60;
const HOLD_UNTIL = 30; // ArrowRight held for ticks [0, HOLD_UNTIL), released after
const TURN_PER_TICK = 0.05;

/**
 * Drive one full deterministic run of a tiny first-person scene: an author that
 * turns a yaw from the `turnLeft`/`turnRight` axis each tick and aims the camera
 * at the resulting look target. Returns the per-tick axis the author observed,
 * the final tick count, and the native sim snapshot.
 */
const runOnce = () => {
  const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS);
  const app = createGame({ fixedHz: FIXED_HZ, seed: 1n, surface: "headless" });

  const axisSeq = [];
  let yaw = 0;
  onFixedUpdate((sim) => {
    // Host-touching setup must run after `headless()` has bound the host, so it
    // lives inside the first fixed update rather than at registration time.
    if (sim.tick === 0) {
      bindAction("turnLeft", ["ArrowLeft"]);
      bindAction("turnRight", ["ArrowRight"]);
    }
    const turn = sim.input.axis("turnLeft", "turnRight");
    axisSeq.push(turn);
    yaw += turn * TURN_PER_TICK;
    // Aim the camera down the yaw — exercises the new look-at `target` seam live.
    setCamera3D({
      far: 100,
      fovY: Math.PI / 3,
      near: 0.1,
      position: { x: 0, y: 1, z: 0 },
      target: { x: Math.sin(yaw), y: 1, z: -Math.cos(yaw) },
    });
  });

  app.start();
  const driver = headless(game, app);

  // Hold "right" for the first stretch, then release — injected programmatically,
  // exactly as an agent (or the DOM edge) would, but with no browser.
  driver.key("ArrowRight", true);
  for (let t = 0; t < TICKS; t++) {
    if (t === HOLD_UNTIL) {
      driver.key("ArrowRight", false);
    }
    driver.stepTicks(1);
  }

  return { axisSeq, snapshot: driver.snapshot(), tick: driver.currentTick };
};

test("headless driver: injected input drives the author surface and steps the loop", () => {
  const run = runOnce();

  // (2) The loop advanced exactly one tick per stepTicks call.
  assert.equal(run.tick, TICKS);
  assert.equal(run.axisSeq.length, TICKS);

  // (1) Injected input reached the native input system and surfaced to the author:
  // the held "right" produced a positive axis at some point...
  assert.ok(
    run.axisSeq.includes(1),
    "the injected ArrowRight hold must surface as a positive turn axis",
  );
  // ...and releasing it returned the axis to neutral by the end.
  assert.ok(
    run.axisSeq.slice(-5).every((v) => v === 0),
    "releasing the key must return the turn axis to neutral",
  );
});

test("headless driver: identical runs are byte-for-byte deterministic", () => {
  const a = runOnce();
  const b = runOnce();

  // (4) The author-visible input sequence is identical run to run...
  assert.deepEqual(a.axisSeq, b.axisSeq);
  // ...and so is the native sim snapshot (the live wasm replays bit-for-bit).
  assert.deepEqual([...a.snapshot], [...b.snapshot]);
});
