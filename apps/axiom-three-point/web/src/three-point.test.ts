/*
 * three-point.test.ts — deterministic tests over the SDK-free core (vec / constants /
 * gameplay / physics / session). Runs under `node --test` with native TS
 * type-stripping: no wasm, no DOM, no SDK.
 *
 *   node --test apps/axiom-three-point/web/src/three-point.test.ts
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";

import {
  BALLS_PER_RACK,
  GOLDEN_BALL_INDEX,
  LOWER_PLANE_Y,
  PREVIEW_POINTS,
  PREVIEW_STRIDE_TICKS,
  RACK_COUNT,
  RIM_RADIUS,
  RIM_X,
  RIM_Y,
  RIM_Z,
  SHOT_TUNING,
  STATIONS,
  TOTAL_SHOTS,
  UPPER_PLANE_Y,
  rackSlotPosition,
} from "./constants.ts";
import { length, vec2, vec3 } from "./vec.ts";
import {
  AUTO_RELEASE_TICKS,
  chestAnchor,
  classifyOutcome,
  IDEAL_PROGRESS,
  INITIAL_DETECTION,
  launchVelocity,
  motionProgress,
  performanceLabel,
  pointsForMake,
  releaseParams,
  RISE_START_TICKS,
  risePosition,
  stepDetection,
  swipeIntents,
} from "./gameplay.ts";
import { PointerHistory } from "./pointer.ts";
import { RIM_COLLIDER_CENTERS, cloneBall, makeBall, predictTrajectory, stepBall } from "./physics.ts";
import { type Intent, IDLE_INTENT } from "./types.ts";
import { ThreePointSession } from "./session.ts";

const intent = (over: Partial<Intent>): Intent => ({ ...IDLE_INTENT, ...over });

/** Ticks to hold Space so the release lands at rise progress `p`. */
const holdTicksFor = (p: number): number => RISE_START_TICKS + Math.round(p * SHOT_TUNING.shotRiseTicks);

/** Advance until a ball is in hand and Space will start a shot (or results). */
const waitReady = (s: ThreePointSession): void => {
  for (let i = 0; i < 3000; i += 1) {
    if (s.phase === "results") return;
    if (s.phase === "ready" && s.ballInHand) return;
    s.advance(IDLE_INTENT);
  }
  assert.fail("session never became ready within 3000 ticks");
};

/** Advance until every airborne ball has resolved and its score was applied. */
const settleShot = (s: ThreePointSession): void => {
  for (let i = 0; i < 3000; i += 1) {
    const p = s.phase;
    if (s.ballsInFlight === 0 && (p === "ready" || p === "results")) return;
    s.advance(IDLE_INTENT);
  }
  assert.fail("shot did not settle within 3000 ticks");
};

/** Wait for the ball, press, hold `ticks` total, release, then settle. */
const playShot = (s: ThreePointSession, ticks: number): void => {
  waitReady(s);
  assert.equal(s.phase, "ready");
  s.advance(intent({ shootHeld: true, shootPressed: true }));
  for (let i = 1; i < ticks - 1; i += 1) s.advance(intent({ shootHeld: true }));
  s.advance(intent({ shootReleased: true }));
  settleShot(s);
};

/** Bring the aim back onto the station's hoop-facing base (the mouse work the
 * follow-through drift demands of a real player). */
const aimToBase = (s: ThreePointSession): void => {
  for (let i = 0; i < 60; i += 1) {
    const dYaw = STATIONS[s.stationIndex]!.baseYaw - s.yaw;
    const dPitch = SHOT_TUNING.pitchNeutral - s.pitch;
    if (Math.abs(dYaw) < 1e-6 && Math.abs(dPitch) < 1e-6) return;
    s.advance(intent({ lookDx: dYaw / SHOT_TUNING.aimYawSensitivity, lookDy: -dPitch / SHOT_TUNING.aimPitchSensitivity }));
  }
};

/**
 * Simulate one release directly through the physics + detector (no session):
 * from `station`/`slot` at base aim, released at rise progress `p`.
 */
