/*
 * heat-check.test.ts — the deterministic advantage-window scoring model, exercised with
 * Node's built-in runner (`node --test`, native TS type-stripping — no wasm/DOM/SDK). It
 * proves the core design: you can't wiggle-until-PERFECT then release — a make comes from
 * creating a real advantage and shooting before it closes. Timing matters but can't rescue
 * a smothered shot; waiting has a cost; wiggle-spam decays via fatigue; and it's all
 * deterministic and explainable.
 */

import assert from "node:assert/strict";
import test from "node:test";

import {
  applyScore,
  clampPlayerPosition,
  computeBalanceTag,
  computeBreakdown,
  computePressurePenalty,
  computeRequiredQuality,
  computeRhythmTag,
  computeSeparationScore,
  computeShotQuality,
  computeShotSelection,
  computeSpaceTag,
  computeStabilityScore,
  createShotArc,
  describeShot,
  determineShotResult,
  gainAdvantage,
  gainFatigue,
  pushFeedback,
  stepAdvantage,
  stepFatigue,
  updateDefenderBalance,
  updateHeat,
  updateStreakMultiplier,
} from "./gameplay.ts";
import type { BreakdownInput } from "./gameplay.ts";
import type { Feedback, Intent, ShotBreakdown } from "./types.ts";
import { HeatCheckSession } from "./session.ts";
import * as C from "./constants.ts";

const REQ = computeRequiredQuality(0, 0);

const q = (over: Partial<Parameters<typeof computeShotQuality>[0]>): number =>
  computeShotQuality({ advantage: 0, heat: 0, pressurePenalty: 0, separation: 0.5, shotSelection: 0.7, stability: 1, timing: 1, ...over });

const bd = (over: Partial<ShotBreakdown>): ShotBreakdown => ({
  advantage: 0,
  heatBonus: 0,
  pressurePenalty: 0.1,
  quality: 0.6,
  separation: 0.5,
  shotSelection: 0.7,
  stability: 1,
  timing: 1,
  ...over,
});

const bin = (over: Partial<BreakdownInput>): BreakdownInput => ({
  advantage: 0,
  defenderBalance: 1,
  defenderX: 0,
  fatigue: 0,
  finalWindow: false,
  heat: 0,
  plantTicks: 20,
  playerVelX: 0,
  playerX: 6,
  rhythmPhase: 0.5,
  ...over,
});

/** A contested, perfectly-timed look — the classic "waited for PERFECT" trap. */
const contestedPerfect = computeShotQuality({
  advantage: 0,
  heat: 0,
  pressurePenalty: 0.85,
  separation: 0.15,
  shotSelection: computeShotSelection(0, 0.85),
  stability: 1,
  timing: 1,
});

// ── deterministic outcome (no randomness) ────────────────────────────────────────

test("1. the same quality always produces the same result (no random roll)", () => {
  for (let i = 0; i < 100; i += 1) {
    assert.equal(determineShotResult(0.7, C.SHOT_REQUIRED_QUALITY, C.SHOT_SWISH_QUALITY), "make");
    assert.equal(determineShotResult(0.4, C.SHOT_REQUIRED_QUALITY, C.SHOT_SWISH_QUALITY), "miss");
    assert.equal(determineShotResult(0.9, C.SHOT_REQUIRED_QUALITY, C.SHOT_SWISH_QUALITY), "swish");
  }
  const rank = { make: 1, miss: 0, swish: 2 };
  let prev = -1;
  for (let x = 0; x <= 1.0001; x += 0.01) {
    const r = rank[determineShotResult(x, 0.55, 0.82)];
    assert.ok(r >= prev);
    prev = r;
  }
});

test("2. computeBreakdown is deterministic for identical inputs", () => {
  const input = bin({ advantage: 0.4, defenderX: 2, playerVelX: 0.1, rhythmPhase: 0.33 });
  assert.equal(JSON.stringify(computeBreakdown(input)), JSON.stringify(computeBreakdown(input)));
});

// ── shot creation matters more than timing ───────────────────────────────────────

test("3. perfect timing CANNOT rescue a fully contested shot", () => {
  assert.ok(contestedPerfect < REQ, "a smothered shot misses even at perfect timing");
});

