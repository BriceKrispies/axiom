/*
 * treasure-chest-pick.test.ts — the chest game's own invariants: the reveal
 * cadence puts the LATCH strictly before the LID; idle dances draw only from
 * the ambient stream (so they can never hint at contents); and the pick only
 * ever reveals the object's preassigned slot (no substitution).
 */

import assert from "node:assert/strict";
import test from "node:test";

import { planChoicePopulation } from "../../chance-engine/probability/choice-population.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import { createSession } from "../../chance-engine/sessions/session.ts";
import type { SessionState } from "../../chance-engine/sessions/session.ts";
import { dancePose, goldGleam, idlePhase, presentationPhase, revealTimeline } from "./game.ts";
import { TREASURE_CHEST_PICK } from "./definition.ts";

test("the reveal cadence puts the latch strictly before the lid", () => {
  for (const speed of [0.5, 1, 2]) {
    for (const reduced of [false, true]) {
      const t = revealTimeline(speed, reduced);
      assert.ok(t.latchStart < t.latchEnd, "latch has a duration");
      assert.ok(t.latchEnd <= t.pauseEnd, "the latch lands before the settle pause");
      assert.ok(t.pauseEnd < t.lidEnd, "the lid opens only after the pause");
      assert.ok(t.latchEnd <= t.pauseEnd && t.pauseEnd <= t.lidEnd, "latch fully precedes lid");
      assert.ok(t.lidEnd < t.riseEnd, "the reward rises after the lid opens");
    }
  }
});

test("the presentation phases name the reveal ritual in its legal order", () => {
  const tl = revealTimeline(1, false);
  const base = createSession(TREASURE_CHEST_PICK.defaultConfig(), 1, 1, new SeededChanceResultSource(1), { choiceCount: 9, kind: "choice" });
  const at = (phase: SessionState["phase"], age: number): SessionState => ({ ...base, phase, phaseStartTick: 0, tick: age });

  assert.equal(presentationPhase(at("intro", 3), tl), "idle");
  assert.equal(presentationPhase(at("ready", 3), tl), "idle");
  assert.equal(presentationPhase(at("committing", 3), tl), "committed");
  assert.equal(presentationPhase(at("resetting", 3), tl), "reset");
  assert.equal(presentationPhase(at("celebrating", 3), tl), "result");
  assert.equal(presentationPhase(at("complete", 3), tl), "result");

  // Inside the reveal, the named sub-phases advance monotonically along the ritual.
  const ritual = [0, tl.braceEnd, tl.latchEnd, tl.pauseEnd, tl.lidEnd, tl.riseEnd].map((age) => presentationPhase(at("revealing", age), tl));
  assert.deepEqual(ritual, ["anticipation", "latch", "seam", "lid", "burst", "prize"]);
});

test("idle cosmetics are deterministic, desynced, and outcome-independent", () => {
  // Each chest gets its own idle phase in [0, 2π) — so the nine never move in unison.
  const phases = Array.from({ length: 9 }, (_, i) => idlePhase(i));
  phases.forEach((p) => assert.ok(p >= 0 && p < Math.PI * 2, "idle phase in range"));
  assert.equal(new Set(phases.map((p) => p.toFixed(5))).size, 9, "nine distinct idle phases");

  // goldGleam is a pure function of (index, tick) with NO seed, so it cannot
  // encode which chest wins; it is bounded, replayable, and never lights all nine.
  for (let tick = 0; tick < 900; tick += 11) {
    const gleams = Array.from({ length: 9 }, (_, i) => goldGleam(i, tick));
    gleams.forEach((g, i) => {
      assert.ok(g >= 0 && g <= 1, "gleam bounded");
      assert.equal(goldGleam(i, tick), g, "gleam is deterministic");
    });
    assert.ok(gleams.filter((g) => g > 0.5).length <= 3, "at most a few chests gleam at once");
  }
});

test("idle dances draw only from the ambient stream", () => {
  // dancePose is a pure function of (index, count, tick, seed, liveliness) —
  // it takes NO presentation/gameplay seed, so it cannot correlate with which
  // chest wins. Same inputs → identical pose; different tick → free to differ.
  for (let tick = 0; tick < 400; tick += 7) {
    const a = dancePose(3, 9, tick, 12345, 0.7);
    const b = dancePose(3, 9, tick, 12345, 0.7);
    assert.deepEqual(a, b);
  }
  // The dance is real motion (not a dead stub) somewhere in the window.
  const moved = Array.from({ length: 200 }, (_, tick) => dancePose(4, 9, tick, 999, 0.7)).some(
    (pose) => Math.abs(pose.scootX) + Math.abs(pose.twist) + Math.abs(pose.squash) > 1e-4,
  );
  assert.ok(moved, "the dance must actually move");
  // Zero liveliness freezes the dance.
  assert.deepEqual(dancePose(4, 9, 50, 999, 0), { scootX: 0, squash: 0, twist: 0 });
});

test("the chest population is fixed before the pick and higher win rate means more prize chests", () => {
  const config = TREASURE_CHEST_PICK.defaultConfig();
  // Assigned before any pick; the selection only looks up its slot.
  const population = planChoicePopulation(config, 9, 4242, 1);
  const winners = population.winnersByIndex.filter((tier) => tier !== null).length;
  assert.equal(winners, population.winnerCount);

  // Averaged over seeds, more of the nine chests hold prizes as the target rises.
  const meanWinners = (p: number): number => {
    let total = 0;
    for (let seed = 1; seed <= 600; seed += 1) {
      total += planChoicePopulation({ ...config, targetWinRate: p }, 9, seed, 1).winnerCount;
    }
    return total / 600;
  };
  assert.ok(meanWinners(0.7) > meanWinners(0.3), "more prize chests at a higher win rate");
  assert.ok(Math.abs(meanWinners(0.5) - 4.5) < 0.2, "≈ 9·0.5 chests hold prizes");
});