const simulateRelease = (
  stationIndex: number,
  p: number,
  slot = 0,
  yawOffset = 0,
): { scored: boolean; touchedRim: boolean; touchedBackboard: boolean } => {
  const station = STATIONS[stationIndex]!;
  const yaw = station.baseYaw + yawOffset;
  const launch = launchVelocity(yaw, p);
  const ball = makeBall(risePosition(station, yaw, slot, p), launch.velocity, launch.angularVelocity);
  let detection = INITIAL_DETECTION;
  let touchedRim = false;
  let touchedBackboard = false;
  let scored = false;
  for (let t = 0; t < SHOT_TUNING.maxShotLifetimeTicks; t += 1) {
    const step = stepBall(ball);
    let hitFloor = false;
    for (const c of step.contacts) {
      touchedRim = touchedRim || c.surface === "rim";
      touchedBackboard = touchedBackboard || c.surface === "backboard";
      hitFloor = hitFloor || c.surface === "floor";
    }
    for (const sample of step.samples) {
      const r = stepDetection(detection, sample);
      detection = r.state;
      scored = scored || r.scoredNow;
    }
    if (scored || hitFloor) break;
  }
  return { scored, touchedBackboard, touchedRim };
};

/** A hold-tick count inside the advertised ideal window that scores — found once. */
const scoringHoldTicks = (): number => {
  for (let ticks = holdTicksFor(SHOT_TUNING.idealWindowStart); ticks <= holdTicksFor(SHOT_TUNING.idealWindowEnd); ticks += 1) {
    if (simulateRelease(1, motionProgress(ticks)).scored) return ticks;
  }
  assert.fail("no scoring hold inside the ideal window — tuning broken");
};

// ── motion progress ───────────────────────────────────────────────────────────

test("motion progress clamps and stays 0 through the pickup", () => {
  assert.equal(motionProgress(-100), 0);
  assert.equal(motionProgress(0), 0);
  assert.equal(motionProgress(RISE_START_TICKS), 0);
  assert.equal(motionProgress(RISE_START_TICKS + SHOT_TUNING.shotRiseTicks), 1);
  assert.equal(motionProgress(100_000), 1);
});

test("an instant tap releases a progress-0 dud", () => {
  const s = new ThreePointSession();
  waitReady(s);
  s.advance(intent({ shootHeld: true, shootPressed: true }));
  s.advance(intent({ shootReleased: true }));
  assert.equal(s.phase, "releasing");
  assert.equal(s.lastReleaseProgress, 0);
});

test("Space is ignored until the dealt ball reaches the chest", () => {
  const s = new ThreePointSession();
  assert.equal(s.ballInHand, false, "the first ball is still being picked up");
  s.advance(intent({ shootHeld: true, shootPressed: true }));
  assert.equal(s.phase, "ready", "a press during the pickup must not start the motion");
  waitReady(s);
  s.advance(intent({ shootHeld: true, shootPressed: true }));
  assert.equal(s.phase, "charging");
});

test("holding past the top auto-releases at full progress", () => {
  const s = new ThreePointSession();
  waitReady(s);
  s.advance(intent({ shootHeld: true, shootPressed: true }));
  for (let i = 0; i < AUTO_RELEASE_TICKS + 5; i += 1) s.advance(intent({ shootHeld: true }));
  assert.notEqual(s.phase, "charging", "the motion must not hold forever");
  assert.equal(s.lastReleaseProgress, 1);
});

test("the next ball is in the hands as soon as the previous one is released", () => {
  const s = new ThreePointSession();
  waitReady(s);
  s.advance(intent({ shootHeld: true, shootPressed: true }));
  for (let i = 0; i < 24; i += 1) s.advance(intent({ shootHeld: true }));
  s.advance(intent({ shootReleased: true }));
  assert.equal(s.ballIndex, 1, "the second ball is dealt at the release instant");
  // By the end of the follow-through the new ball is at the chest, ready to
  // shoot — while the first ball is still in the air.
  for (let i = 0; i < SHOT_TUNING.followThroughTicks + 2; i += 1) s.advance(IDLE_INTENT);
  assert.equal(s.phase, "ready");
  assert.equal(s.ballInHand, true);
  assert.ok(s.ballsInFlight >= 1, "the previous ball must still be flying");
});

