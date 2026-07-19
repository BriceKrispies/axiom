/*
 * home-run.test.ts — the deterministic batting model, exercised with Node's
 * built-in runner (`node --test`, native TS type-stripping — no wasm/DOM/SDK).
 * It proves the core design: the swing is a real spring-loaded state machine
 * that fires on RELEASE only; contact is resolved from the actual spatial
 * relationship of bat and ball (position along the bat, timing angle, vertical
 * offset) — never a timing-window roll; the pitch sequence, fielder wander, and
 * every outcome replay bit-for-bit from the seed.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { hash01, vec3 } from "./vec.ts";
import type { Intent, Outcome } from "./types.ts";
import { newSwing, resolveContact, stepSwing } from "./swing.ts";
import { isStrike, pitchPool, selectPitch, solvePitch } from "./pitch.ts";
import { catchingFielder, projectLanding, stepFielders } from "./fielders.ts";
import { beyondWall, classifyFlight, isFair, newFlight, scoreFor, stepFlight } from "./ball.ts";
import { HomeRunSession } from "./session.ts";
import * as C from "./constants.ts";

const IDLE: Intent = { moveX: 0, start: false, swing: false };
const intent = (over: Partial<Intent>): Intent => ({ ...IDLE, ...over });

const adv = (s: HomeRunSession, n: number, i: Intent = IDLE): void => {
  for (let k = 0; k < n; k += 1) {
    s.advance(i);
  }
};

/** Run a whole round taking every pitch (no swing); returns the session. */
const takeAllRound = (seed: number): HomeRunSession => {
  const s = new HomeRunSession(seed);
  s.advance(intent({ start: true }));
  let guard = 40_000;
  while (s.phase !== "over" && guard > 0) {
    s.advance(IDLE);
    guard -= 1;
  }
  assert.ok(guard > 0, "take-all round must complete");
  return s;
};

/**
 * Play one pitch: start the round on tick 1, optionally step the batter, then
 * press swing at `swingTick`. Returns the first pitch's outcome.
 */
const playFirstPitch = (seed: number, swingTick: number, moveX = 0, moveTicks = 0): Outcome => {
  const s = new HomeRunSession(seed);
  for (let t = 1; t < swingTick; t += 1) {
    const moving = t <= moveTicks ? moveX : 0;
    s.advance(intent({ moveX: moving, start: t === 1 }));
  }
  s.advance(intent({ swing: true }));
  let guard = 1200;
  while (s.results.length === 0 && guard > 0) {
    s.advance(IDLE);
    guard -= 1;
  }
  assert.ok(guard > 0, "pitch must resolve");
  return s.results[0]!.outcome;
};

// ── swing state machine ───────────────────────────────────────────────────────

test("the batter starts wound and ready at full power", () => {
  const s = newSwing();
  assert.equal(s.state, "ready");
  assert.equal(s.readiness, 1);
  assert.equal(s.theta, C.THETA_READY);
});

test("pressing swing fires the full-power strike instantly", () => {
  let s = newSwing();
  s = stepSwing(s, true);
  assert.equal(s.state, "swing");
  assert.equal(s.omega, C.OMEGA_SWING, "every swing is max power");
  assert.equal(s.theta, C.THETA_READY, "the strike starts from the wound stance");
});

test("presses during the strike and the rewind cooldown do nothing", () => {
  let s = newSwing();
  s = stepSwing(s, true);
  const firstSwingTick = 0;
  let sawRewind = false;
  let reentered = false;
  let guard = 500;
  // Spam the button every tick: the machine must run its full cycle untouched —
  // a spammed press must never restart the strike mid-cycle.
  while (guard > 0 && s.state !== "ready") {
    const before = s.state;
    s = stepSwing(s, true);
    sawRewind = sawRewind || s.state === "rewind";
    reentered = reentered || (before !== "ready" && s.state === "swing" && s.stateTicks === firstSwingTick && before !== "swing");
    guard -= 1;
  }
  assert.ok(guard > 0, "the cycle returns to ready");
  assert.ok(sawRewind, "the cooldown rewind ran");
  assert.ok(!reentered, "spam never restarted the strike mid-cycle");
  // Once ready, the very next press swings again.
  s = stepSwing(s, true);
  assert.equal(s.state, "swing");
});

