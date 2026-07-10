/*
 * minimal-3v3.test.ts — node:test suite over the SDK-free core (gameplay.ts +
 * session.ts). No wasm, no DOM, no engine: tests script plain `Intent` streams
 * straight into `Mini3v3Session` and assert on the deterministic results.
 *
 *   node --test apps/axiom-minimal-3v3/web/src/minimal-3v3.test.ts
 */

import assert from "node:assert/strict";
import test from "node:test";

import { vec3 } from "./vec.ts";
import type { Intent } from "./types.ts";
import {
  classifyTiming,
  clampToBounds,
  computeShotChance,
  contestPenalty,
  defenderTarget,
  hashUnit,
  missEndpoint,
  passIntercepted,
  passTargets,
  rollShot,
  stealTouch,
  timingScore,
} from "./gameplay.ts";
import { Mini3v3Session } from "./session.ts";
import * as C from "./constants.ts";

const idle = (): Intent => ({
  gatherHeld: false,
  gatherPressed: false,
  gatherReleased: false,
  moveX: 0,
  moveZ: 0,
  passLeft: false,
  passRight: false,
  reset: false,
});

const withKeys = (overrides: Partial<Intent>): Intent => ({ ...idle(), ...overrides });

const run = (session: Mini3v3Session, intents: readonly Intent[]): void => {
  for (const intent of intents) {
    session.advance(intent);
  }
};

const repeat = (intent: Intent, n: number): Intent[] => Array.from({ length: n }, () => intent);

/** A varied deterministic 2000-tick script: move, pass, gather, hold, release. */
const scriptedStream = (): Intent[] => {
  const intents: Intent[] = [];
  for (let t = 0; t < 2000; t += 1) {
    const cycle = t % 400;
    if (cycle < 80) {
      intents.push(withKeys({ moveX: 0.7, moveZ: 1 }));
    } else if (cycle === 90) {
      intents.push(withKeys({ passLeft: true }));
    } else if (cycle < 150) {
      intents.push(idle());
    } else if (cycle === 150) {
      intents.push(withKeys({ gatherHeld: true, gatherPressed: true }));
    } else if (cycle < 150 + C.JUMP_APEX_TICK) {
      intents.push(withKeys({ gatherHeld: true }));
    } else if (cycle === 150 + C.JUMP_APEX_TICK) {
      intents.push(withKeys({ gatherReleased: true }));
    } else {
      intents.push(idle());
    }
  }
  return intents;
};

// ── determinism ───────────────────────────────────────────────────────────────

test("identical intent streams replay to identical hashes", () => {
  const a = new Mini3v3Session();
  const b = new Mini3v3Session();
  const stream = scriptedStream();
  for (let t = 0; t < stream.length; t += 1) {
    a.advance(stream[t]!);
    b.advance(stream[t]!);
    if (t % 100 === 0) {
      assert.equal(a.hash(), b.hash(), `hash diverged at tick ${t}`);
    }
  }
  assert.equal(a.hash(), b.hash());
});

test("hashUnit is deterministic, in [0,1), and varies with its fields", () => {
  const r = hashUnit(1, 0, 2, 30);
  assert.equal(r, hashUnit(1, 0, 2, 30));
  assert.ok(r >= 0 && r < 1);
  const rolls = new Set([hashUnit(1, 0, 2, 30), hashUnit(2, 0, 2, 30), hashUnit(1, 1, 2, 30), hashUnit(1, 0, 3, 30), hashUnit(1, 0, 2, 31)]);
  assert.ok(rolls.size >= 4, "field changes should change the roll");
});

// ── shot formula ──────────────────────────────────────────────────────────────

test("timingScore: 1 at the apex, 0 outside the window, monotone between", () => {
  assert.equal(timingScore(0), 1);
  assert.equal(timingScore(C.TIMING_WINDOW), 0);
  assert.equal(timingScore(C.TIMING_WINDOW + 5), 0);
  assert.ok(timingScore(2) > timingScore(4));
  assert.ok(timingScore(4) > timingScore(7));
});