test("several balls fly at once and scores apply in launch order", () => {
  const s = new ThreePointSession();
  const tap = (): void => {
    waitReady(s);
    s.advance(intent({ shootHeld: true, shootPressed: true }));
    s.advance(intent({ shootReleased: true }));
  };
  tap();
  tap();
  assert.equal(s.ballsInFlight, 2, "two duds airborne together");
  settleShot(s);
  assert.equal(s.shotsTaken, 2);
  assert.equal(s.score, 0);
  assert.equal(s.streak, 0);
});

// ── release curves ────────────────────────────────────────────────────────────

test("release curves are stable, bounded, and hit their tuned keyframes", () => {
  for (let i = 0; i <= 20; i += 1) {
    const p = i / 20;
    assert.deepEqual(releaseParams(p), releaseParams(p), "same progress must give the same params");
  }
  const early = releaseParams(0);
  const ideal = releaseParams(IDEAL_PROGRESS);
  const late = releaseParams(1);
  assert.equal(early.speed, Math.hypot(SHOT_TUNING.earlyReleaseForwardSpeed, SHOT_TUNING.earlyReleaseVerticalSpeed));
  assert.equal(ideal.speed, Math.hypot(SHOT_TUNING.idealReleaseForwardSpeed, SHOT_TUNING.idealReleaseVerticalSpeed));
  assert.equal(late.speed, Math.hypot(SHOT_TUNING.lateReleaseForwardSpeed, SHOT_TUNING.lateReleaseVerticalSpeed));
});

test("the effective aim changes throughout the rising shot motion", () => {
  const samples = [0, 0.2, 0.4, IDEAL_PROGRESS, 0.85, 1].map((p) => releaseParams(p).aimPitch);
  for (let i = 0; i < samples.length; i += 1) {
    for (let j = i + 1; j < samples.length; j += 1) {
      assert.notEqual(samples[i], samples[j], `aim pitch at samples ${i} and ${j} must differ`);
    }
  }
  // The aim rises from the early slump up to the ideal alignment.
  assert.ok(samples[0]! < samples[1]! && samples[1]! < samples[3]!, "aim must rise toward the ideal window");
});

test("early, ideal, and late releases generate different deterministic trajectories", () => {
  const yaw = STATIONS[1]!.baseYaw;
  const runs = [0.15, IDEAL_PROGRESS, 1].map((p) => {
    const a = launchVelocity(yaw, p);
    const b = launchVelocity(yaw, p);
    assert.deepEqual(a, b, "identical progress must launch identically");
    const ball = makeBall(risePosition(STATIONS[1]!, yaw, 0, p), a.velocity, a.angularVelocity);
    for (let t = 0; t < 30; t += 1) stepBall(ball);
    return { pos: ball.pos, v: a.velocity };
  });
  assert.notDeepEqual(runs[0]!.v, runs[1]!.v);
  assert.notDeepEqual(runs[1]!.v, runs[2]!.v);
  assert.notDeepEqual(runs[0]!.pos, runs[1]!.pos);
  assert.notDeepEqual(runs[1]!.pos, runs[2]!.pos);
});

test("identical mouse input and release progress produce identical launch velocity", () => {
  const run = (): { hash: number; p: number } => {
    const s = new ThreePointSession();
    for (let i = 0; i < 25; i += 1) s.advance(intent({ lookDx: 4, lookDy: -3 }));
    playShot(s, 40);
    return { hash: s.hash(), p: s.lastReleaseProgress };
  };
  const a = run();
  const b = run();
  assert.equal(a.p, b.p);
  assert.equal(a.hash, b.hash);
  assert.deepEqual(launchVelocity(0.17, 0.63), launchVelocity(0.17, 0.63));
});

// ── pickup pose ───────────────────────────────────────────────────────────────

test("rack slot position deterministically changes the pickup pose", () => {
  const station = STATIONS[0]!;
  const entries = Array.from({ length: BALLS_PER_RACK }, (_, slot) => rackSlotPosition(station, slot));
  const chests = Array.from({ length: BALLS_PER_RACK }, (_, slot) => chestAnchor(station, station.baseYaw, slot));
  for (let a = 0; a < BALLS_PER_RACK; a += 1) {
    assert.deepEqual(chestAnchor(station, station.baseYaw, a), chests[a], "the pose must be deterministic");
    for (let b = a + 1; b < BALLS_PER_RACK; b += 1) {
      assert.notDeepEqual(entries[a], entries[b], `slots ${a}/${b} must enter from different points`);
      assert.notDeepEqual(chests[a], chests[b], `slots ${a}/${b} must present differently`);
    }
  }
});

