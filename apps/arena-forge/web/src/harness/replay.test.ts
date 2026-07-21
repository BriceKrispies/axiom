import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "../sim/content/bundle.ts";
import { LocalMatchHost } from "../api/local-host.ts";
import { matchFingerprint, serializeReplay } from "./serialize.ts";
import { replayMatch } from "./replay.ts";

const content = loadDefaultContent();

test("a replay reproduces the original final state and event log byte-for-byte", () => {
  for (const seed of [1, 17, 202]) {
    const host = new LocalMatchHost({ seed, content, allBots: true });
    host.runToCompletion();
    const original = host.getMatch();
    const replay = serializeReplay(original);
    const replayed = replayMatch(content, replay);
    assert.equal(matchFingerprint(replayed), matchFingerprint(original), `replay diverged for seed ${seed}`);
  }
});

test("a single combat's event stream is recoverable from the replayed log", () => {
  const host = new LocalMatchHost({ seed: 5, content, allBots: true });
  host.runToCompletion();
  const replayed = replayMatch(content, serializeReplay(host.getMatch()));
  const combatIds = new Set(
    replayed
      .getEvents()
      .filter((e) => e.kind === "combat_begin")
      .map((e) => (e.kind === "combat_begin" ? e.combatId : -1)),
  );
  assert.ok(combatIds.size > 0);
  // Every combat's stream is exactly the log slice for its id (via EventSink).
  const first = [...combatIds][0]!;
  const stream = replayed.getEvents().filter((e) => "combatId" in e && e.combatId === first);
  assert.ok(stream.some((e) => e.kind === "combat_begin"));
  assert.ok(stream.some((e) => e.kind === "combat_end"));
});

test("a replay against an incompatible content version is rejected", () => {
  const host = new LocalMatchHost({ seed: 9, content, allBots: true });
  host.runToCompletion();
  const replay = { ...serializeReplay(host.getMatch()), contentVersion: 999 };
  assert.throws(() => replayMatch(content, replay), /content version/);
});