test("classifyTiming buckets by signed error", () => {
  assert.equal(classifyTiming(0), "perfect");
  assert.equal(classifyTiming(-C.PERFECT_ERR), "perfect");
  assert.equal(classifyTiming(C.PERFECT_ERR), "perfect");
  assert.equal(classifyTiming(-4), "good");
  assert.equal(classifyTiming(4), "good");
  assert.equal(classifyTiming(-(C.GOOD_ERR + 1)), "early");
  assert.equal(classifyTiming(C.GOOD_ERR + 1), "late");
});

test("a perfect uncontested point-blank shot is strong but never guaranteed", () => {
  const { chance } = computeShotChance(C.JUMP_APEX_TICK, vec3(0, 0, C.HOOP_Z), []);
  assert.ok(Math.abs(chance - (C.SHOT_BASE + C.SHOT_TIMING_WEIGHT)) < 1e-9);
  assert.ok(chance < 1);
  assert.ok(chance <= C.CHANCE_MAX);
});

test("the worst shot clamps to the floor chance", () => {
  const farAwful = computeShotChance(C.JUMP_APEX_TICK + 12, vec3(0, 0, 1), [
    { jumping: true, pos: vec3(0, 0, 1.05) },
  ]);
  assert.equal(farAwful.chance, C.CHANCE_MIN);
});

test("contestPenalty: jumping-close > standing-close > far away", () => {
  const shooter = vec3(0, 0, 6);
  const close = vec3(0, 0, 6.5);
  const jumping = contestPenalty(shooter, [{ jumping: true, pos: close }]);
  const standing = contestPenalty(shooter, [{ jumping: false, pos: close }]);
  const far = contestPenalty(shooter, [{ jumping: true, pos: vec3(0, 0, 6 + C.CONTEST_RADIUS + 0.1) }]);
  assert.ok(jumping > standing);
  assert.ok(standing > 0);
  assert.equal(far, 0);
  assert.ok(jumping >= C.CONTEST_JUMPING_PENALTY_MIN && jumping <= C.CONTEST_JUMPING_PENALTY_MAX);
  assert.ok(standing >= C.CONTEST_STANDING_PENALTY_MIN && standing <= C.CONTEST_STANDING_PENALTY_MAX);
});

test("missEndpoint: early falls short, late clangs long, on-time rims out sideways", () => {
  const shooter = vec3(0, 0, 4);
  const early = missEndpoint(-5, shooter, 1, 0);
  assert.ok(early.z < C.HOOP_Z - C.RIM_RADIUS, "early miss lands short of the rim");
  const late = missEndpoint(5, shooter, 1, 0);
  assert.ok(late.z > C.HOOP_Z, "late miss reaches the backboard");
  assert.ok(late.y > C.HOOP_Y);
  const onTime = missEndpoint(0, shooter, 1, 0);
  assert.ok(Math.abs(Math.abs(onTime.x) - (C.RIM_RADIUS + 0.17)) < 1e-9, "on-time miss rims out to a side");
  assert.equal(onTime.z, C.HOOP_Z);
  assert.deepEqual(onTime, missEndpoint(0, shooter, 1, 0));
});

// ── predicates ────────────────────────────────────────────────────────────────

test("passIntercepted: near low ball yes, far or high ball no", () => {
  const defender = vec3(2, 0, 6);
  assert.ok(passIntercepted(vec3(2.2, 1.2, 6), [defender]));
  assert.ok(!passIntercepted(vec3(4, 1.2, 6), [defender]));
  assert.ok(!passIntercepted(vec3(2.2, C.INTERCEPT_MAX_BALL_Y + 0.2, 6), [defender]), "a high pass sails over");
});

test("stealTouch: only inside the steal radius", () => {
  const handler = vec3(0, 0, 4);
  assert.ok(stealTouch(handler, [vec3(0.3, 0, 4.2)]));
  assert.ok(!stealTouch(handler, [vec3(0, 0, 4 + C.STEAL_RADIUS + 0.1)]));
});

test("clampToBounds pins positions to the half court", () => {
  const p = clampToBounds(vec3(50, 0, -50));
  assert.equal(p.x, C.BOUND_X);
  assert.equal(p.z, C.BOUND_Z_MIN);
});