test("the game never changes the player's aim", () => {
  const s = new ThreePointSession();
  const yawBefore = s.yaw;
  const pitchBefore = s.pitch;
  // A full rack of shots with zero mouse input: pickup, rise, release,
  // follow-through, feedback — none of it may touch the view.
  for (let shot = 0; shot < BALLS_PER_RACK; shot += 1) {
    playShot(s, scoringHoldTicks());
    assert.equal(s.yaw, yawBefore, `yaw changed after shot ${shot}`);
    assert.equal(s.pitch, pitchBefore, `pitch changed after shot ${shot}`);
  }
  // The rack glide moves the POSITION only; orientation stays mouse-owned.
  for (let i = 0; i < 3000 && s.phase !== "ready"; i += 1) s.advance(IDLE_INTENT);
  assert.equal(s.stationIndex, 1);
  assert.equal(s.yaw, yawBefore, "the glide must not retarget the view");
  assert.equal(s.pitch, pitchBefore);
  // And mid-motion the camera target is derived from the stored aim alone.
  s.advance(intent({ shootHeld: true, shootPressed: true }));
  for (let i = 0; i < 20; i += 1) s.advance(intent({ shootHeld: true }));
  const view = s.view();
  const dir = {
    x: view.cameraTarget.x - view.cameraPosition.x,
    y: view.cameraTarget.y - view.cameraPosition.y,
    z: view.cameraTarget.z - view.cameraPosition.z,
  };
  assert.ok(Math.abs(Math.atan2(dir.x, -dir.z) - s.yaw) < 1e-9, "camera yaw must equal the stored aim mid-motion");
  assert.ok(Math.abs(Math.asin(dir.y / Math.hypot(dir.x, dir.y, dir.z)) - s.pitch) < 1e-9, "camera pitch must equal the stored aim mid-motion");
});

test("the soft yaw bound blocks outward movement but never snaps the view", () => {
  const s = new ThreePointSession();
  const base = STATIONS[0]!.baseYaw;
  // Sweep far right: yaw stops at the bound instead of wrapping or snapping.
  for (let i = 0; i < 40; i += 1) s.advance(intent({ lookDx: 200 }));
  assert.ok(Math.abs(s.yaw - (base + SHOT_TUNING.yawClampHalf)) < 0.05, "yaw should rest at the soft bound");
  const atEdge = s.yaw;
  s.advance(intent({ lookDx: 500 }));
  assert.equal(s.yaw, atEdge, "outward movement past the bound is blocked");
  s.advance(intent({ lookDx: -500 * 0.5 }));
  assert.ok(s.yaw < atEdge, "inward movement always passes through");
});

test("station changes never make shots without fresh horizontal aim", () => {
  // With the camera fully player-owned, an ideal-timed Space-tapper who never
  // moves the mouse keeps station 0's aim forever: racks 2 and 3 point 0.7 rad
  // away from their hoop line, so only the first rack can score.
  const s = new ThreePointSession();
  const ticks = scoringHoldTicks();
  for (let shot = 0; shot < TOTAL_SHOTS; shot += 1) playShot(s, ticks);
  assert.equal(s.phase, "results");
  assert.ok(s.makes <= BALLS_PER_RACK, `no-mouse tapping must not score beyond rack 1 (made ${s.makes}/15)`);
  // While re-aiming at each station turns the same timing into makes everywhere.
  const skilled = new ThreePointSession();
  for (let shot = 0; shot < TOTAL_SHOTS; shot += 1) {
    waitReady(skilled);
    aimToBase(skilled);
    playShot(skilled, ticks);
  }
  assert.ok(skilled.makes > BALLS_PER_RACK, "re-aiming per station scores on every rack");
});

// ── the swipe shot (mobile) ───────────────────────────────────────────────────

