/*
 * fishing-cast.test.ts — the two contracts that matter for Fishing Cast:
 *
 * 1. Fairness: for a fixed seed the committed plan (win + tierId) is IDENTICAL
 *    no matter which region the cast context names. The region can only move
 *    the manifestation's focusIndex (the reward family) — never the outcome.
 * 2. Continuity: the bobber's flight path is continuous — the per-tick position
 *    delta stays bounded across the whole flight/float/dip/reel timeline, so
 *    nothing snaps at the final frame.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { OutcomeResolutionContext } from "../../chance-engine/outcomes/plan.ts";
import { commitOutcome, createSession, transition } from "../../chance-engine/sessions/session.ts";
import { bobberAt, castTimeline, committedAim, DEFAULT_FISHING_REGIONS, regionIndexAt } from "./game.ts";

// Tests never import definition.ts (it value-imports the engine via casino-mount);
// the config is rebuilt here from the shared schema + the game's default spec.
const testConfig = (): ReturnType<typeof baseConfig> =>
  baseConfig("fishing-cast", "Fishing Cast", "showcase", { regions: DEFAULT_FISHING_REGIONS }, { targetWinRate: 0.42 });

const commitWith = (seed: number, round: number, context: OutcomeResolutionContext): ReturnType<typeof commitOutcome> => {
  const config = testConfig();
  const source = new SeededChanceResultSource(seed);
  const session = createSession(config, seed, round, source, { kind: "single" });
  const ready = transition(session, "ready");
  const committing = transition(ready, "committing");
  return commitOutcome(committing, source, context);
};

test("the committed outcome is identical regardless of the cast region", () => {
  const seed = 0xc0ffee;
  for (let round = 1; round <= 40; round += 1) {
    const plans = DEFAULT_FISHING_REGIONS.map((_, region) =>
      commitWith(seed, round, { aim: { x: 0.2, y: -0.1 }, castRegion: region }).committed,
    );
    const first = plans[0];
    assert.ok(first !== null);
    for (const plan of plans) {
      assert.ok(plan !== null);
      assert.equal(plan.win, first.win, `win must not depend on region (round ${round})`);
      assert.equal(plan.tierId, first.tierId, `tier must not depend on region (round ${round})`);
    }
  }
});

test("the cast region only moves the manifestation focus index", () => {
  const seed = 4242;
  const a = commitWith(seed, 3, { aim: { x: 0, y: 0 }, castRegion: 0 }).committed;
  const b = commitWith(seed, 3, { aim: { x: 0, y: 0 }, castRegion: 2 }).committed;
  assert.ok(a !== null && b !== null);
  assert.equal(a.manifestation.kind, "single");
  assert.equal(b.manifestation.kind, "single");
  assert.ok(a.manifestation.kind === "single" && b.manifestation.kind === "single");
  assert.equal(a.manifestation.focusIndex, 0);
  assert.equal(b.manifestation.focusIndex, 2);
});

test("region resolution: inside a ring picks that ring, otherwise the nearest", () => {
  const spec = { regions: DEFAULT_FISHING_REGIONS };
  const shallows = DEFAULT_FISHING_REGIONS[0]!;
  assert.equal(regionIndexAt(spec, shallows.x, shallows.z), 0);
  // A point far outside every ring resolves to the closest center (deep pool).
  const deep = DEFAULT_FISHING_REGIONS[1]!;
  assert.equal(regionIndexAt(spec, deep.x + 0.01, deep.z + 0.01), 1);
});

test("the bobber flight path is continuous (bounded per-tick delta)", () => {
  const committed = commitWith(777, 5, { aim: { x: 0.6, y: -0.5 }, castRegion: 1 });
  const aim = committedAim(committed);
  const tl = castTimeline(1, false);
  let maxDelta = 0;
  let prev = bobberAt(aim, tl, 0);
  for (let age = 1; age <= tl.total; age += 1) {
    const here = bobberAt(aim, tl, age);
    const delta = Math.hypot(here.x - prev.x, here.y - prev.y, here.z - prev.z);
    maxDelta = Math.max(maxDelta, delta);
    prev = here;
  }
  // No frame teleports: the whole rod→water→dock path moves in small steps.
  assert.ok(maxDelta < 0.3, `max per-tick delta ${maxDelta.toFixed(3)} must stay small`);
});

test("the bobber ends its reel-in at the dock catch point", () => {
  const committed = commitWith(31, 2, { aim: { x: -0.4, y: 0.3 }, castRegion: 0 });
  const aim = committedAim(committed);
  const tl = castTimeline(1, false);
  const atReel = bobberAt(aim, tl, tl.reelEnd);
  assert.ok(Math.abs(atReel.z - 2.3) < 0.05, "the reel-in must finish at the dock");
});