test("full cycle: ready → swing → follow → rewind → ready, rewind slower than the strike", () => {
  let s = newSwing();
  const seen: string[] = [s.state];
  const record = (): void => {
    if (seen[seen.length - 1] !== s.state) {
      seen.push(s.state);
    }
  };
  s = stepSwing(s, true);
  record();
  let strikeTicks = 0;
  while (s.state === "swing") {
    s = stepSwing(s, false);
    strikeTicks += 1;
    record();
  }
  let followTicks = 0;
  while (s.state === "follow") {
    s = stepSwing(s, false);
    followTicks += 1;
    record();
  }
  let rewindTicks = 0;
  while (s.state === "rewind") {
    s = stepSwing(s, false);
    rewindTicks += 1;
    record();
    assert.ok(rewindTicks < 500, "the rewind terminates");
  }
  record();
  assert.deepEqual(seen, ["ready", "swing", "follow", "rewind", "ready"]);
  assert.ok(rewindTicks > strikeTicks, "the self-rewind is slower than the strike");
  assert.ok(followTicks > 0, "the bat overshoots into follow-through");
  assert.equal(s.theta, C.THETA_READY);
});

test("readiness is bounded and climbs monotonically through the rewind", () => {
  let s = newSwing();
  s = stepSwing(s, true);
  let prev = -1;
  let guard = 500;
  while (s.state !== "ready" && guard > 0) {
    s = stepSwing(s, false);
    assert.ok(s.readiness >= 0 && s.readiness <= 1, "readiness is bounded");
    if (s.state === "rewind") {
      assert.ok(s.readiness >= prev, "rewind readiness never regresses");
      prev = s.readiness;
    }
    guard -= 1;
  }
  assert.ok(guard > 0);
  assert.equal(s.readiness, 1, "ready means fully re-wound");
});

test("identical inputs produce identical bat poses (pure state machine)", () => {
  const script = (tick: number): boolean => tick % 90 === 5;
  let a = newSwing();
  let b = newSwing();
  for (let t = 0; t < 800; t += 1) {
    a = stepSwing(a, script(t));
    b = stepSwing(b, script(t));
    assert.equal(a.theta, b.theta);
    assert.equal(a.state, b.state);
    assert.equal(a.readiness, b.readiness);
  }
});

// ── contact model ────────────────────────────────────────────────────────────

const contactAt = (theta: number, r: number, dy: number) =>
  resolveContact(theta, C.OMEGA_SWING, r, dy, vec3(0, C.BAT_PLANE_Y + dy, 0), -0.3);

test("centered contact launches much harder than handle contact", () => {
  const sweet = contactAt(C.THETA_SWEET, C.SWEET_SPOT_R, 0);
  const handle = contactAt(C.THETA_SWEET, 0.25, 0);
  assert.ok(sweet.exitSpeed > handle.exitSpeed * 1.6, `sweet ${sweet.exitSpeed} vs handle ${handle.exitSpeed}`);
  assert.ok(sweet.quality > handle.quality);
});

test("contact toward the end of the bat beats contact near the hands", () => {
  const tip = contactAt(C.THETA_SWEET, 0.95, 0);
  const jam = contactAt(C.THETA_SWEET, 0.3, 0);
  assert.ok(tip.exitSpeed > jam.exitSpeed);
});

test("early and late contact spray to opposite fields; extremes are foul", () => {
  const early = contactAt(C.THETA_SWEET + 0.35, C.SWEET_SPOT_R, 0); // bat already past square
  const late = contactAt(C.THETA_SWEET - 0.35, C.SWEET_SPOT_R, 0);
  assert.ok(early.spray > 0.1, "early pulls to +X");
  assert.ok(late.spray < -0.1, "late pushes to -X");
  const veryLate = contactAt(C.THETA_SWEET - 0.95, C.SWEET_SPOT_R, 0);
  assert.ok(Math.abs(veryLate.spray) > C.FOUL_ANGLE, "extreme timing is foul territory");
});