test("swipe intents follow the swipe-basketball gesture model", () => {
  const dead = SHOT_TUNING.swipeGestureDeadzone;
  const full = SHOT_TUNING.swipeGestureFull;
  // Below the deadzone (or downward/sideways-only) a lift-off is not a shot.
  assert.equal(swipeIntents(vec2(0, 0)), null);
  assert.equal(swipeIntents(vec2(0, -dead)), null);
  assert.equal(swipeIntents(vec2(30, 10)), null, "a downward drag never shoots");
  // Progress is deadzone→full normalized from the UPWARD flick strength.
  const mid = swipeIntents(vec2(0, -(dead + (full - dead) / 2)))!;
  assert.ok(Math.abs(mid.progress - 0.5) < 1e-9);
  assert.equal(swipeIntents(vec2(0, -full * 3))!.progress, 1, "harder than full clamps to 1");
  // The sideways flick is a BOUNDED launch-yaw offset, deterministic.
  const left = swipeIntents(vec2(-full, -full))!;
  assert.ok(Math.abs(left.yawOffset + SHOT_TUNING.swipeLateralMaxYaw) < 1e-9);
  assert.equal(swipeIntents(vec2(full * 9, -full))!.yawOffset, SHOT_TUNING.swipeLateralMaxYaw);
  assert.deepEqual(swipeIntents(vec2(11, -23)), swipeIntents(vec2(11, -23)), "identical swipes launch identically");
});

test("a smoothed swipe history survives jitter and glitches", () => {
  const h = new PointerHistory();
  // Uniform upward motion → the smoothed velocity is that motion.
  for (let t = 0; t < 8; t += 1) h.push(100, 400 - t * 20, t);
  const v = h.releaseVelocity();
  assert.ok(Math.abs(v.y + 20) < 1e-9 && Math.abs(v.x) < 1e-9);
  // A tab-switch teleport clears the history instead of throwing a garbage flick.
  h.push(100, 4000, 9);
  assert.equal(h.size, 1);
  assert.deepEqual(h.releaseVelocity(), { x: 0, y: 0 });
});

test("a swipe launches the shot at the flick's progress without touching the aim", () => {
  const s = new ThreePointSession();
  waitReady(s);
  const yawBefore = s.yaw;
  const pitchBefore = s.pitch;
  s.advance(intent({ swipe: { progress: 0.64, yawOffset: 0.05 } }));
  assert.equal(s.phase, "releasing", "a swipe is the whole shot");
  assert.equal(s.lastReleaseProgress, 0.64);
  assert.equal(s.ballIndex, 1, "the next ball is dealt at the swipe release");
  assert.equal(s.yaw, yawBefore, "a swipe must never rotate the camera");
  assert.equal(s.pitch, pitchBefore);
  // The offset steered the launch: the airborne ball drifts off the aim line.
  settleShot(s);
  assert.equal(s.shotsTaken, 1);
});

test("an ideal swipe scores; its lateral offset steers the launch", () => {
  const s = new ThreePointSession();
  waitReady(s);
  s.advance(intent({ swipe: { progress: motionProgress(scoringHoldTicks()), yawOffset: 0 } }));
  settleShot(s);
  assert.equal(s.makes, 1, "a clean ideal-strength swipe scores");
  // The same swipe flicked hard sideways misses.
  waitReady(s);
  s.advance(intent({ swipe: { progress: motionProgress(scoringHoldTicks()), yawOffset: SHOT_TUNING.swipeLateralMaxYaw } }));
  settleShot(s);
  assert.equal(s.makes, 1, "a full-lateral flick must not score");
});

test("a swipe is ignored while no ball is in hand", () => {
  const s = new ThreePointSession();
  assert.equal(s.ballInHand, false);
  s.advance(intent({ swipe: { progress: 0.6, yawOffset: 0 } }));
  assert.equal(s.phase, "ready", "swiping during the pickup must not launch");
  assert.equal(s.shotsTaken, 0);
});

// ── scoring ───────────────────────────────────────────────────────────────────

test("streak scoring follows pointsAwarded = 3 + 3 * currentStreak", () => {
  assert.equal(pointsForMake(0), 3);
  assert.equal(pointsForMake(1), 6);
  assert.equal(pointsForMake(2), 9);
  assert.equal(pointsForMake(3), 12);
});

