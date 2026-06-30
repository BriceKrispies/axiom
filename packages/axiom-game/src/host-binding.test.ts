import assert from "node:assert/strict";
import { test } from "node:test";

import { bindNative, boundHost, latchOutcome } from "./host-binding.ts";
import { FakeHost } from "./fake-host.testkit.ts";

// This test runs FIRST, before any bindNative, so `boundHost()` is the inert
// UNBOUND_HOST composed in this module — proving the free surface is total (no null
// bridge) before the app installs a real channel.
test("boundHost returns the inert composed default before any host is bound", () => {
  const inert = boundHost();
  assert.equal(inert.clamp(5, 0, 10), 5); // identity passthrough from the base
  assert.deepEqual(inert.getSessionConfig(), { params: {}, seed: 0n });
});

test("bindNative installs the native channel boundHost reads back", () => {
  const host = new FakeHost();
  bindNative(host);
  assert.equal(boundHost(), host);
});

test("latchOutcome returns true exactly once per session", () => {
  bindNative(new FakeHost()); // opens a fresh session, clearing the latch
  assert.equal(latchOutcome(), true);
  assert.equal(latchOutcome(), false);
  assert.equal(latchOutcome(), false);
});

test("bindNative reopens the session and clears the terminal latch", () => {
  // The previous test left the latch closed.
  assert.equal(latchOutcome(), false);
  bindNative(new FakeHost());
  assert.equal(latchOutcome(), true);
});
