/*
 * coin-fountain.test.ts — the two contracts that matter for Coin Fountain:
 *
 * 1. Continuity + landing: the token's arc is continuous (bounded per-tick
 *    delta) and ends inside the basin radius for ANY aim in the legal range and
 *    any charge strength — a clamped reticle means every toss lands in water.
 * 2. Fairness: the committed outcome is identical for two different aims under
 *    the same seed — aim is presentation context only, never the odds.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { OutcomeResolutionContext } from "../../chance-engine/outcomes/plan.ts";
import { commitOutcome, createSession, transition } from "../../chance-engine/sessions/session.ts";
import {
  clampAimToBasin,
  committedAim,
  committedStrength,
  DEFAULT_FOUNTAIN_SPEC,
  tokenAt,
  tossTimeline,
} from "./game.ts";

// Tests never import definition.ts (it value-imports the engine via casino-mount);
// the config is rebuilt here from the shared schema + the game's default spec.
const testConfig = (): ReturnType<typeof baseConfig> =>
  baseConfig("coin-fountain", "Coin Fountain", "showcase", DEFAULT_FOUNTAIN_SPEC, { targetWinRate: 0.4 });

const commitWith = (seed: number, round: number, context: OutcomeResolutionContext): ReturnType<typeof commitOutcome> => {
  const config = testConfig();
  const source = new SeededChanceResultSource(seed);
  const session = createSession(config, seed, round, source, { kind: "single" });
  const ready = transition(session, "ready");
  const committing = transition(ready, "committing");
  return commitOutcome(committing, source, context);
};

test("the reticle is always clamped inside the basin water", () => {
  const spec = DEFAULT_FOUNTAIN_SPEC;
  for (let x = -6; x <= 6; x += 0.5) {
    for (let z = -6; z <= 6; z += 0.5) {
      const aim = clampAimToBasin(spec, x, z);
      assert.ok(Math.hypot(aim.x, aim.z) <= spec.basinRadius + 1e-9, `aim (${x},${z}) must stay in the basin`);
    }
  }
});

test("the token arc is continuous and lands inside the basin for any legal aim", () => {
  const spec = DEFAULT_FOUNTAIN_SPEC;
  const tl = tossTimeline(1, false);
  const aims = [
    { x: 0, y: 0 },
    { x: 1, y: 0 },
    { x: -0.9, y: 0.7 },
    { x: 0.5, y: -0.8 },
    { x: -1, y: -1 },
  ];
  for (const rawAim of aims) {
    for (const strength of [0, 0.4, 1]) {
      const aim = clampAimToBasin(spec, rawAim.x * spec.basinRadius, rawAim.y * spec.basinRadius);
      let maxDelta = 0;
      let prev = tokenAt(spec, aim, strength, tl, 0);
      for (let age = 1; age <= tl.flightEnd; age += 1) {
        const here = tokenAt(spec, aim, strength, tl, age);
        maxDelta = Math.max(maxDelta, Math.hypot(here.x - prev.x, here.y - prev.y, here.z - prev.z));
        prev = here;
      }
      const landing = tokenAt(spec, aim, strength, tl, tl.flightEnd);
      assert.ok(Math.hypot(landing.x, landing.z) <= spec.basinRadius + 1e-9, "token must land in the basin");
      assert.ok(maxDelta < 0.5, `arc must be continuous (max delta ${maxDelta.toFixed(3)})`);
    }
  }
});

test("the committed outcome is identical for two different aims under one seed", () => {
  const seed = 0xf0017a1;
  for (let round = 1; round <= 40; round += 1) {
    const a = commitWith(seed, round, { aim: { x: -0.8, y: 0.6 }, launchStrength: 0.2 }).committed;
    const b = commitWith(seed, round, { aim: { x: 0.9, y: -0.4 }, launchStrength: 0.9 }).committed;
    assert.ok(a !== null && b !== null);
    assert.equal(a.win, b.win, `win must not depend on aim (round ${round})`);
    assert.equal(a.tierId, b.tierId, `tier must not depend on aim (round ${round})`);
  }
});

test("the committed aim + strength round-trip through the session context", () => {
  const committed = commitWith(99, 4, { aim: { x: 0.5, y: -0.5 }, launchStrength: 0.75 });
  const aim = committedAim(DEFAULT_FOUNTAIN_SPEC, committed);
  assert.ok(Math.hypot(aim.x, aim.z) <= DEFAULT_FOUNTAIN_SPEC.basinRadius);
  assert.equal(committedStrength(committed), 0.75);
});
