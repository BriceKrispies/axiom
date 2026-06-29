import assert from "node:assert/strict";
import { test } from "node:test";

import { getSessionConfig, notifyReady, reportOutcome, reportOutcomes } from "../src/host.ts";
import { bindNative, boundHost } from "../src/host-binding.ts";
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
