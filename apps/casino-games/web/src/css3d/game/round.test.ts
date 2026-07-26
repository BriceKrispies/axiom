/*
 * round.test.ts — the CSS3D build's game layer is held to the SAME fairness
 * contract as the engine build, because it runs the same chance engine. These
 * tests pin the properties that matter: the population is decided up front, a
 * pick only looks its slot up, and the whole thing is a pure function of the
 * seed. No DOM is touched, so this runs under bare `node --test`.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { CHEST_COUNT, buildConfig, startRound } from "./round.ts";

test("the default config passes the real validation gate", () => {
  assert.deepEqual(startRound(1, 1, 0.44).issues, []);
  assert.equal(buildConfig(0.44).gameId, "treasure-chest-pick");
  assert.equal(buildConfig(0.44).choiceCount, CHEST_COUNT);
});

test("winner count is the stochastic rounding of n·p, so it brackets the target", () => {
  // 9 · 0.44 = 3.96 -> floor 3, plus a Bernoulli(0.96) extra: 3 or 4, never else.
  const counts = Array.from({ length: 40 }, (unused, round) => startRound(99, round + 1, 0.44).winnerCount);
  assert.ok(counts.every((count) => count === 3 || count === 4), `unexpected counts: ${[...new Set(counts)].join(",")}`);
  // and over many rounds the mean lands near the target
  const mean = counts.reduce((sum, count) => sum + count, 0) / counts.length / CHEST_COUNT;
  assert.ok(Math.abs(mean - 0.44) < 0.08, `realized rate ${mean} drifted from 0.44`);
});

test("the extremes are exact: rate 0 wins nothing, rate 1 wins everything", () => {
  assert.equal(startRound(7, 1, 0).winnerCount, 0);
  assert.equal(startRound(7, 1, 1).winnerCount, CHEST_COUNT);
  assert.ok(startRound(7, 1, 0).winnersByIndex.every((tier) => tier === null));
  assert.ok(startRound(7, 1, 1).winnersByIndex.every((tier) => tier !== null));
});

test("a round is a pure function of (seed, round, rate)", () => {
  const a = startRound(4242, 3, 0.44);
  const b = startRound(4242, 3, 0.44);
  assert.deepEqual(a.winnersByIndex, b.winnersByIndex);
  assert.deepEqual(
    Array.from({ length: CHEST_COUNT }, (unused, i) => a.reveal(i).label),
    Array.from({ length: CHEST_COUNT }, (unused, i) => b.reveal(i).label),
  );
});

test("advancing the round changes the population under the same seed", () => {
  const first = startRound(4242, 1, 0.44).winnersByIndex.join(",");
  const later = Array.from({ length: 8 }, (unused, i) => startRound(4242, i + 2, 0.44).winnersByIndex.join(","));
  assert.ok(later.some((population) => population !== first), "every round produced an identical population");
});

test("revealing only LOOKS UP the preassigned slot — it never rerolls", () => {
  const round = startRound(2024, 5, 0.44);
  Array.from({ length: CHEST_COUNT }, (unused, i) => i).forEach((i) => {
    const once = round.reveal(i);
    const twice = round.reveal(i);
    assert.equal(once.won, twice.won);
    assert.equal(once.tier?.id, twice.tier?.id);
    // and the reveal agrees with the population committed before any pick
    assert.equal(once.won, round.winnersByIndex[i] !== null);
  });
});

test("a win carries a real reward tier; a loss carries none", () => {
  const round = startRound(2024, 5, 0.44);
  const results = Array.from({ length: CHEST_COUNT }, (unused, i) => round.reveal(i));
  assert.equal(results.filter((result) => result.won).length, round.winnerCount);
  results.forEach((result) => {
    assert.equal(result.tier === null, !result.won);
    assert.equal(result.label === "", !result.won);
    if (result.tier !== null) assert.ok(result.tier.countsAsWin);
  });
});

test("decoration draws from the ambient stream, never from the outcome streams", () => {
  const round = startRound(11, 1, 0.44);
  const before = round.winnersByIndex.join(",");
  // Pulling any amount of ambient noise cannot perturb the committed population.
  Array.from({ length: 200 }, (unused, i) => round.ambient(i)).forEach((value) => {
    assert.ok(value >= 0 && value < 1, `ambient out of range: ${value}`);
  });
  assert.equal(round.winnersByIndex.join(","), before);
});
