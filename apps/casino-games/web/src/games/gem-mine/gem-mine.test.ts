/*
 * gem-mine.test.ts — the Gem Mine fairness/ordering pins: crack stages strictly
 * precede the break (the timeline is monotone: approach → strikes → break →
 * reveal), fragment trajectories are a pure function of the presentation seed,
 * and reduced motion PRESERVES that ordering while shortening durations. Runs
 * under bare `node --test` with no DOM — the timeline and ballistics are pure.
 */

import assert from "node:assert/strict";
import test from "node:test";
import {
  CRACK_STAGES,
  crackStagesAt,
  FRAGMENT_MAX,
  FRAGMENT_MIN,
  fragmentCount,
  fragmentPose,
  mineTimeline,
} from "./game.ts";
import { v3 } from "../../presentation/stage/vectors.ts";

test("gem-mine: crack stages strictly precede fragmentation", () => {
  const timeline = mineTimeline(1, false);
  // Ordering invariant: approach < strike0 < strike1 < strike2 = break < reveal.
  assert.ok(timeline.approachEnd < timeline.strikes[0]);
  assert.ok(timeline.strikes[0] < timeline.strikes[1]);
  assert.ok(timeline.strikes[1] < timeline.strikes[2]);
  assert.equal(timeline.strikes[2], timeline.breakAt);
  assert.ok(timeline.breakAt < timeline.revealEnd);
  assert.ok(timeline.revealEnd < timeline.total);

  // All three crack stages have landed by the break, none before the first strike.
  assert.equal(crackStagesAt(timeline, timeline.approachEnd), 0);
  assert.equal(crackStagesAt(timeline, timeline.strikes[0]), 1);
  assert.equal(crackStagesAt(timeline, timeline.strikes[1]), 2);
  assert.equal(crackStagesAt(timeline, timeline.breakAt), CRACK_STAGES);
});

test("gem-mine: fragment trajectories are a pure function of the presentation seed", () => {
  const origin = v3(1, 0, -0.5);
  const seed = 0x0f_1e_2d_3c;
  const n = fragmentCount(seed);
  assert.ok(n >= FRAGMENT_MIN && n <= FRAGMENT_MAX);

  for (let age = 0; age <= 60; age += 12) {
    for (let i = 0; i < n; i += 1) {
      const a = fragmentPose(origin, seed, i, age);
      const b = fragmentPose(origin, seed, i, age);
      assert.deepEqual(a, b);
    }
  }

  // A different seed yields a different flight (not a constant path).
  const other = fragmentPose(origin, seed ^ 0xffff, 0, 30);
  const same = fragmentPose(origin, seed, 0, 30);
  assert.notDeepEqual(other, same);

  // Fragments start at the origin height band and rise before falling.
  const start = fragmentPose(origin, seed, 0, 0);
  const mid = fragmentPose(origin, seed, 0, 18);
  assert.ok(mid.position.y > start.position.y);
});

test("gem-mine: reduced motion preserves ordering while shortening durations", () => {
  const full = mineTimeline(1, false);
  const reduced = mineTimeline(1, true);

  // Reduced motion is shorter end to end...
  assert.ok(reduced.total < full.total);
  assert.ok(reduced.breakAt < full.breakAt);

  // ...but the strike → crack → break → reveal ordering is intact.
  assert.ok(reduced.approachEnd < reduced.strikes[0]);
  assert.ok(reduced.strikes[0] < reduced.strikes[1]);
  assert.ok(reduced.strikes[1] < reduced.strikes[2]);
  assert.equal(reduced.strikes[2], reduced.breakAt);
  assert.ok(reduced.breakAt < reduced.revealEnd);
  assert.equal(crackStagesAt(reduced, reduced.breakAt), CRACK_STAGES);
});
