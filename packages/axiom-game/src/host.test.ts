import assert from "node:assert/strict";
import { test } from "node:test";

import { getSessionConfig, notifyReady, reportOutcome, reportOutcomes } from "./host.ts";
import { bindNative } from "./host-binding.ts";
import { FakeHost } from "./fake-host.testkit.ts";

const won = { score: 1, won: true };

test("getSessionConfig forwards the host's constant session config", () => {
  const host = new FakeHost();
  host.config = { params: { mode: "duel", rounds: 3 }, seed: 4660n };
  bindNative(host);
  assert.deepEqual(getSessionConfig(), { params: { mode: "duel", rounds: 3 }, seed: 4660n });
});

test("notifyReady signals the host channel each call", () => {
  const host = new FakeHost();
  bindNative(host);
  notifyReady();
  notifyReady();
  assert.equal(host.readyCount, 2);
});

test("reportOutcome emits exactly once and drops later calls", () => {
  const host = new FakeHost();
  bindNative(host); // a fresh session clears the terminal latch
  reportOutcome(won);
  reportOutcome({ score: 0, won: false });
  assert.deepEqual(host.outcomes, [won]);
});

test("reportOutcomes emits exactly once and drops later calls", () => {
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