// ── session: movement + bounds ────────────────────────────────────────────────

test("held movement clamps at the court bounds", () => {
  const s = new Mini3v3Session();
  run(s, repeat(withKeys({ moveZ: 1 }), 600));
  assert.ok(s.view().blues[0].pos.z <= C.BOUND_Z_MAX + 1e-9);
  const s2 = new Mini3v3Session();
  run(s2, repeat(withKeys({ moveX: 1 }), 600));
  assert.ok(s2.view().blues[0].pos.x <= C.BOUND_X + 1e-9);
  assert.ok(Math.abs(s2.view().blues[0].pos.x - C.BOUND_X) < 0.01, "reaches the sideline");
});

// ── session: passing ──────────────────────────────────────────────────────────

test("Q passes to the higher-x (screen-left) teammate and transfers control", () => {
  const positions = [vec3(0, 0, 4), vec3(4.2, 0, 7), vec3(-4.2, 0, 7)];
  assert.deepEqual(passTargets(positions, 0), { left: 1, right: 2 });

  const s = new Mini3v3Session();
  s.advance(withKeys({ passLeft: true }));
  assert.equal(s.possessionLabel, "PASS IN FLIGHT");
  run(s, repeat(idle(), C.PASS_TICKS + 2));
  assert.equal(s.phase, "playing");
  assert.equal(s.controlledIndex, 1, "control moved to the left wing");
  assert.equal(s.possessionLabel, "YOU HAVE THE BALL");
});

test("E passes to the lower-x (screen-right) teammate", () => {
  const s = new Mini3v3Session();
  s.advance(withKeys({ passRight: true }));
  run(s, repeat(idle(), C.PASS_TICKS + 2));
  assert.equal(s.controlledIndex, 2);
});

// ── session: shooting ─────────────────────────────────────────────────────────

test("holding Space past the apex auto-releases with LATE timing", () => {
  const s = new Mini3v3Session();
  s.advance(withKeys({ gatherHeld: true, gatherPressed: true }));
  let sawShooting = false;
  for (let t = 0; t < 300 && s.phase !== "shotResult"; t += 1) {
    sawShooting = sawShooting || s.phase === "shooting";
    s.advance(withKeys({ gatherHeld: true }));
  }
  assert.ok(sawShooting);
  assert.equal(s.phase, "shotResult", "auto-release fired without a release edge");
  assert.equal(s.timingTag, "late");
  assert.equal(s.attempts, 1);
});

test("an apex release resolves at release, matching the pure formula", () => {
  const s = new Mini3v3Session();
  s.advance(withKeys({ gatherHeld: true, gatherPressed: true }));
  run(s, repeat(withKeys({ gatherHeld: true }), C.JUMP_APEX_TICK - 1));
  s.advance(withKeys({ gatherReleased: true }));
  assert.equal(s.timingTag, "perfect");
  assert.equal(s.attempts, 1);

  // Recompute the expected outcome independently: the shooter never moved from the
  // reset slot and the defenders start ON their AI targets, so they are stationary.
  const shooter = C.RESET_HANDLER;
  const threats = [
    { jumping: false, pos: defenderTarget(true, C.RESET_HANDLER, C.RESET_HANDLER) },
    { jumping: false, pos: defenderTarget(false, C.RESET_WING_LEFT, C.RESET_HANDLER) },
    { jumping: false, pos: defenderTarget(false, C.RESET_WING_RIGHT, C.RESET_HANDLER) },
  ];
  const { chance, signedErr, distance } = computeShotChance(C.JUMP_APEX_TICK, shooter, threats);
  const expectedMade = rollShot(chance, 1, 0, signedErr, distance);

  run(s, repeat(idle(), 200));
  assert.ok(s.attempts === 1);
  assert.equal(s.makes, expectedMade ? 1 : 0, "session outcome matches the pure-formula roll");
});