test("session awards streak points in order and a miss resets the streak", () => {
  const make = scoringHoldTicks();
  const s = new ThreePointSession();
  aimToBase(s);
  playShot(s, make);
  assert.equal(s.score, 3, "first make = 3");
  assert.equal(s.streak, 1);
  aimToBase(s);
  playShot(s, make);
  assert.equal(s.score, 9, "second consecutive make adds 6");
  assert.equal(s.streak, 2);
  // A deliberate dud: a pickup-instant release falls far short.
  playShot(s, 2);
  assert.equal(s.streak, 0, "a miss resets the streak");
  assert.equal(s.score, 9, "a miss never removes points");
  aimToBase(s);
  playShot(s, make);
  assert.equal(s.score, 12, "streak restarts at 3 points");
  assert.equal(s.bestStreak, 2);
});

// ── basket detection ──────────────────────────────────────────────────────────

const fall = (prevY: number, y: number, horizDistSq = 0): Parameters<typeof stepDetection>[1] => ({
  horizDistSq,
  prevY,
  velY: (y - prevY) * 60,
  y,
});

test("basket detection requires upper-then-lower downward crossing", () => {
  let d = INITIAL_DETECTION;
  let r = stepDetection(d, fall(LOWER_PLANE_Y + 0.05, LOWER_PLANE_Y - 0.05));
  assert.equal(r.scoredNow, false);

  d = INITIAL_DETECTION;
  r = stepDetection(d, fall(UPPER_PLANE_Y + 0.05, UPPER_PLANE_Y - 0.05));
  assert.equal(r.state.enteredFromAbove, true);
  r = stepDetection(r.state, fall(LOWER_PLANE_Y + 0.05, LOWER_PLANE_Y - 0.05));
  assert.equal(r.scoredNow, true);
});

test("a downward crossing outside the scoring cylinder does not arm or score", () => {
  const wide = (RIM_RADIUS + SHOT_TUNING.scoreDetectionTolerance + 0.05) ** 2;
  let r = stepDetection(INITIAL_DETECTION, fall(UPPER_PLANE_Y + 0.05, UPPER_PLANE_Y - 0.05, wide));
  assert.equal(r.state.enteredFromAbove, false);
  r = stepDetection({ enteredFromAbove: true, scored: false }, fall(LOWER_PLANE_Y + 0.05, LOWER_PLANE_Y - 0.05, wide));
  assert.equal(r.scoredNow, false);
});

test("upward crossings never score and clear the entry record", () => {
  const rise = (prevY: number, y: number): Parameters<typeof stepDetection>[1] => ({
    horizDistSq: 0,
    prevY,
    velY: (y - prevY) * 60,
    y,
  });
  let d = { enteredFromAbove: false, scored: false };
  let r = stepDetection(d, rise(LOWER_PLANE_Y - 0.05, LOWER_PLANE_Y + 0.05));
  assert.equal(r.scoredNow, false);
  r = stepDetection(r.state, rise(UPPER_PLANE_Y - 0.05, UPPER_PLANE_Y + 0.05));
  assert.equal(r.scoredNow, false);
  d = { enteredFromAbove: true, scored: false };
  r = stepDetection(d, rise(UPPER_PLANE_Y - 0.05, UPPER_PLANE_Y + 0.05));
  assert.equal(r.state.enteredFromAbove, false);
  r = stepDetection(r.state, fall(LOWER_PLANE_Y + 0.05, LOWER_PLANE_Y - 0.05));
  assert.equal(r.scoredNow, false, "after the clear, a lower crossing alone must not score");
});

test("a ball cannot score twice", () => {
  let r = stepDetection({ enteredFromAbove: true, scored: false }, fall(LOWER_PLANE_Y + 0.05, LOWER_PLANE_Y - 0.05));
  assert.equal(r.scoredNow, true);
  r = stepDetection(r.state, fall(UPPER_PLANE_Y + 0.05, UPPER_PLANE_Y - 0.05));
  r = stepDetection(r.state, fall(LOWER_PLANE_Y + 0.05, LOWER_PLANE_Y - 0.05));
  assert.equal(r.scoredNow, false, "scored latches — no duplicate baskets");
});

// ── progression ───────────────────────────────────────────────────────────────

