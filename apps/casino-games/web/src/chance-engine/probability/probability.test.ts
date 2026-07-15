/*
 * probability.test.ts — the statistical contract of the four mechanic
 * adapters, checked over large deterministic seed ranges (no live entropy):
 * observed win rates converge to the configured target; 0 never wins; 1
 * always wins; stochastic rounding converges to n·p; placements carry exactly
 * the committed number of winners; wheel arc mass equals compiled probability;
 * combinations always match their committed tier; destinations are final.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { baseConfig } from "../configuration/schema.ts";
import { planChoicePopulation } from "./choice-population.ts";
import type { CombinationSpace } from "./combination.ts";
import { losingCombinations, planCombination } from "./combination.ts";
import type { DestinationSlot } from "./destination.ts";
import { destinationProbabilities, planDestination } from "./destination.ts";
import { planSingleReveal } from "./single-reveal.ts";

const config = (p: number) => baseConfig("prob-test", "Prob Test", "showcase", {}, { targetWinRate: p });

const SEEDS = 4000;

test("choice population: stochastic rounding converges to n·p and placement is exact", () => {
  const p = 0.37;
  const n = 9;
  let winners = 0;
  for (let seed = 1; seed <= SEEDS; seed += 1) {
    const population = planChoicePopulation(config(p), n, seed, 1);
    const placed = population.winnersByIndex.filter((tier) => tier !== null).length;
    assert.equal(placed, population.winnerCount, "placement must contain exactly the committed number of winners");
    assert.ok(population.winnerCount === Math.floor(n * p) || population.winnerCount === Math.ceil(n * p));
    winners += population.winnerCount;
  }
  const meanWinners = winners / SEEDS;
  assert.ok(Math.abs(meanWinners - n * p) < 0.05, `mean winners ${meanWinners} should approach ${n * p}`);
});

test("choice population: realized selection win rate converges to p", () => {
  const p = 0.42;
  const n = 9;
  let wins = 0;
  for (let seed = 1; seed <= SEEDS; seed += 1) {
    const population = planChoicePopulation(config(p), n, seed, 1);
    // The player's pick is independent of placement; picking slot 0 is fair.
    wins += population.winnersByIndex[0] !== null ? 1 : 0;
  }
  const observed = wins / SEEDS;
  assert.ok(Math.abs(observed - p) < 0.03, `observed ${observed} vs target ${p}`);
});

test("choice population: p=0 never wins, p=1 always wins", () => {
  for (let seed = 1; seed <= 200; seed += 1) {
    assert.equal(planChoicePopulation(config(0), 9, seed, 1).winnerCount, 0);
    assert.equal(planChoicePopulation(config(1), 9, seed, 1).winnerCount, 9);
  }
});

const SLOTS: readonly DestinationSlot[] = [
  { id: "gold", mass: 1, tierId: "jackpot" },
  { id: "star", mass: 3, tierId: "common" },
  { id: "miss-a", mass: 2, tierId: null },
  { id: "miss-b", mass: 2, tierId: null },
];

test("destination: observed win rate converges to the target", () => {
  const p = 0.45;
  let wins = 0;
  for (let seed = 1; seed <= SEEDS; seed += 1) {
    const plan = planDestination(SLOTS, p, seed, 1);
    assert.equal(plan.win, plan.slot.tierId !== null);
    wins += plan.win ? 1 : 0;
  }
  const observed = wins / SEEDS;
  assert.ok(Math.abs(observed - p) < 0.03, `observed ${observed} vs target ${p}`);
});

test("destination: p=0 never wins, p=1 always wins; compiled mass matches", () => {
  for (let seed = 1; seed <= 200; seed += 1) {
    assert.equal(planDestination(SLOTS, 0, seed, 1).win, false);
    assert.equal(planDestination(SLOTS, 1, seed, 1).win, true);
  }
  const probabilities = destinationProbabilities(SLOTS, 0.45);
  const total = probabilities.reduce((sum, x) => sum + x, 0);
  assert.ok(Math.abs(total - 1) < 1e-9);
  // Winning arc mass = target; proportioned by slot mass within the group.
  assert.ok(Math.abs((probabilities[0]! + probabilities[1]!) - 0.45) < 1e-9);
  assert.ok(Math.abs(probabilities[0]! * 3 - probabilities[1]!) < 1e-9);
});

const SPACE: CombinationSpace = {
  reels: 2,
  symbolsPerReel: 6,
  winningCombos: [
    { combo: [5, 5], tierId: "jackpot" },
    { combo: [0, 0], tierId: "uncommon" },
    { combo: [1, 1], tierId: "uncommon" },
    { combo: [2, 4], tierId: "common" },
    { combo: [4, 2], tierId: "common" },
  ],
};

test("combination: committed combination always matches its committed tier", () => {
  const cfg = config(0.4);
  for (let seed = 1; seed <= 1000; seed += 1) {
    const plan = planCombination(cfg, SPACE, seed, 1);
    if (plan.win) {
      const match = SPACE.winningCombos.find((w) => w.combo.join(",") === plan.combination.join(","));
      assert.ok(match, "winning combination must be in the winning set");
      assert.equal(match.tierId, plan.tierId);
    } else {
      assert.equal(plan.tierId, null);
      assert.ok(!SPACE.winningCombos.some((w) => w.combo.join(",") === plan.combination.join(",")));
    }
  }
});

test("combination: win rate converges; p=0 never wins; p=1 always wins", () => {
  let wins = 0;
  for (let seed = 1; seed <= SEEDS; seed += 1) {
    wins += planCombination(config(0.4), SPACE, seed, 1).win ? 1 : 0;
    assert.equal(planCombination(config(0), SPACE, seed % 200, 1).win, false);
    assert.equal(planCombination(config(1), SPACE, seed % 200, 1).win, true);
  }
  const observed = wins / SEEDS;
  assert.ok(Math.abs(observed - 0.4) < 0.03, `observed ${observed} vs target 0.4`);
});

test("combination: losing combinations enumerate the exact complement", () => {
  const losers = losingCombinations(SPACE);
  assert.equal(losers.length, 36 - SPACE.winningCombos.length);
});

test("single reveal: converges, respects 0 and 1", () => {
  let wins = 0;
  for (let seed = 1; seed <= SEEDS; seed += 1) {
    wins += planSingleReveal(config(0.35), seed, 1).win ? 1 : 0;
    assert.equal(planSingleReveal(config(0), seed % 200, 1).win, false);
    assert.equal(planSingleReveal(config(1), seed % 200, 1).win, true);
  }
  const observed = wins / SEEDS;
  assert.ok(Math.abs(observed - 0.35) < 0.03, `observed ${observed} vs target 0.35`);
});

test("single reveal: a win always resolves a tier that counts as a win", () => {
  const cfg = config(0.5);
  for (let seed = 1; seed <= 500; seed += 1) {
    const plan = planSingleReveal(cfg, seed, 1);
    if (plan.win) {
      const tier = cfg.rewardTiers.find((t) => t.id === plan.tierId);
      assert.ok(tier !== undefined && tier.countsAsWin);
    }
  }
});