test("vertical offset shapes the launch: undercut lifts, topping drives down", () => {
  const under = contactAt(C.THETA_SWEET, C.SWEET_SPOT_R, 0.15);
  const square = contactAt(C.THETA_SWEET, C.SWEET_SPOT_R, 0);
  const topped = contactAt(C.THETA_SWEET, C.SWEET_SPOT_R, -0.18);
  assert.ok(under.loft > square.loft);
  assert.ok(topped.loft < C.GROUNDER_LOFT, "topped ball is a grounder arc");
  assert.ok(square.exitSpeed > topped.exitSpeed, "mishit bleeds exit speed");
});

// ── pitch sequence ───────────────────────────────────────────────────────────

test("the pitch sequence reproduces exactly from the same seed", () => {
  for (let i = 0; i < C.PITCHES_PER_ROUND; i += 1) {
    assert.deepEqual(selectPitch(42, i), selectPitch(42, i));
  }
});

test("different seeds produce different rounds", () => {
  const a = Array.from({ length: 10 }, (_, i) => selectPitch(1, i).mph).join(",");
  const b = Array.from({ length: 10 }, (_, i) => selectPitch(2, i).mph).join(",");
  assert.notEqual(a, b);
});

test("pitch profiles have genuinely different speeds", () => {
  const speeds = new Set(C.PITCH_PROFILES.map((p) => p.speed));
  assert.ok(speeds.size >= 5, "at least five distinct profile speeds");
  const mphs = new Set(Array.from({ length: 10 }, (_, i) => selectPitch(9, i).mph));
  assert.ok(mphs.size >= 3, "one round mixes clearly different speeds");
});

test("early pitches are easy; hard pitches only appear late", () => {
  const tierOf = new Map(C.PITCH_PROFILES.map((p) => [p.id, p.tier]));
  for (let seed = 1; seed <= 30; seed += 1) {
    for (let i = 0; i < C.PITCHES_PER_ROUND; i += 1) {
      const tier = tierOf.get(selectPitch(seed, i).profileId);
      if (i < C.EASY_ONLY_BEFORE) {
        assert.equal(tier, "easy", `pitch ${i} of seed ${seed}`);
      } else if (i < C.HARD_ALLOWED_FROM) {
        assert.notEqual(tier, "hard", `pitch ${i} of seed ${seed}`);
      }
    }
  }
  // The late pool really does include hard profiles.
  assert.ok(pitchPool(9).some((p) => p.tier === "hard"));
});

test("a solved pitch arrives at its aim point over the plate", () => {
  for (let seed = 1; seed <= 8; seed += 1) {
    const spec = selectPitch(seed, 3);
    const { vel, gravityPerTick } = solvePitch(spec);
    let pos = C.PITCH_RELEASE;
    let prev = pos;
    let v = vel;
    let guard = 400;
    while (pos.z > 0 && guard > 0) {
      prev = pos;
      v = vec3(v.x, v.y - gravityPerTick, v.z);
      pos = vec3(pos.x + v.x, pos.y + v.y, pos.z + v.z);
      guard -= 1;
    }
    // Interpolate the exact z=0 plate crossing between the last two integrator steps.
    const f = prev.z / (prev.z - pos.z);
    const xAt = prev.x + (pos.x - prev.x) * f;
    const yAt = prev.y + (pos.y - prev.y) * f;
    assert.ok(Math.abs(xAt - spec.targetX) < 0.02, "lateral aim");
    assert.ok(Math.abs(yAt - spec.targetY) < 0.02, "height aim");
  }
});

// ── fielders ─────────────────────────────────────────────────────────────────

const mkFielders = () => C.FIELDER_SPOTS.map((s) => ({ chasing: false, facing: 0, speed: 0, traveled: 0, x: s.x, z: s.z }));

test("fielders hold their spot when there is no ball to chase", () => {
  const fielders = mkFielders();
  for (let t = 0; t < 300; t += 1) {
    stepFielders(fielders, null);
  }
  for (let i = 0; i < fielders.length; i += 1) {
    const spot = C.FIELDER_SPOTS[i]!;
    assert.ok(Math.hypot(fielders[i]!.x - spot.x, fielders[i]!.z - spot.z) < 1e-6, `${spot.name} stays put`);
    assert.equal(fielders[i]!.chasing, false);
    assert.equal(fielders[i]!.speed, 0, "a held fielder is not moving");
  }
});

