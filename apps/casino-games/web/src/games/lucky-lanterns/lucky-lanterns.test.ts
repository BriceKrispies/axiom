/*
 * lucky-lanterns.test.ts — the two contracts that matter for Lucky Lanterns:
 *
 * 1. Rise continuity + landing: the lantern's height-over-time is continuous
 *    (bounded per-tick delta) and ends inside the committed band's height range.
 * 2. Stream independence: the wind sway comes from the TRAJECTORY purpose only.
 *    Two rounds that differ ONLY in how much the AMBIENT stream is consumed
 *    resolve the same committed outcome AND settle into the same final band —
 *    decorative draws cannot perturb the result.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { sample01 } from "../../chance-engine/randomness/streams.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import { commitOutcome, createSession, transition } from "../../chance-engine/sessions/session.ts";
import {
  bandRange,
  committedBandIndex,
  DEFAULT_LANTERN_BANDS,
  destinationSlotsOf,
  lanternHeightAt,
  lanternSwayAt,
  riseTimeline,
} from "./game.ts";

// Tests never import definition.ts (it value-imports the engine via casino-mount);
// the config is rebuilt here from the shared schema + the game's default spec.
const testConfig = (): ReturnType<typeof baseConfig> =>
  baseConfig("lucky-lanterns", "Lucky Lanterns", "showcase", { bands: DEFAULT_LANTERN_BANDS }, { targetWinRate: 0.45 });

const commit = (seed: number, round: number): ReturnType<typeof commitOutcome> => {
  const config = testConfig();
  const source = new SeededChanceResultSource(seed);
  const slots = destinationSlotsOf(config.gameSpecific);
  const session = createSession(config, seed, round, source, { kind: "destination", slots });
  const ready = transition(session, "ready");
  const committing = transition(ready, "committing");
  return commitOutcome(committing, source, {});
};

test("the lantern rise is continuous and ends inside the committed band range", () => {
  const bandCount = DEFAULT_LANTERN_BANDS.length;
  const tl = riseTimeline(1, false);
  for (let round = 1; round <= 40; round += 1) {
    const committed = commit(0xa17e, round);
    const bandIndex = committedBandIndex(committed);
    const range = bandRange(bandCount, bandIndex);

    let maxDelta = 0;
    let prev = lanternHeightAt(range, tl, 0);
    for (let age = 1; age <= tl.total; age += 1) {
      const here = lanternHeightAt(range, tl, age);
      maxDelta = Math.max(maxDelta, Math.abs(here - prev));
      prev = here;
    }
    // A snap would jump several world units in one tick; the eased climb to the
    // highest band peaks near 0.3/tick, so this bound rejects teleports only.
    assert.ok(maxDelta < 0.4, `rise must be continuous (round ${round}, max delta ${maxDelta.toFixed(3)})`);

    const finalHeight = lanternHeightAt(range, tl, tl.total);
    assert.ok(finalHeight >= range.low - 1e-9 && finalHeight <= range.high + 1e-9, `must end inside band ${bandIndex}`);
  }
});

test("the wind sway is a pure function of the trajectory stream only", () => {
  // The sway must depend only on the committed presentation seed (trajectory).
  const committed = commit(55, 7);
  assert.ok(committed.committed !== null);
  const seed = committed.committed.presentationSeed;
  // Deterministic + independent of any ambient draws made elsewhere.
  const before = lanternSwayAt(seed, 30);
  // Consume the ambient stream heavily; sway must be unchanged.
  for (let i = 0; i < 100; i += 1) {
    sample01(seed, "ambient", i);
  }
  assert.equal(lanternSwayAt(seed, 30), before);
});

test("extra ambient-stream usage changes neither the outcome nor the final band", () => {
  // Two identical commitments; between them we drain the ambient stream. The
  // ambient stream feeds only decoration, so both must match exactly.
  const first = commit(0x1a27e, 12);
  for (let i = 0; i < 500; i += 1) {
    sample01(0x1a27e, "ambient", i, i * 3);
  }
  const second = commit(0x1a27e, 12);
  assert.ok(first.committed !== null && second.committed !== null);
  assert.equal(first.committed.win, second.committed.win);
  assert.equal(first.committed.tierId, second.committed.tierId);
  assert.equal(committedBandIndex(first), committedBandIndex(second), "the final band must be identical");
});

test("the default sky has at least one winning and one drift-away band", () => {
  assert.ok(DEFAULT_LANTERN_BANDS.some((b) => b.tierId !== null), "needs a winning band");
  assert.ok(DEFAULT_LANTERN_BANDS.some((b) => b.tierId === null), "needs a drift-away band");
});
