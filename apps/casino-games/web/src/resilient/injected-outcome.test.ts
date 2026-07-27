/*
 * injected-outcome.test.ts — the server's answer must survive the trip into the
 * game unchanged.
 *
 * The engine-rendered rung animates a committed outcome it did not decide. The
 * risk that matters is therefore not "is the animation right" but "is the thing
 * being animated the thing the authority said" — so what is pinned here is that
 * every material fact is carried over verbatim, that nothing is invented when
 * the response is thin, and that the presentation seed is derived rather than
 * drawn.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { presentationSeedOf } from "../chance-engine/randomness/streams.ts";
import type { PickResponse } from "./contract.ts";
import { injectedOutcomeOf } from "./injected-outcome.ts";

const response = (overrides: Partial<PickResponse> = {}): PickResponse => ({
  board: Array.from({ length: 9 }, (unused, index) => ({ index, reward: null })),
  chestCount: 9,
  kind: "pick",
  picked: 2,
  replay: false,
  reward: null,
  round: 3,
  seed: 4242,
  targetWinRate: 0.44,
  winnerCount: 4,
  won: false,
  ...overrides,
});

const GEM = { rarity: "rare", rewardLabel: "Radiant gem", tierId: "rare", tierLabel: "Gem Trophy" } as const;

test("a win carries the authority's tier id through untouched", () => {
  const outcome = injectedOutcomeOf(response({ reward: GEM, won: true }));
  assert.equal(outcome.win, true);
  assert.equal(outcome.tierId, "rare");
});

test("a loss carries no tier, so no reward can be resolved for it", () => {
  const outcome = injectedOutcomeOf(response());
  assert.equal(outcome.win, false);
  assert.equal(outcome.tierId, null);
});

test("a win that names no tier reveals an empty chest rather than inventing a prize", () => {
  // The server never sends this; the point is that if it ever did, the page
  // shows nothing rather than picking a reward out of the ladder itself.
  const outcome = injectedOutcomeOf(response({ reward: null, won: true }));
  assert.equal(outcome.win, false);
  assert.equal(outcome.tierId, null);
});

test("the round identity is the authority's (seed, round), not a local counter", () => {
  assert.equal(injectedOutcomeOf(response()).roundId, "4242#3");
  assert.equal(injectedOutcomeOf(response({ round: 7, seed: 11 })).roundId, "11#7");
});

test("the presentation seed is DERIVED from the round, never drawn", () => {
  // Same function the seeded source uses, so the cosmetic streams behave
  // identically on both paths — and calling it twice is stable, which is what
  // makes a replayed round look the same as the round it replays.
  const outcome = injectedOutcomeOf(response());
  assert.equal(outcome.presentationSeed, presentationSeedOf(4242, 3));
  assert.equal(injectedOutcomeOf(response()).presentationSeed, outcome.presentationSeed);
  assert.notEqual(injectedOutcomeOf(response({ round: 4 })).presentationSeed, outcome.presentationSeed);
});