test("a reachable landing point pulls nearby fielders into a clamped chase, then they return", () => {
  const fielders = mkFielders();
  const cf = C.FIELDER_SPOTS.findIndex((s) => s.name === "CF");
  const landing = { x: C.FIELDER_SPOTS[cf]!.x + 1.5, z: C.FIELDER_SPOTS[cf]!.z + 1.5 };
  for (let t = 0; t < 200; t += 1) {
    stepFielders(fielders, landing);
  }
  const f = fielders[cf]!;
  assert.ok(f.chasing, "CF reacts");
  assert.ok(Math.hypot(f.x - landing.x, f.z - landing.z) < 0.2, "CF converges on the landing point");
  const spot = C.FIELDER_SPOTS[cf]!;
  assert.ok(Math.hypot(f.x - spot.x, f.z - spot.z) <= spot.radius * C.FIELDER_CHASE_CLAMP + 1e-9, "never leaves the clamp");
  // A fielder across the field ignores it and holds its spot.
  const rf = C.FIELDER_SPOTS.findIndex((s) => s.name === "1B");
  assert.equal(fielders[rf]!.chasing, false);
  assert.ok(Math.hypot(fielders[rf]!.x - C.FIELDER_SPOTS[rf]!.x, fielders[rf]!.z - C.FIELDER_SPOTS[rf]!.z) < 1e-6);
  // Ball gone: CF walks back to its spot.
  for (let t = 0; t < 400; t += 1) {
    stepFielders(fielders, null);
  }
  assert.ok(Math.hypot(f.x - spot.x, f.z - spot.z) < 1e-6, "CF returns to its spot");
});

test("catchingFielder requires closeness AND a catchable height", () => {
  const fielders = C.FIELDER_SPOTS.map((s) => ({ chasing: false, facing: 0, speed: 0, traveled: 0, x: s.x, z: s.z }));
  const cf = C.FIELDER_SPOTS.findIndex((s) => s.name === "CF")!;
  const spot = C.FIELDER_SPOTS[cf]!;
  assert.equal(catchingFielder(fielders, vec3(spot.x, 0.5, spot.z)), cf);
  assert.equal(catchingFielder(fielders, vec3(spot.x, C.CATCH_HEIGHT + 1, spot.z)), -1);
  assert.equal(catchingFielder(fielders, vec3(spot.x + 5, 0.5, spot.z)), -1);
});

test("projectLanding lands where the integrator lands", () => {
  const g = C.GRAVITY / (C.FIXED_HZ * C.FIXED_HZ);
  const pos0 = vec3(0.2, 1.1, 0.1);
  const vel0 = vec3(0.05, 0.25, 0.3);
  const proj = projectLanding(pos0, vel0, g);
  let pos = pos0;
  let vel = vel0;
  while (pos.y > C.BALL_RADIUS || vel.y > 0) {
    vel = vec3(vel.x, vel.y - g, vel.z);
    pos = vec3(pos.x + vel.x, pos.y + vel.y, pos.z + vel.z);
  }
  assert.ok(Math.hypot(proj.x - pos.x, proj.z - pos.z) < 0.5);
});

// ── boundaries + outcomes ────────────────────────────────────────────────────

test("fair/foul wedge and the wall line", () => {
  assert.ok(isFair(0, 10));
  assert.ok(isFair(9, 10));
  assert.ok(!isFair(11, 10));
  assert.ok(!isFair(0, -1));
  assert.ok(beyondWall(0, C.WALL_LINE));
  assert.ok(beyondWall(10, C.WALL_LINE - 10));
  assert.ok(!beyondWall(0, C.WALL_LINE - 1));
});

test("a high drive over the wall classifies as a home run", () => {
  const speed = 38 / C.FIXED_HZ;
  const loft = 0.5;
  const b = newFlight(vec3(0, 1, 0), vec3(0, Math.sin(loft) * speed, Math.cos(loft) * speed), 38, loft, 0);
  let guard = 2000;
  while (!stepFlight(b) && guard > 0) {
    guard -= 1;
  }
  assert.ok(b.homer, "cleared the wall");
  assert.equal(classifyFlight(b), "homer");
});

