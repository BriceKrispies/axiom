/*
 * dice-vault.test.ts — the fairness/animation contract for Dice Vault: the
 * settled dice show EXACTLY the committed combination (no snap, no re-roll),
 * and every winning combination the space enumerates maps to a tier that
 * actually exists in the config. Runs under bare `node --test` (no DOM).
 */

import assert from "node:assert/strict";
import { test } from "node:test";

import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import { commitOutcome, createSession, transition } from "../../chance-engine/sessions/session.ts";
import {
  DEFAULT_DICE_SPEC,
  diceSpace,
  diceTimeline,
  dieRotationAt,
  upFaceOf,
} from "./game.ts";
import type { DiceSpec } from "./game.ts";

const configOf = (spec: DiceSpec): CasinoGameConfig<DiceSpec> =>
  baseConfig("dice-vault", "Dice Vault", "tabletop", spec, { targetWinRate: 0.5 });

/** Commit a round through a seeded source, returning the committed plan. */
const committedFor = (spec: DiceSpec, seed: number, round: number) => {
  const config = configOf(spec);
  const source = new SeededChanceResultSource(seed);
  const space = diceSpace(spec);
  const created = createSession(config, seed, round, source, { kind: "combination", space });
  const ready = transition(created, "ready");
  const committing = transition(ready, "committing");
  const committed = commitOutcome(committing, source, {});
  assert.notEqual(committed.committed, null);
  return committed.committed;
};

test("settled dice show exactly the committed combination across seeds", () => {
  const timeline = diceTimeline(1, false);
  let sawWin = false;
  let sawLoss = false;
  for (let seed = 1; seed <= 60; seed += 1) {
    const plan = committedFor(DEFAULT_DICE_SPEC, seed, 0);
    assert.ok(plan !== null);
    assert.equal(plan.manifestation.kind, "combination");
    const combination = plan.manifestation.kind === "combination" ? plan.manifestation.combination : [];
    sawWin ||= plan.win;
    sawLoss ||= !plan.win;
    combination.forEach((symbol, index) => {
      const settled = dieRotationAt(timeline.settleEnd, timeline, plan.presentationSeed, index, symbol);
      assert.equal(upFaceOf(settled), symbol + 1, `seed ${seed} die ${index}`);
    });
  }
  assert.ok(sawWin, "expected at least one winning roll");
  assert.ok(sawLoss, "expected at least one losing roll");
});

test("every enumerated winning combo maps to a tier present in the config", () => {
  const config = configOf(DEFAULT_DICE_SPEC);
  const tierIds = new Set(config.rewardTiers.map((tier) => tier.id));
  const space = diceSpace(DEFAULT_DICE_SPEC);
  assert.ok(space.winningCombos.length > 0);
  for (const combo of space.winningCombos) {
    assert.ok(tierIds.has(combo.tierId), `combo ${combo.combo.join(",")} → ${combo.tierId}`);
    assert.equal(combo.combo.length, DEFAULT_DICE_SPEC.diceCount);
  }
});

test("dice settle onto committed faces for one and three dice too", () => {
  const timeline = diceTimeline(1, false);
  const specs: readonly DiceSpec[] = [
    { combos: { allMaxTierId: "jackpot", allSameTierId: null, totals: [{ tierId: "common", total: 6 }] }, diceCount: 1 },
    { combos: { allMaxTierId: "jackpot", allSameTierId: "uncommon", totals: [{ tierId: "common", total: 10 }] }, diceCount: 3 },
  ];
  for (const spec of specs) {
    for (let seed = 1; seed <= 12; seed += 1) {
      const plan = committedFor(spec, seed, 0);
      assert.ok(plan !== null);
      const combination = plan.manifestation.kind === "combination" ? plan.manifestation.combination : [];
      assert.equal(combination.length, spec.diceCount);
      combination.forEach((symbol, index) => {
        const settled = dieRotationAt(timeline.settleEnd, timeline, plan.presentationSeed, index, symbol);
        assert.equal(upFaceOf(settled), symbol + 1);
      });
    }
  }
});
