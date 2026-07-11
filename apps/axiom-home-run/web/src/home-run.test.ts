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
import { pitchPool, selectPitch, solvePitch } from "./pitch.ts";
import { catchingFielder, projectLanding, stepFielders, wanderPos } from "./fielders.ts";
import { beyondWall, classifyFlight, isFair, newFlight, scoreFor, stepFlight } from "./ball.ts";
import { HomeRunSession } from "./session.ts";
import * as C from "./constants.ts";

const IDLE: Intent = { holding: false, moveX: 0, released: false, start: false };
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
 * Play one pitch: hold from the first tick, release at `releaseTick`, optionally
 * stepping the batter first. Returns the first pitch's outcome.
 */
const playFirstPitch = (seed: number, releaseTick: number, moveX = 0, moveTicks = 0): Outcome => {
  const s = new HomeRunSession(seed);
  for (let t = 1; t <= releaseTick; t += 1) {
    const moving = t <= moveTicks ? moveX : 0;
    s.advance(intent({ holding: true, moveX: moving }));
  }
  s.advance(intent({ released: true }));
  let guard = 1200;
  while (s.results.length === 0 && guard > 0) {
    s.advance(IDLE);
    guard -= 1;
  }
  assert.ok(guard > 0, "pitch must resolve");
  return s.results[0]!.outcome;
};

// ── swing state machine ───────────────────────────────────────────────────────

test("the swing never fires on press — holding only loads", () => {
  let s = newSwing();
  s = stepSwing(s, true, false);
  assert.equal(s.state, "loading");
  for (let k = 0; k < 200; k += 1) {
    s = stepSwing(s, true, false);
    assert.ok(s.state === "loading" || s.state === "loaded", "held bat never swings");
  }
});

test("loading reaches a bounded maximum and winds the bat back", () => {
  let s = newSwing();
  let prevLoad = 0;
  for (let k = 0; k < 500; k += 1) {
    s = stepSwing(s, true, false);
    assert.ok(s.load <= 1, "load is bounded");
    assert.ok(s.load >= prevLoad, "load never regresses while held");
    prevLoad = s.load;
  }
  assert.equal(s.state, "loaded");
  assert.ok(s.load >= C.LOAD_FULL);
  assert.ok(Math.abs(s.theta - C.THETA_LOADED) < 0.03, "fully wound pose");
});

test("release triggers a fast forward swing scaled by load", () => {
  // Quick tap: barely loaded.
  let quick = newSwing();
  for (let k = 0; k < 3; k += 1) {
    quick = stepSwing(quick, true, false);
  }
  quick = stepSwing(quick, false, true);
  assert.equal(quick.state, "swing");

  // Full hold: maximum spring.
  let full = newSwing();
  for (let k = 0; k < 300; k += 1) {
    full = stepSwing(full, true, false);
  }
  full = stepSwing(full, false, true);
  assert.equal(full.state, "swing");
  assert.ok(full.omega > quick.omega, "longer load → faster swing");
  assert.ok(Math.abs(full.omega - C.OMEGA_MAX) < 1e-9);
  assert.ok(full.omega >= C.OMEGA_MIN);
});

test("full cycle: idle → loading → loaded → swing → follow → recover → idle, recovery slower than strike", () => {
  let s = newSwing();
  const seen: string[] = [s.state];
  const record = (): void => {
    if (seen[seen.length - 1] !== s.state) {
      seen.push(s.state);
    }
  };
  for (let k = 0; k < 300; k += 1) {
    s = stepSwing(s, true, false);
    record();
  }
  s = stepSwing(s, false, true);
  record();
  let strikeTicks = 0;
  while (s.state === "swing") {
    s = stepSwing(s, false, false);
    strikeTicks += 1;
    record();
  }
  let followTicks = 0;
  while (s.state === "follow") {
    s = stepSwing(s, false, false);
    followTicks += 1;
    record();
  }
  let recoverTicks = 0;
  while (s.state === "recover") {
    s = stepSwing(s, false, false);
    recoverTicks += 1;
    record();
    assert.ok(recoverTicks < 500, "recovery terminates");
  }
  record();
  assert.deepEqual(seen, ["idle", "loading", "loaded", "swing", "follow", "recover", "idle"]);
  assert.ok(recoverTicks > strikeTicks, "the bat eases home slower than it struck");
  assert.ok(followTicks > 0, "the bat overshoots into follow-through");
  assert.equal(s.theta, C.THETA_IDLE);
});