test("a hooked ball into foul territory classifies foul", () => {
  const spray = C.FOUL_ANGLE + 0.2;
  const speed = 25 / C.FIXED_HZ;
  const b = newFlight(vec3(0, 1, 0), vec3(Math.sin(spray) * speed, 0.1, Math.cos(spray) * speed), 25, 0.3, spray);
  assert.ok(b.foul, "spray past the foul line is foul off the bat");
  let guard = 2000;
  while (!stepFlight(b) && guard > 0) {
    guard -= 1;
  }
  assert.equal(classifyFlight(b), "foul");
});

test("weak exit speed, low loft, and short high flies classify weak/grounder/popup", () => {
  const mk = (exitSpeed: number, loft: number): ReturnType<typeof newFlight> => {
    const s = exitSpeed / C.FIXED_HZ;
    const b = newFlight(vec3(0, 1, 0.2), vec3(0, Math.sin(loft) * s, Math.cos(loft) * s), exitSpeed, loft, 0);
    let guard = 2000;
    while (!stepFlight(b) && guard > 0) {
      guard -= 1;
    }
    return b;
  };
  assert.equal(classifyFlight(mk(10, 0.4)), "weak");
  assert.equal(classifyFlight(mk(24, 0.05)), "grounder");
  assert.equal(classifyFlight(mk(20, 1.1)), "popup");
  assert.equal(classifyFlight(mk(30, 0.42)), "clean");
});

test("scoring table + distance bonuses + the homer streak multiplier", () => {
  assert.equal(scoreFor("miss", 0, 0), 0);
  assert.equal(scoreFor("foul", 12, 0), 0);
  assert.equal(scoreFor("weak", 6, 0), 25);
  assert.equal(scoreFor("grounder", 9, 0), 50);
  assert.equal(scoreFor("popup", 9, 0), 50);
  assert.equal(scoreFor("clean", 24, 0), 100 + 24);
  assert.equal(scoreFor("homer", 40, 1), 500 + 80);
  assert.equal(scoreFor("homer", 40, 2), (500 + 80) * 2);
  assert.equal(scoreFor("homer", 40, 9), (500 + 80) * C.STREAK_MULT_CAP);
});

// ── session round loop ───────────────────────────────────────────────────────

test("batter movement is clamped to the batting box", () => {
  const s = new HomeRunSession(1);
  s.advance(intent({ start: true }));
  adv(s, 500, intent({ moveX: 1 }));
  assert.equal(s.batterX, C.BATTER_MAX_X);
  adv(s, 500, intent({ moveX: -1 }));
  assert.equal(s.batterX, C.BATTER_MIN_X);
});

test("taking every pitch completes a 10-pitch round of strikes and balls, scoring zero", () => {
  const s = takeAllRound(11);
  assert.equal(s.phase, "over");
  assert.equal(s.results.length, C.PITCHES_PER_ROUND);
  assert.ok(s.results.every((r) => r.outcome === "miss" || r.outcome === "ball"));
  assert.equal(s.score, 0);
  assert.equal(s.pitchNumber, C.PITCHES_PER_ROUND);
});

test("the strike zone judges plate crossings", () => {
  assert.ok(isStrike(0, 0.9), "over the middle");
  assert.ok(isStrike(C.STRIKE_ZONE_HALF_X, C.STRIKE_ZONE_LOW), "the zone edge is a strike");
  assert.ok(!isStrike(C.STRIKE_ZONE_HALF_X + 0.1, 0.9), "off the outside edge");
  assert.ok(!isStrike(-C.STRIKE_ZONE_HALF_X - 0.1, 0.9), "off the inside edge");
  assert.ok(!isStrike(0, C.STRIKE_ZONE_HIGH + 0.1), "above the zone");
  assert.ok(!isStrike(0, C.STRIKE_ZONE_LOW - 0.1), "in the dirt");
});

test("a taken pitch off the plate is a BALL (0 points); in the zone it is a strike", () => {
  const outcomes: Outcome[] = [];
  for (let seed = 1; seed <= 12; seed += 1) {
    const s = takeAllRound(seed);
    for (const r of s.results) {
      assert.ok(r.outcome === "miss" || r.outcome === "ball", "a take never gets a swing outcome");
      assert.equal(r.points, 0, "neither call scores");
    }
    outcomes.push(...s.results.map((r) => r.outcome));
  }
  assert.ok(outcomes.includes("ball"), "some off-the-plate takes get called balls");
  assert.ok(outcomes.includes("miss"), "in-zone takes are still strikes");
  // The umpire is deterministic: the same seed reproduces the same calls.
  assert.deepEqual(
    takeAllRound(3).results.map((r) => r.outcome),
    takeAllRound(3).results.map((r) => r.outcome),
  );
});