test("both makes and misses occur across varied distances and release timings", () => {
  // Deep contested shots should tend to miss; closer, apex-timed shots should be
  // able to make. Sweep distance (walk-in ticks) × hold length and require both
  // outcomes to appear — proving timing AND distance matter, and neither outcome
  // is guaranteed.
  const outcomes = new Set<boolean>();
  for (const walk of [0, 40, 60]) {
    for (let hold = C.JUMP_APEX_TICK - 6; hold <= C.JUMP_APEX_TICK + 6; hold += 2) {
      const s = new Mini3v3Session();
      run(s, repeat(withKeys({ moveZ: 1 }), walk));
      run(s, repeat(idle(), 30));
      s.advance(withKeys({ gatherHeld: true, gatherPressed: true }));
      run(s, repeat(withKeys({ gatherHeld: true }), hold - 1));
      s.advance(withKeys({ gatherReleased: true }));
      run(s, repeat(idle(), 150));
      if (s.phase === "playing" && s.attempts === 1) {
        outcomes.add(s.makes === 1);
      }
    }
  }
  assert.equal(outcomes.size, 2, "shots can both make and miss");
});

// ── session: result + reset ───────────────────────────────────────────────────

test("after a shot result the possession resets with the player holding the ball", () => {
  const s = new Mini3v3Session();
  s.advance(withKeys({ gatherHeld: true, gatherPressed: true }));
  run(s, repeat(withKeys({ gatherHeld: true }), 200));
  run(s, repeat(idle(), C.RESULT_TICKS + 5));
  const v = s.view();
  assert.equal(s.phase, "playing");
  assert.equal(s.controlledIndex, 0);
  assert.deepEqual(v.blues[0].pos, C.RESET_HANDLER);
  assert.deepEqual(v.blues[1].pos, C.RESET_WING_LEFT);
  assert.deepEqual(v.blues[2].pos, C.RESET_WING_RIGHT);
  assert.ok(Math.hypot(v.ball.x - v.blues[0].pos.x, v.ball.z - v.blues[0].pos.z) < 1, "ball back with the handler");
});

test("R resets mid-play", () => {
  const s = new Mini3v3Session();
  run(s, repeat(withKeys({ moveX: 1, moveZ: 1 }), 60));
  assert.ok(s.view().blues[0].pos.z > C.RESET_HANDLER.z);
  s.advance(withKeys({ reset: true }));
  assert.deepEqual(s.view().blues[0].pos, C.RESET_HANDLER);
  assert.equal(s.phase, "playing");
});

// ── session: defenders ────────────────────────────────────────────────────────

test("defenders stay in bounds, finite, and eventually contest-jump", () => {
  const s = new Mini3v3Session();
  let sawJump = false;
  for (let t = 0; t < 600; t += 1) {
    s.advance(idle());
    const v = s.view();
    for (const d of v.defenders) {
      assert.ok(Number.isFinite(d.pos.x) && Number.isFinite(d.pos.z));
      assert.ok(Math.abs(d.pos.x) <= C.BOUND_X + 1e-9);
      assert.ok(d.pos.z >= C.BOUND_Z_MIN - 1e-9 && d.pos.z <= C.BOUND_Z_MAX + 1e-9);
      sawJump = sawJump || d.jumpY > 0;
    }
  }
  assert.ok(sawJump, "a contest jump fired within 600 idle ticks near the handler");
});

test("a pass through a defender is intercepted and play resets", () => {
  // Drive the handler far right so the return pass to the LEFT wing must cross the
  // court; defenders re-shade toward the handler, putting bodies near the lane.
  const s = new Mini3v3Session();
  let intercepted = false;
  // Try several pass moments; deterministically at least one crossing pass should
  // meet a defender given the primary shades the handler at close range.
  for (let attempt = 0; attempt < 8 && !intercepted; attempt += 1) {
    run(s, repeat(withKeys({ moveX: -1 }), 30));
    run(s, repeat(withKeys({ moveZ: 1 }), 30 + attempt * 10));
    s.advance(withKeys({ passLeft: true }));
    for (let t = 0; t < C.PASS_TICKS + 2; t += 1) {
      s.advance(idle());
      if (s.phase === "turnoverResult") {
        intercepted = true;
        assert.equal(s.resultKind, "intercepted");
        break;
      }
    }
    run(s, repeat(idle(), C.RESULT_TICKS + 5));
  }
  assert.ok(intercepted, "at least one crossing pass is intercepted");
});