test("4. an open, in-rhythm shot beats a contested perfect-timed one (creation > timing)", () => {
  const openInRhythm = computeShotQuality({
    advantage: 0,
    heat: 0,
    pressurePenalty: 0.08,
    separation: 0.85,
    shotSelection: computeShotSelection(0, 0.08),
    stability: 0.9,
    timing: 0.7,
  });
  assert.ok(openInRhythm >= REQ, "an open, decent-timing shot is viable");
  assert.ok(openInRhythm > contestedPerfect);
});

test("5. a quick shot right after beating the defender is viable even off perfect rhythm", () => {
  const quick = computeShotQuality({
    advantage: 0.7,
    heat: 0,
    pressurePenalty: 0.12,
    separation: 0.5,
    shotSelection: computeShotSelection(0.7, 0.12),
    stability: 0.65,
    timing: 0.5,
  });
  assert.ok(quick >= REQ, "beating your man opens a real (if imperfect) shot");
});

test("6. advantage + separation outweigh timing", () => {
  assert.ok(C.ADVANTAGE_WEIGHT + C.SEPARATION_WEIGHT > C.TIMING_WEIGHT);
  // Same look, worse timing but real advantage/space still beats no-space perfect timing.
  const created = q({ advantage: 0.7, separation: 0.8, timing: 0.4 });
  const timedOnly = q({ advantage: 0, separation: 0.15, timing: 1, shotSelection: 0.3 });
  assert.ok(created > timedOnly);
});

test("7. heat helps a little but never fixes bad shot selection", () => {
  assert.ok(q({ heat: C.HEAT_MAX }) - q({ heat: 0 }) <= C.HEAT_BONUS_WEIGHT + 1e-9);
  const badWithHeat = computeShotQuality({ advantage: 0, heat: C.HEAT_MAX, pressurePenalty: 0.85, separation: 0.15, shotSelection: 0.4, stability: 1, timing: 1 });
  assert.ok(badWithHeat < computeRequiredQuality(C.HEAT_MAX, 0));
});

// ── the advantage window (create, then shoot before it closes) ────────────────────

test("8. advantage decays over time, and faster as the defender recovers", () => {
  assert.ok(stepAdvantage(0.8, 0) < 0.8);
  assert.ok(stepAdvantage(0.8, 1) < stepAdvantage(0.8, 0));
});

test("9. beating the defender jumps advantage; waiting lets it (and the shot) decay", () => {
  assert.ok(gainAdvantage(0.1, 0, false) > 0.1 + 0.4);
  const inWindow = computeBreakdown(bin({ advantage: 0.6, defenderBalance: 0.2, defenderX: 2, playerVelX: 0.05, plantTicks: 6, playerX: 5, rhythmPhase: 0.35 }));
  const closed = computeBreakdown(bin({ advantage: 0.05, defenderBalance: 0.9, defenderX: 4.3, playerVelX: 0, plantTicks: 20, playerX: 5, rhythmPhase: 0.5 }));
  assert.ok(inWindow.quality >= REQ, "a shot inside the window scores");
  assert.ok(closed.quality < REQ, "the same spot after the window closes misses");
  assert.ok(inWindow.quality > closed.quality);
});

// ── anti-spam: dribble fatigue ────────────────────────────────────────────────────

test("10. repeated / fatigued crossovers give diminishing advantage", () => {
  assert.ok(gainAdvantage(0, 0, true) < gainAdvantage(0, 0, false)); // a repeat move
  assert.ok(gainAdvantage(0, 0.8, false) < gainAdvantage(0, 0, false)); // fatigued
});

test("11. quick repeat reversals build fatigue, which shakes the handle", () => {
  assert.ok(gainFatigue(0) > 0);
  assert.ok(stepFatigue(0.5) < 0.5); // bleeds off while committing
  assert.ok(computeStabilityScore(0, 0, 0.8) < computeStabilityScore(0, 0, 0));
});

// ── the three readiness tags (no single "guaranteed make" label) ─────────────────

test("12. SPACE tag reflects the advantage window + defender pressure", () => {
  assert.equal(computeSpaceTag(0.6, 0.1), "broken");
  assert.equal(computeSpaceTag(0, 0.1), "open");
  assert.equal(computeSpaceTag(0, 0.7), "smothered");
  assert.equal(computeSpaceTag(0, 0.45), "contested");
});