test("rack progression produces exactly 15 shots and two rack transitions", () => {
  const s = new ThreePointSession();
  let sawMoving = 0;
  for (let shot = 0; shot < TOTAL_SHOTS; shot += 1) {
    waitReady(s);
    assert.equal(s.phase, "ready", `shot ${shot} must start from ready`);
    s.advance(intent({ shootHeld: true, shootPressed: true }));
    for (let i = 0; i < 24; i += 1) s.advance(intent({ shootHeld: true }));
    s.advance(intent({ shootReleased: true }));
    let wasMoving = false;
    for (let i = 0; i < 3000 && !(s.phase === "results" || (s.phase === "ready" && s.ballInHand)); i += 1) {
      s.advance(IDLE_INTENT);
      wasMoving = wasMoving || (s.phase as string) === "movingToNextRack";
    }
    if (wasMoving) sawMoving += 1;
  }
  for (let i = 0; i < 3000 && s.phase !== "results"; i += 1) s.advance(IDLE_INTENT);
  assert.equal(s.phase, "results", "the 15th shot must lead to results");
  assert.equal(sawMoving, RACK_COUNT - 1, "exactly two rack transitions");
  assert.equal(s.shotsTaken, TOTAL_SHOTS);
  s.advance(intent({ shootPressed: true }));
  assert.equal(s.phase, "results", "Space in results must not start a 16th shot");
});

test("the golden ball is the fifth ball at every rack", () => {
  assert.equal(GOLDEN_BALL_INDEX, BALLS_PER_RACK - 1);
  const s = new ThreePointSession();
  for (let rack = 0; rack < RACK_COUNT; rack += 1) {
    for (let ball = 0; ball < BALLS_PER_RACK; ball += 1) {
      assert.equal(s.hud().golden, ball === GOLDEN_BALL_INDEX, `rack ${rack} ball ${ball}`);
      playShot(s, 22);
    }
  }
  assert.equal(s.phase, "results");
});

test("the results screen reports makes, best streak, and the performance label", () => {
  assert.equal(performanceLabel(0), "WARMING UP");
  assert.equal(performanceLabel(4), "WARMING UP");
  assert.equal(performanceLabel(5), "SHARPSHOOTER");
  assert.equal(performanceLabel(8), "SHARPSHOOTER");
  assert.equal(performanceLabel(9), "ON FIRE");
  assert.equal(performanceLabel(12), "ON FIRE");
  assert.equal(performanceLabel(13), "UNSTOPPABLE");
  assert.equal(performanceLabel(15), "UNSTOPPABLE");

  const make = scoringHoldTicks();
  const s = new ThreePointSession();
  for (let shot = 0; shot < TOTAL_SHOTS; shot += 1) {
    aimToBase(s);
    playShot(s, make);
  }
  assert.equal(s.phase, "results");
  const results = s.results!;
  assert.ok(results.makes >= 12, `aim-corrected ideal timing should make nearly all (made ${results.makes})`);
  assert.equal(results.makes, s.makes);
  assert.equal(results.bestStreak, s.bestStreak);
  assert.equal(results.label, performanceLabel(s.makes));
  assert.equal(results.score, s.score);
});

// ── restart ───────────────────────────────────────────────────────────────────

test("restart returns all observable state to its original values", () => {
  const fresh = new ThreePointSession();
  const freshHud = JSON.stringify(fresh.hud());
  const freshView = JSON.stringify(fresh.view());

  const s = new ThreePointSession();
  for (let shot = 0; shot < TOTAL_SHOTS; shot += 1) playShot(s, 30);
  assert.equal(s.phase, "results");
  s.advance(intent({ restartPressed: true }));
  assert.equal(s.phase, "ready");
  assert.equal(JSON.stringify(s.hud()), freshHud);
  assert.equal(JSON.stringify(s.view()), freshView);
  assert.equal(s.score, 0);
  assert.equal(s.shotsTaken, 0);

  const mid = new ThreePointSession();
  playShot(mid, 40);
  mid.advance(intent({ shootHeld: true, shootPressed: true }));
  mid.advance(intent({ restartPressed: true }));
  assert.equal(JSON.stringify(mid.hud()), freshHud);
});

// ── the rim you see is the rim you hit ────────────────────────────────────────

