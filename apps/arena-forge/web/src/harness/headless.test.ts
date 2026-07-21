import { strict as assert } from "node:assert";
import { test } from "node:test";

import { loadDefaultContent } from "../sim/content/bundle.ts";
import { LocalMatchHost } from "../api/local-host.ts";
import { runManyMatches, runSeededMatch } from "./headless.ts";

const content = loadDefaultContent();

const fingerprint = (host: LocalMatchHost): string =>
  JSON.stringify({ state: host.getMatch().state, events: host.getMatch().getEvents() });

test("same seed + command stream produces byte-identical final state and events", () => {
  const a = new LocalMatchHost({ seed: 4242, content, allBots: true });
  const b = new LocalMatchHost({ seed: 4242, content, allBots: true });
  a.runToCompletion();
  b.runToCompletion();
  assert.equal(fingerprint(a), fingerprint(b));
});

test("different seeds produce controlled variation (not all identical winners)", () => {
  const winners = new Set<number>();
  for (let seed = 1; seed <= 12; seed += 1) {
    const { report } = runSeededMatch(content, seed);
    assert.equal(report.complete, true);
    if (report.winner !== null) {
      winners.add(report.winner);
    }
  }
  assert.ok(winners.size >= 2, `expected varied winners across seeds, saw ${winners.size}`);
});

test("all 100 seeded matches complete, legally, within the round cap, with no negative gold", () => {
  const suite = runManyMatches(content, 100, 1);
  assert.equal(suite.allComplete, true, "every match must reach match_complete");
  assert.equal(suite.illegalTransitions, 0, "no illegal phase transitions");
  assert.equal(suite.negativeGoldMatches, 0, "gold never goes negative");
  assert.ok(suite.maxRoundsSeen <= content.version + 60, "matches must finish within the round cap");
  assert.ok(suite.avgRounds > 1, "matches should last multiple rounds");
  // Report (not assert) forge + bound + group usage so a human sees the shape.
  console.log(
    `[suite] avgRounds=${suite.avgRounds.toFixed(1)} avgElimRound=${suite.avgEliminationRound.toFixed(1)} forgedTotal=${suite.forgedTotal} boundCombats=${suite.boundCombats}`,
  );
  console.log(`[suite] winnerDistribution=${JSON.stringify(suite.winnerDistribution)}`);
  console.log(`[suite] groupUsage=${JSON.stringify(suite.groupUsage)}`);
});

test("every group is used across the suite (archetypes are all reachable)", () => {
  const suite = runManyMatches(content, 30, 500);
  for (const group of content.groups) {
    assert.ok((suite.groupUsage[group.id] ?? 0) > 0, `group ${group.id} was never purchased`);
  }
});