test("13. RHYTHM + BALANCE tags reflect timing and how set the body is", () => {
  assert.equal(computeRhythmTag(0.9, 0.5), "perfect");
  assert.equal(computeRhythmTag(0.6, 0.5), "good");
  assert.equal(computeRhythmTag(0.1, 0.3), "early");
  assert.equal(computeRhythmTag(0.1, 0.8), "late");
  assert.equal(computeBalanceTag(0.9, 10), "planted");
  assert.equal(computeBalanceTag(0.7, 0), "set");
  assert.equal(computeBalanceTag(0.3, 0), "moving");
});

// ── two-part feedback ────────────────────────────────────────────────────────────

test("14. feedback is two-part: SPACE / RHYTHM, and reads like the design", () => {
  assert.equal(describeShot("swish", "perfect", bd({ pressurePenalty: 0.05, timing: 1 }), 0.5, 0.3).text, "OPEN / PERFECT");
  assert.equal(describeShot("miss", "contested", bd({ pressurePenalty: 0.5, timing: 0.9 }), 0.5, 0).text, "CONTESTED / PERFECT TIMING");
  assert.equal(describeShot("miss", "offBalance", bd({ timing: 0.6 }), 0.5, 0).text, "OFF BALANCE / GOOD TIMING");
  assert.equal(describeShot("make", "clean", bd({ pressurePenalty: 0.1, timing: 0.6 }), 0.5, 0.7).text, "BROKEN ANKLES / CLEAN");
  assert.equal(describeShot("miss", "forced", bd({ timing: 0.2 }), 0.8, 0).text, "FORCED / LATE");
});

// ── the miss arc matches the reason, deterministically ───────────────────────────

test("15. the miss arc is deterministic and matches the reason", () => {
  assert.equal(JSON.stringify(createShotArc("early", 5, 0, 0.1)), JSON.stringify(createShotArc("early", 5, 0, 0.1)));
  assert.ok(createShotArc("early", 0, 0, 0).end.z < C.HOOP_Z);
  assert.ok(createShotArc("late", 0, 0, 0).end.z > C.HOOP_Z);
  assert.ok(createShotArc("offBalance", 0, 0, 1).end.x > 0);
  assert.ok(createShotArc("offBalance", 0, 0, -1).end.x < 0);
  assert.equal(createShotArc("perfect", 0, 0, 0).result, "swish");
});

// ── sub-scores + scoring + defender helpers ──────────────────────────────────────

test("16. separation, timing, stability, pressure behave", () => {
  assert.ok(computeSeparationScore(0, 4, 1) > computeSeparationScore(0, 1, 1));
  assert.ok(computeSeparationScore(0, 0.5, 0.2) > computeSeparationScore(0, 0.5, 1)); // beaten defender
  assert.ok(computePressurePenalty(0, 0.3, 1, false) > computePressurePenalty(0, 3, 1, false)); // close = more pressure
  assert.ok(q({ timing: 1 }) > q({ timing: 0.2 }));
  assert.ok(q({ stability: 0.2 }) < q({ stability: 1 }));
});

test("17. streak, multiplier, heat, scoring, clamp, balance", () => {
  assert.equal(updateStreakMultiplier(2, "make").multiplier, 2);
  assert.equal(updateStreakMultiplier(20, "make").multiplier, C.STREAK_MULTIPLIER_CAP);
  assert.deepEqual(updateStreakMultiplier(5, "miss"), { multiplier: 1, streak: 0 });
  assert.ok(updateHeat(0, "swish") > updateHeat(0, "make"));
  assert.equal(updateHeat(4, "miss", 0.1, 0.55), 0);
  assert.equal(applyScore({ deep: false, doublePoints: true, multiplier: 1, result: "make" }), C.MAKE_POINTS * 2);
  assert.equal(applyScore({ deep: true, doublePoints: false, multiplier: 4, result: "miss" }), 0);
  assert.equal(clampPlayerPosition(100), C.COURT_MAX_X);
  assert.equal(updateDefenderBalance(1, true), C.DEFENDER_BEATEN_BALANCE);
  assert.ok(updateDefenderBalance(0.2, false) > 0.2);
});