test("rim collider ring lies exactly on the visual torus circle", () => {
  assert.equal(RIM_COLLIDER_CENTERS.length >= 12, true);
  for (const c of RIM_COLLIDER_CENTERS) {
    const d = Math.hypot(c.x - RIM_X, c.z - RIM_Z);
    assert.ok(Math.abs(d - RIM_RADIUS) < 1e-9, "collider center off the rim circle");
    assert.equal(c.y, RIM_Y);
  }
});

// ── winnability + extremes (pins the tuning) ──────────────────────────────────

test("the ideal release window is achievable from every rack position", () => {
  for (let station = 0; station < RACK_COUNT; station += 1) {
    for (let slot = 0; slot < BALLS_PER_RACK; slot += 1) {
      let scored = false;
      for (let ticks = holdTicksFor(SHOT_TUNING.idealWindowStart); ticks <= holdTicksFor(SHOT_TUNING.idealWindowEnd); ticks += 1) {
        scored = scored || simulateRelease(station, motionProgress(ticks), slot).scored;
      }
      assert.ok(scored, `station ${station} slot ${slot} must score inside the ideal window`);
    }
  }
  // Too early and maximum hold both produce misses.
  assert.equal(simulateRelease(1, 0).scored, false, "a pickup-instant release must fall short");
  assert.equal(simulateRelease(0, 1).scored, false, "a maximum hold must miss (left wing)");
  assert.equal(simulateRelease(2, 1).scored, false, "a maximum hold must miss (right wing)");
});

test("the reticle is never repositioned by the game", () => {
  // The reticle carries no position — the game may only choose its visibility.
  const s = new ThreePointSession();
  assert.deepEqual(Object.keys(s.hud().reticle), ["mode"], "a reticle position field would let the game move it");
  waitReady(s);
  assert.equal(s.hud().reticle.mode, "active", "ball in hand → bright crosshair");
  s.advance(intent({ shootHeld: true, shootPressed: true }));
  for (let i = 0; i < 20; i += 1) s.advance(intent({ shootHeld: true }));
  assert.equal(s.hud().reticle.mode, "active", "rising → still just the crosshair");
  s.advance(intent({ shootReleased: true }));
  assert.equal(s.hud().reticle.mode, "dim", "ball away → faint crosshair");
});

test("outcome classification distinguishes swish, made, rim, backboard, miss", () => {
  assert.equal(classifyOutcome(true, false, false), "swish");
  assert.equal(classifyOutcome(true, true, false), "made");
  assert.equal(classifyOutcome(true, false, true), "made");
  assert.equal(classifyOutcome(false, true, false), "rim");
  assert.equal(classifyOutcome(false, false, true), "backboard");
  assert.equal(classifyOutcome(false, false, false), "miss");
});

// ── the trajectory preview is the real trajectory ─────────────────────────────

test("trajectory preview equals the actual flight for the same launch", () => {
  const station = STATIONS[1]!;
  const launch = launchVelocity(station.baseYaw, 0.6);
  const start = makeBall(risePosition(station, station.baseYaw, 2, 0.6), launch.velocity, launch.angularVelocity);
  const preview = predictTrajectory(start, PREVIEW_POINTS, PREVIEW_STRIDE_TICKS);
  const real = cloneBall(start);
  for (let i = 0; i < PREVIEW_POINTS; i += 1) {
    for (let k = 0; k < PREVIEW_STRIDE_TICKS; k += 1) stepBall(real);
    assert.deepEqual(preview[i], real.pos, `preview point ${i} must equal the real flight`);
  }
});

// ── physics sanity: energy never increases in free flight ─────────────────────

test("free flight never gains energy", () => {
  const ball = makeBall(vec3(3, 5, 8), vec3(1.5, 4, -3), vec3(0, 0, 0));
  const energy = (): number => 0.5 * length(ball.vel) ** 2 + 9.8 * ball.pos.y;
  let prev = energy();
  for (let t = 0; t < 120; t += 1) {
    stepBall(ball);
    const e = energy();
    assert.ok(e <= prev + 1e-6, `energy rose at tick ${t}`);
    prev = e;
    if (ball.pos.y <= 0.13) break;
  }
});
