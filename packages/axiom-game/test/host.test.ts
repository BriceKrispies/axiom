import assert from "node:assert/strict";
import { test } from "node:test";

import { getSessionConfig, notifyReady, reportOutcome, reportOutcomes } from "../src/host.ts";
import { bindNative, boundHost } from "../src/host-binding.ts";
import type { Vec3 } from "../src/vocabulary.ts";
import { FakeHost } from "./fake-host.ts";

const won = { score: 1, won: true };

// This test runs FIRST, before any bindNative, so `boundHost()` is the inert
// UNBOUND_HOST: every method is a safe no-op returning a neutral value. We call
// it directly (not through the latched free functions) so both inert terminal
// reporters are exercised despite the emit-once latch.
test("the unbound host surface is inert until a host is bound", () => {
  const inert = boundHost();
  assert.equal(inert.clamp(5, 0, 10), 5);
  assert.equal(inert.normalizeAngle(7), 7);
  assert.deepEqual(inert.overlapCircle(0, 0, 1), []);
  assert.deepEqual(inert.overlapBox({ x: 0, y: 0, z: 0 }, { x: 1, y: 1, z: 1 }), []);
  assert.equal(inert.raycast({ x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: 1 }, 1), undefined);
  assert.deepEqual(inert.getSessionConfig(), { params: {}, seed: 0n });
  assert.doesNotThrow(() => {
    inert.bindAction("noop", ["KeyN"]);
    inert.notifyReady();
    inert.reportOutcome(won);
    inert.reportOutcomes({});
  });
  // The inert audio surface returns a null handle and no-ops every signal.
  assert.equal(inert.loadSound("s.wav"), 0);
  assert.equal(inert.playSound(0), 0);
  assert.equal(inert.playMusic(["a", "b"]), 0);
  assert.equal(inert.playTone({ duration: 1, freq: 440, wave: "sine" }), 0);
  assert.equal(inert.scheduleSound(0, 1), 0);
  assert.doesNotThrow(() => {
    inert.stopVoice(0);
    inert.setMasterVolume(1);
    inert.setMuted(true);
  });
});

// The inert GRID / 3D / math surfaces (SPEC-06/11) before any host is bound: every
// read is a neutral total value, every signal a no-op. Runs before bindNative below.
test("the unbound grid / 3D / math surface is inert until a host is bound", () => {
  const inert = boundHost();
  const one: Vec3 = { x: 1, y: 1, z: 1 };
  const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

  const field = { cols: 1, passable: [true], rows: 1 };
  const origin = { x: 0, y: 0 };

  // Grid queries return neutral empties / origin.
  assert.deepEqual(inert.gridPath(field, origin, origin), []);
  assert.equal(inert.gridReachable(field, origin, origin), false);
  assert.deepEqual(inert.gridDistanceField(field, origin), []);
  assert.deepEqual(inert.gridStepToward(field, origin, origin), { x: 0, y: 0 });

  // 3D authoring mints a null handle / entity and no-ops the camera.
  assert.equal(inert.createMesh(0), 0);
  assert.equal(inert.createMaterial({ baseColor: [1, 1, 1, 1], emissive: [0, 0, 0, 0], opacity: 1, roughness: 1 }), 0);
  assert.equal(inert.addLight({ color: [1, 1, 1, 1], intensity: 1, kind: 0, vector: one }), 0);
  assert.doesNotThrow(() => {
    inert.setCamera3D({ far: 1, fovY: 1, near: 1, position: one, target: one });
  });

  // 3D math returns zero vectors / scalars / identity matrices / identity quaternion.
  assert.deepEqual(inert.v3Add(one, one), { x: 0, y: 0, z: 0 });
  assert.deepEqual(inert.v3Sub(one, one), { x: 0, y: 0, z: 0 });
  assert.deepEqual(inert.v3Scale(one, 2), { x: 0, y: 0, z: 0 });
  assert.deepEqual(inert.v3Cross(one, one), { x: 0, y: 0, z: 0 });
  assert.deepEqual(inert.v3Normalize(one), { x: 0, y: 0, z: 0 });
  assert.deepEqual(inert.v3Lerp(one, one, 0.5), { x: 0, y: 0, z: 0 });
  assert.equal(inert.v3Dot(one, one), 0);
  assert.equal(inert.v3Len(one), 0);
  assert.equal(inert.v3Dist(one, one), 0);
  assert.deepEqual(inert.mat4Identity(), identity);
  assert.deepEqual(inert.mat4Multiply(identity, identity), identity);
  assert.deepEqual(inert.mat4Perspective({ aspect: 1, far: 1, fovY: 1, near: 1 }), identity);
  assert.deepEqual(inert.mat4LookAt(one, one, one), identity);
  assert.deepEqual(inert.mat4Invert(identity), identity);
  assert.deepEqual(inert.mat4FromTRS(one, [0, 0, 0, 1], one), identity);
  assert.deepEqual(inert.quatIdentity(), [0, 0, 0, 1]);
  assert.deepEqual(inert.quatFromEuler(1, 1, 1), [0, 0, 0, 1]);
  assert.deepEqual(inert.quatMultiply([1, 0, 0, 0], [0, 1, 0, 0]), [0, 0, 0, 1]);
  assert.deepEqual(inert.quatNormalize([1, 0, 0, 0]), [0, 0, 0, 1]);
  assert.deepEqual(inert.quatToMat4([0, 0, 0, 1]), identity);
});

test("getSessionConfig forwards the host's constant session config", () => {
  const host = new FakeHost();
  host.config = { params: { mode: "duel", rounds: 3 }, seed: 4660n };
  bindNative(host);
  assert.deepEqual(getSessionConfig(), { params: { mode: "duel", rounds: 3 }, seed: 4660n });
});

test("notifyReady signals the host channel", () => {
  const host = new FakeHost();
  bindNative(host);
  notifyReady();
  notifyReady();
  assert.equal(host.readyCount, 2);
});

test("reportOutcome emits exactly once and drops later calls", () => {
  const host = new FakeHost();
  bindNative(host);
  reportOutcome(won);
  reportOutcome({ score: 0, won: false });
  assert.deepEqual(host.outcomes, [won]);
});

test("reportOutcomes emits exactly once", () => {
  const host = new FakeHost();
  bindNative(host);
  const results = { 1: won, 2: { score: 0, won: false } };
  reportOutcomes(results);
  reportOutcomes({ 3: won });
  assert.deepEqual(host.outcomeSets, [results]);
});

test("the terminal latch is shared across reportOutcome and reportOutcomes", () => {
  const host = new FakeHost();
  bindNative(host);
  reportOutcome(won);
  reportOutcomes({ 1: won });
  assert.deepEqual(host.outcomes, [won]);
  assert.deepEqual(host.outcomeSets, []);
});