test("18. the feedback list stays bounded", () => {
  let list: readonly Feedback[] = [];
  for (let i = 0; i < C.FEEDBACK_MAX + 4; i += 1) {
    list = pushFeedback(list, { big: false, kind: "open", text: `#${i}` });
  }
  assert.equal(list.length, C.FEEDBACK_MAX);
});

// ── the session: create the window, and the anti-spam ────────────────────────────

const stick = (x: number): Intent => ({ holding: true, released: false, reset: false, shoot: false, stickX: x });
const release = (): Intent => ({ holding: false, released: true, reset: false, shoot: false, stickX: 0 });
const IDLE: Intent = { holding: false, released: false, reset: false, shoot: false, stickX: 0 };
const RESET: Intent = { holding: false, released: false, reset: true, shoot: false, stickX: 0 };

test("19. the round starts on first press; the meter is live while holding", () => {
  const s = new HeatCheckSession();
  assert.equal(s.readiness(), undefined);
  s.advance(stick(0.5));
  s.advance(stick(0.5));
  assert.equal(s.phase, "playing");
  assert.notEqual(s.readiness(), undefined);
});

test("20. a clean crossover opens an advantage window", () => {
  const s = new HeatCheckSession();
  for (let k = 0; k < 24; k += 1) {
    s.advance(stick(1)); // commit right so the defender chases right
  }
  const before = s.view().advantage;
  s.advance(stick(-1)); // hard cross back: the defender is going the wrong way
  assert.ok(s.view().advantage > before + 0.3, "beating the defender opens a window");
  assert.ok(s.view().windowActive);
});

test("21. creating a window then releasing in it scores", () => {
  const s = new HeatCheckSession();
  for (let k = 0; k < 30; k += 1) {
    s.advance(stick(1));
  }
  s.advance(stick(-1)); // crossover → advantage window opens
  s.advance(stick(-1));
  s.advance(release()); // shoot inside the window
  for (let k = 0; k < C.SHOT_ARC_DURATION + 4; k += 1) {
    s.advance(IDLE);
  }
  assert.ok(s.score > 0, "a shot inside the advantage window should score");
});

test("22. wiggle-spam builds fatigue and beats the defender far less than one committed cross", () => {
  const committed = new HeatCheckSession();
  for (let k = 0; k < 24; k += 1) {
    committed.advance(stick(1));
  }
  committed.advance(stick(-1));
  const committedAdv = committed.view().advantage;

  const spam = new HeatCheckSession();
  for (let k = 0; k < 26; k += 1) {
    spam.advance(stick(k % 2 === 0 ? 1 : -1)); // rapid left-right-left every tick
  }
  const spamAdv = spam.view().advantage;

  assert.ok(committedAdv > spamAdv, "one believable move beats endless wiggling");
});

test("23. a micro-tap does not fire a shot", () => {
  const s = new HeatCheckSession();
  s.advance(stick(0));
  for (let k = 0; k < C.MIN_SHOOT_HOLD_TICKS - 2; k += 1) {
    s.advance(stick(0));
  }
  s.advance(release());
  assert.equal(s.phase, "playing");
  assert.equal(s.score, 0);
});

test("24. the session is replay-deterministic for identical intent scripts", () => {
  const script: Intent[] = [
    stick(1), stick(1), stick(1), stick(-1), stick(-1),
    release(), IDLE, IDLE, stick(0.5), stick(-0.7),
  ];
  const run = (): number => {
    const s = new HeatCheckSession();
    for (const intent of script) {
      s.advance(intent);
    }
    for (let i = 0; i < 80; i += 1) {
      s.advance(IDLE);
    }
    return s.hash();
  };
  assert.equal(run(), run());
});

test("25. the round ends after 60s and preserves the session best", () => {
  const s = new HeatCheckSession();
  s.advance(stick(0.5));
  for (let i = 0; i < C.ROUND_TICKS + 4; i += 1) {
    s.advance(IDLE);
  }
  assert.equal(s.phase, "gameOver");
  const bestAfter = s.best;
  s.advance(stick(1));
  assert.equal(s.phase, "gameOver");
  s.advance(RESET);
  assert.equal(s.phase, "ready");
  assert.equal(s.best, bestAfter);
});
