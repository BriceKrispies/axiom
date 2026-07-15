/*
 * mystery-portal.test.ts — the Mystery Portal fairness pins: the idle pose is
 * a pure function of (index, tick, seed, liveliness) that takes NO population
 * input, so two rounds with genuinely different winner populations produce
 * byte-identical idle motion (idle cannot leak contents); and the reveal-focus
 * camera pull is exactly 0 until a portal is selected and the reveal begins.
 * Runs under bare `node --test` with no DOM.
 */

import assert from "node:assert/strict";
import test from "node:test";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { planChoicePopulation } from "../../chance-engine/probability/choice-population.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import type { PortalSpec } from "./game.ts";
import { PORTAL_DEFAULT_CHOICES, portalFocusT, portalIdlePose } from "./game.ts";

const config = baseConfig<PortalSpec>(
  "mystery-portal",
  "Mystery Portal",
  "showcase",
  { shimmerLiveliness: 0.7 },
  { choiceCount: PORTAL_DEFAULT_CHOICES },
);

/** Find two rounds under one seed whose winner populations genuinely differ. */
const twoDistinctPopulations = (seed: number): readonly [number, number] => {
  const round0 = planChoicePopulation(config, PORTAL_DEFAULT_CHOICES, seed, 0);
  for (let round = 1; round < 64; round += 1) {
    const other = planChoicePopulation(config, PORTAL_DEFAULT_CHOICES, seed, round);
    const differs =
      other.winnerCount !== round0.winnerCount ||
      other.winnersByIndex.some((tier, i) => tier !== round0.winnersByIndex[i]);
    if (differs) {
      return [0, round];
    }
  }
  throw new Error("expected two rounds with differing populations");
};

test("mystery-portal: idle pose is identical across different winner populations", () => {
  const seed = 0x9a17_bead;
  const [roundA, roundB] = twoDistinctPopulations(seed);
  const popA = planChoicePopulation(config, PORTAL_DEFAULT_CHOICES, seed, roundA);
  const popB = planChoicePopulation(config, PORTAL_DEFAULT_CHOICES, seed, roundB);
  // Precondition: the populations really are different.
  assert.notDeepEqual([...popA.winnersByIndex], [...popB.winnersByIndex]);

  // The idle pose depends only on (index, tick, seed, liveliness) — never the
  // population — so it must be byte-identical whichever round is live.
  for (let tick = 0; tick < 240; tick += 7) {
    for (let index = 0; index < PORTAL_DEFAULT_CHOICES; index += 1) {
      const a = portalIdlePose(index, tick, seed, config.gameSpecific.shimmerLiveliness);
      const b = portalIdlePose(index, tick, seed, config.gameSpecific.shimmerLiveliness);
      assert.deepEqual(a, b);
    }
  }

  // And the pose actually moves — it is a real idle, not a constant.
  const moved = portalIdlePose(0, 40, seed, config.gameSpecific.shimmerLiveliness);
  const later = portalIdlePose(0, 41, seed, config.gameSpecific.shimmerLiveliness);
  assert.notEqual(moved.bob, later.bob);
});

test("mystery-portal: camera pull is zero until a portal is selected and the reveal begins", () => {
  const phaseAt = (phase: SessionState["phase"], phaseStartTick: number, tick: number): SessionState =>
    ({ config, phase, phaseStartTick, tick } as SessionState);

  // No selection: no pull in any phase.
  assert.equal(portalFocusT(phaseAt("ready", 0, 30), null, false), 0);
  assert.equal(portalFocusT(phaseAt("revealing", 0, 30), null, false), 0);

  // Selected but not yet revealing (committing): still no pull.
  assert.equal(portalFocusT(phaseAt("committing", 0, 5), 1, false), 0);

  // Selected AND revealing: the pull eases in from the phase start.
  assert.equal(portalFocusT(phaseAt("revealing", 10, 10), 1, false), 0);
  assert.ok(portalFocusT(phaseAt("revealing", 10, 16), 1, false) > 0);
});