test("a round's pitch speeds vary and reproduce from the seed", () => {
  const a = takeAllRound(21).results.map((r) => r.mph);
  const b = takeAllRound(21).results.map((r) => r.mph);
  assert.deepEqual(a, b);
  assert.ok(new Set(a).size >= 3, `speeds vary: ${a.join(",")}`);
});

test("swinging and whiffing detects a miss (not a take)", () => {
  // Release almost immediately: the bat sweeps long before the ball arrives.
  const outcome = playFirstPitch(1, 5);
  assert.equal(outcome, "miss");
});

test("some release timing on the first pitch produces a home run (and it replays)", () => {
  let found: { readonly t: number; readonly move: number; readonly mt: number } | null = null;
  outer: for (const [move, mt] of [
    [0, 0],
    [-1, 12],
    [1, 12],
  ] as const) {
    for (let t = 30; t <= 150; t += 1) {
      if (playFirstPitch(1, t, move, mt) === "homer") {
        found = { move, mt, t };
        break outer;
      }
    }
  }
  assert.ok(found !== null, "a full-load swing at the right moment must clear the wall");
  // …and the exact same input reproduces the exact same homer.
  assert.equal(playFirstPitch(1, found.t, found.move, found.mt), "homer");
});

test("different swing timings on the same pitch produce different outcomes", () => {
  const outcomes = new Set<Outcome>();
  for (let t = 95; t <= 140; t += 1) {
    outcomes.add(playFirstPitch(3, t));
  }
  assert.ok(outcomes.size >= 3, `timing matters: ${[...outcomes].join(",")}`);
});

test("restart from the finished state resets all gameplay state", () => {
  const s = takeAllRound(4);
  assert.equal(s.phase, "over");
  s.advance(intent({ start: true }));
  assert.equal(s.phase, "ready");
  assert.equal(s.score, 0);
  assert.equal(s.results.length, 0);
  assert.equal(s.homers, 0);
  assert.equal(s.streak, 0);
  assert.equal(s.bestDistance, 0);
  assert.equal(s.batterX, C.BATTER_START_X);
  assert.equal(s.swing.state, "ready");
  assert.equal(s.pitchNumber, 1);
});

test("same seed + same input history reproduce the same final score and results", () => {
  const script = (tick: number): Intent => {
    // A pseudo-random but fully deterministic press pattern: one swing press
    // somewhere inside each ~260-tick window, plus periodic batter steps.
    const window = Math.floor(tick / 260);
    const pressAt = 30 + Math.floor(hash01(99, window) * 180);
    return intent({
      moveX: tick % 3 === 0 ? (hash01(7, Math.floor(tick / 120)) > 0.5 ? 1 : -1) : 0,
      start: tick === 1,
      swing: tick % 260 === pressAt,
    });
  };
  const run = (): { readonly score: number; readonly results: string; readonly hashes: number[] } => {
    const s = new HomeRunSession(31);
    const hashes: number[] = [];
    for (let t = 1; t <= 12_000; t += 1) {
      s.advance(script(t));
      if (t % 500 === 0) {
        hashes.push(s.hash());
      }
    }
    return { hashes, results: JSON.stringify(s.results), score: s.score };
  };
  const a = run();
  const b = run();
  assert.equal(a.score, b.score);
  assert.equal(a.results, b.results);
  assert.deepEqual(a.hashes, b.hashes);
});

test("view() exposes a stable camera and hides the ball between pitches", () => {
  const s = new HomeRunSession(1);
  const v0 = s.view();
  assert.equal(v0.ballVisible, false);
  assert.deepEqual(v0.cameraPos, C.CAMERA_POS);
  s.advance(intent({ start: true }));
  let guard = 2000;
  while (s.phase !== "pitch" && guard > 0) {
    s.advance(IDLE);
    guard -= 1;
  }
  assert.ok(guard > 0);
  const v1 = s.view();
  assert.equal(v1.ballVisible, true);
  assert.ok(v1.ball.z <= C.PITCH_RELEASE.z);
});