test("identical inputs produce identical bat poses (pure state machine)", () => {
  const script = (tick: number): { readonly hold: boolean; readonly rel: boolean } => ({
    hold: tick % 90 < 40,
    rel: tick % 90 === 40,
  });
  let a = newSwing();
  let b = newSwing();
  for (let t = 0; t < 800; t += 1) {
    const { hold, rel } = script(t);
    a = stepSwing(a, hold, rel);
    b = stepSwing(b, hold, rel);
    assert.equal(a.theta, b.theta);
    assert.equal(a.state, b.state);
    assert.equal(a.load, b.load);
  }
});

// ── contact model ────────────────────────────────────────────────────────────

const contactAt = (theta: number, r: number, dy: number) =>
  resolveContact(theta, C.OMEGA_MAX, r, dy, vec3(0, C.BAT_PLANE_Y + dy, 0), -0.3);

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

test("every fielder's wander stays inside its patrol circle", () => {
  for (let i = 0; i < C.FIELDER_SPOTS.length; i += 1) {
    const spot = C.FIELDER_SPOTS[i]!;
    for (let t = 0; t < 3000; t += 7) {
      const p = wanderPos(5, i, t);
      const d = Math.hypot(p.x - spot.x, p.z - spot.z);
      assert.ok(d <= spot.radius + 1e-9, `${spot.name} at t=${t}: ${d}`);
    }
  }
});

test("fielder wander reproduces from the seed and differs across fielders", () => {
  assert.deepEqual(wanderPos(3, 4, 500), wanderPos(3, 4, 500));
  assert.notDeepEqual(wanderPos(3, 4, 500), wanderPos(4, 4, 500));
  // Unsynchronized: two outfielders are not in phase.
  const a = wanderPos(3, 4, 500);
  const sa = C.FIELDER_SPOTS[4]!;
  const b = wanderPos(3, 6, 500);
  const sb = C.FIELDER_SPOTS[6]!;
  assert.notDeepEqual({ x: a.x - sa.x, z: a.z - sa.z }, { x: b.x - sb.x, z: b.z - sb.z });
});

test("a reachable landing point pulls nearby fielders into a clamped chase", () => {
  const fielders = C.FIELDER_SPOTS.map((s) => ({ chasing: false, x: s.x, z: s.z }));
  const cf = C.FIELDER_SPOTS.findIndex((s) => s.name === "CF");
  const landing = { x: C.FIELDER_SPOTS[cf]!.x + 1.5, z: C.FIELDER_SPOTS[cf]!.z + 1.5 };
  for (let t = 0; t < 200; t += 1) {
    stepFielders(fielders, 1, t, landing);
  }
  const f = fielders[cf]!;
  assert.ok(f.chasing, "CF reacts");
  assert.ok(Math.hypot(f.x - landing.x, f.z - landing.z) < 0.2, "CF converges on the landing point");
  const spot = C.FIELDER_SPOTS[cf]!;
  assert.ok(Math.hypot(f.x - spot.x, f.z - spot.z) <= spot.radius * C.FIELDER_CHASE_CLAMP + 1e-9, "never leaves the clamp");
  // A fielder across the field ignores it.
  const rf = C.FIELDER_SPOTS.findIndex((s) => s.name === "1B");
  assert.equal(fielders[rf]!.chasing, false);
});

test("catchingFielder requires closeness AND a catchable height", () => {
  const fielders = C.FIELDER_SPOTS.map((s) => ({ chasing: false, x: s.x, z: s.z }));
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

test("taking every pitch completes a 10-pitch round of misses", () => {
  const s = takeAllRound(11);
  assert.equal(s.phase, "over");
  assert.equal(s.results.length, C.PITCHES_PER_ROUND);
  assert.ok(s.results.every((r) => r.outcome === "miss"));
  assert.equal(s.score, 0);
  assert.equal(s.pitchNumber, C.PITCHES_PER_ROUND);
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

test("different release timings on the same pitch produce different outcomes", () => {
  const outcomes = new Set<Outcome>();
  for (let t = 90; t <= 135; t += 1) {
    outcomes.add(playFirstPitch(2, t));
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
  assert.equal(s.swing.state, "idle");
  assert.equal(s.pitchNumber, 1);
});

test("same seed + same input history reproduce the same final score and results", () => {
  const script = (tick: number): Intent => {
    const phase = tick % 260;
    const hold = phase >= 30 && phase < 30 + 40 + Math.floor(hash01(99, Math.floor(tick / 260)) * 50);
    const prevPhase = (tick - 1) % 260;
    const prevHold = prevPhase >= 30 && prevPhase < 30 + 40 + Math.floor(hash01(99, Math.floor((tick - 1) / 260)) * 50);
    return intent({
      holding: hold,
      moveX: tick % 3 === 0 ? (hash01(7, Math.floor(tick / 120)) > 0.5 ? 1 : -1) : 0,
      released: prevHold && !hold,
      start: tick === 1,
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
