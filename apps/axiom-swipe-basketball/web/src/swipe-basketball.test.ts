/*
 * Deterministic game-logic tests for Swipe Basketball. They run in bare Node
 * (`node --test`, native TS type-stripping) because the whole core imports nothing
 * from `@axiom/game` — no wasm, no browser. They pin the behaviours the brief
 * requires: pointer-sample velocity, swipe→throw mapping, ball selection, one-way
 * scoring, reset, the bounded pointer history, and replay determinism.
 *
 * Run: node --test apps/axiom-swipe-basketball/web/src/swipe-basketball.test.ts
 */

import assert from "node:assert/strict";
import test from "node:test";

import { type Vec2, vec2, vec3 } from "./vec.ts";
import { type Camera, project, viewProjection } from "./projection.ts";
import { PointerHistory } from "./pointer.ts";
import { swipeToThrow, throwIntents } from "./throw.ts";
import { pickBall } from "./selection.ts";
import { scoredThroughHoop } from "./scoring.ts";
import { type Intent, SwipeBasketballSession, rackPositions } from "./session.ts";
import {
  basePoints,
  classifyShot,
  inFinalWindow,
  isGoldenSpawn,
  newRound,
  registerMake,
  registerMiss,
  registerShot,
  startIfReady,
  tick as arcadeTick,
} from "./arcade.ts";
import {
  BALL_COUNT,
  CAMERA_FAR,
  CAMERA_FOV_Y,
  CAMERA_NEAR,
  CAMERA_POS,
  CAMERA_TARGET,
  FINAL_MULTIPLIER,
  FINAL_TICKS,
  FIXED_HZ,
  GOLDEN_EVERY,
  HOOP_X,
  HOOP_Y,
  HOOP_Z,
  MAX_POINTER_DELTA,
  POINTER_HISTORY,
  POINTS_GOLDEN,
  POINTS_SWISH,
  ROUND_TICKS,
  STREAK_MULT_CAP,
  STREAK_STEP,
  THROW_FORWARD_MAX,
  THROW_FORWARD_MIN,
  THROW_VERTICAL_MIN,
  THROW_VERTICAL_TO_FORWARD_MAX_RATIO,
  TRIGGER_HALF_W,
} from "./constants.ts";

const VIEWPORT: Vec2 = vec2(960, 600);
const CAMERA: Camera = {
  far: CAMERA_FAR,
  fovY: CAMERA_FOV_Y,
  near: CAMERA_NEAR,
  position: CAMERA_POS,
  target: CAMERA_TARGET,
  up: vec3(0, 1, 0),
};
const VIEW_PROJ = viewProjection(CAMERA, VIEWPORT.x / VIEWPORT.y);

/** The canvas-pixel position a rack ball projects to (for driving pointer intents). */
const projectRackBall = (i: number): Vec2 => project(rackPositions()[i]!, VIEW_PROJ, VIEWPORT).pos;

const idle: Intent = { pointer: null, pressed: false, released: false, reset: false, viewport: VIEWPORT };

// ── 1. pointer-sample velocity ────────────────────────────────────────────────

test("1. release velocity is (dx,dy)/ticks over the recent window", () => {
  const h = new PointerHistory();
  h.push(100, 300, 10);
  h.push(130, 240, 12); // +30 x, −60 y over 2 ticks
  const v = h.releaseVelocity();
  assert.equal(v.x, 15);
  assert.equal(v.y, -30);
});

test("1b. fewer than two samples yields zero velocity", () => {
  const h = new PointerHistory();
  h.push(50, 50, 1);
  assert.deepEqual(h.releaseVelocity(), vec2(0, 0));
});

// ── 2. constrained arcade throw model ─────────────────────────────────────────

test("2. an upward swipe lifts and carries forward, forward-dominant (−Z)", () => {
  const throwVel = swipeToThrow(vec2(0, -40));
  assert.ok(throwVel.y > 0, "upward swipe gives positive lift");
  assert.ok(throwVel.z < 0, "throw goes into the machine (−Z)");
  assert.ok(Math.abs(throwVel.z) > throwVel.y, "forward speed dominates vertical");
});

test("2a. upward swipe cannot exceed the vertical/forward clamp (no rainbow)", () => {
  for (const gy of [-15, -30, -60, -120, -500]) {
    const intents = throwIntents(vec2(0, gy));
    const ratio = intents.vertical / intents.forward;
    assert.ok(
      ratio <= THROW_VERTICAL_TO_FORWARD_MAX_RATIO + 1e-9,
      `vertical/forward ${ratio.toFixed(3)} exceeds clamp for gy=${gy}`,
    );
  }
});

test("2b. a stronger swipe increases forward speed", () => {
  const soft = throwIntents(vec2(0, -15));
  const hard = throwIntents(vec2(0, -40));
  assert.ok(hard.forward > soft.forward, "harder upward flick has more forward speed");
  assert.ok(hard.forward <= THROW_FORWARD_MAX + 1e-9 && soft.forward >= THROW_FORWARD_MIN - 1e-9, "forward stays in range");
});

test("2c. a lateral swipe changes the X launch velocity", () => {
  assert.ok(swipeToThrow(vec2(40, -30)).x > 0, "rightward swipe → +X");
  assert.ok(swipeToThrow(vec2(-40, -30)).x < 0, "leftward swipe → −X");
  assert.equal(swipeToThrow(vec2(0, -30)).x, 0, "a purely vertical swipe has no lateral");
});

test("2d. a very weak swipe stays at the floor of the launch range (falls short)", () => {
  const weak = throwIntents(vec2(0, -3)); // below the gesture dead-zone
  assert.equal(weak.power, 0, "a sub-dead-zone flick has zero power");
  assert.equal(weak.forward, THROW_FORWARD_MIN, "weak forward is the minimum");
  assert.equal(weak.vertical, THROW_VERTICAL_MIN, "weak lift is the minimum");
});

test("2e. a strong flick drives forward, not straight up (forward-dominant)", () => {
  const strong = throwIntents(vec2(0, -200));
  assert.ok(strong.forward > strong.vertical, "forward speed exceeds lift even at full power");
});

// ── 3. ball selection from a pointer hit ──────────────────────────────────────

test("3. the pointer selects the rack ball under it, or none when off", () => {
  const balls = rackPositions().map((pos) => ({ pos, selectable: true }));
  const onBall2 = projectRackBall(2);
  assert.equal(pickBall(onBall2, balls, VIEW_PROJ, VIEWPORT), 2);
  // Far off in the corner → nothing.
  assert.equal(pickBall(vec2(5, 5), balls, VIEW_PROJ, VIEWPORT), -1);
  // A ball already in flight is not selectable.
  const noneSelectable = rackPositions().map((pos) => ({ pos, selectable: false }));
  assert.equal(pickBall(onBall2, noneSelectable, VIEW_PROJ, VIEWPORT), -1);
});

// ── 4. one-way scoring ────────────────────────────────────────────────────────

test("4. a downward pass through the hoop opening scores", () => {
  const above = vec3(HOOP_X, HOOP_Y + 0.05, HOOP_Z);
  const below = vec3(HOOP_X, HOOP_Y - 0.05, HOOP_Z);
  assert.ok(scoredThroughHoop(above, below, vec3(0, -3, 0)));
});

test("4b. rising up through the hoop from below does NOT score", () => {
  const below = vec3(HOOP_X, HOOP_Y - 0.05, HOOP_Z);
  const above = vec3(HOOP_X, HOOP_Y + 0.05, HOOP_Z);
  assert.ok(!scoredThroughHoop(below, above, vec3(0, 3, 0)));
});

test("4c. a downward crossing OUTSIDE the opening does NOT score", () => {
  const above = vec3(HOOP_X + TRIGGER_HALF_W + 0.2, HOOP_Y + 0.05, HOOP_Z);
  const below = vec3(HOOP_X + TRIGGER_HALF_W + 0.2, HOOP_Y - 0.05, HOOP_Z);
  assert.ok(!scoredThroughHoop(above, below, vec3(0, -3, 0)));
});

test("4d. a ball moving upward while crossing down (glitch) does NOT score", () => {
  const above = vec3(HOOP_X, HOOP_Y + 0.05, HOOP_Z);
  const below = vec3(HOOP_X, HOOP_Y - 0.05, HOOP_Z);
  assert.ok(!scoredThroughHoop(above, below, vec3(0, 2, 0)));
});

// ── 5. reset ──────────────────────────────────────────────────────────────────

/** Drive a full grab + release of rack ball `i`, returning the session mid-shot. */
const grabAndRelease = (session: SwipeBasketballSession, i: number): void => {
  const at = projectRackBall(i);
  session.advance({ pointer: at, pressed: true, released: false, reset: false, viewport: VIEWPORT });
  session.advance({ pointer: at, pressed: false, released: true, reset: false, viewport: VIEWPORT });
};

test("5. reset restores score, shots, and every ball to the rack", () => {
  const session = new SwipeBasketballSession();
  grabAndRelease(session, 0);
  assert.equal(session.shots, 1, "a release counts as a shot");
  assert.equal(session.ballViews()[0]!.mode, "flight", "the released ball is in flight");

  session.advance({ ...idle, reset: true });
  assert.equal(session.score, 0);
  assert.equal(session.shots, 0);
  for (const ball of session.ballViews()) {
    assert.equal(ball.mode, "rack");
  }
});

// ── 6. bounded pointer history ────────────────────────────────────────────────

test("6. the pointer history never grows past its capacity", () => {
  const h = new PointerHistory();
  for (let t = 0; t < POINTER_HISTORY * 4; t += 1) {
    h.push(t % 5, (t * 2) % 7, t);
  }
  assert.equal(h.size, POINTER_HISTORY);
});

test("6b. a giant delta (tab-switch glitch) discards the history", () => {
  const h = new PointerHistory();
  h.push(100, 100, 1);
  h.push(105, 98, 2);
  h.push(100 + MAX_POINTER_DELTA + 50, 98, 3); // absurd jump
  assert.equal(h.size, 1, "history resets to the fresh sample");
  assert.deepEqual(h.releaseVelocity(), vec2(0, 0));
});

test("6c. a jittery final sample does not dominate the smoothed release velocity", () => {
  const h = new PointerHistory();
  // A steady upward swipe (x fixed, y climbing) …
  for (let t = 1; t <= 5; t += 1) {
    h.push(200, 400 - 25 * (t - 1), t);
  }
  // … then one twitchy final sample that jumps sideways.
  h.push(260, 275, 6);
  const v = h.releaseVelocity();
  assert.ok(Math.abs(v.x) < 20, `smoothed x ${v.x.toFixed(1)} should not follow the +60 final jitter`);
  assert.ok(v.y < -20, "the sustained upward swipe still dominates the vertical velocity");
});

// ── 7. determinism ────────────────────────────────────────────────────────────

test("7. identical intent sequences produce identical state (replayable)", () => {
  const script: Intent[] = [];
  const at = projectRackBall(1);
  script.push({ pointer: at, pressed: true, released: false, reset: false, viewport: VIEWPORT });
  const drag = vec2(at.x + 30, at.y - 120);
  script.push({ pointer: drag, pressed: false, released: false, reset: false, viewport: VIEWPORT });
  script.push({ pointer: drag, pressed: false, released: true, reset: false, viewport: VIEWPORT });
  for (let k = 0; k < 90; k += 1) {
    script.push(idle);
  }

  const run = (): string => {
    const s = new SwipeBasketballSession();
    for (const intent of script) {
      s.advance(intent);
    }
    return JSON.stringify({ balls: s.ballViews(), score: s.score, shots: s.shots });
  };

  assert.equal(run(), run());
});

// ── 8. released ball is physically simulated ──────────────────────────────────

/** Grab rack ball `i`, drag the pointer up by `dyPx`, and release — a real swipe. */
const swipeShot = (session: SwipeBasketballSession, i: number, dxPx: number, dyPx: number): void => {
  const at = projectRackBall(i);
  session.advance({ pointer: at, pressed: true, released: false, reset: false, viewport: VIEWPORT });
  const mid = vec2(at.x + dxPx * 0.5, at.y + dyPx * 0.5);
  const end = vec2(at.x + dxPx, at.y + dyPx);
  session.advance({ pointer: mid, pressed: false, released: false, reset: false, viewport: VIEWPORT });
  session.advance({ pointer: end, pressed: false, released: false, reset: false, viewport: VIEWPORT });
  session.advance({ pointer: end, pressed: false, released: true, reset: false, viewport: VIEWPORT });
};

test("8. after release the ball flies under physics (rises, then drives into the machine)", () => {
  const session = new SwipeBasketballSession();
  const idx = BALL_COUNT - 1;
  swipeShot(session, idx, 0, -160); // a strong upward flick
  const start = session.ballViews()[idx]!.pos;
  // Track the arc's high point and deepest point (robust to a make that recycles).
  let peakY = start.y;
  let deepestZ = start.z;
  for (let k = 0; k < 40; k += 1) {
    session.advance(idle);
    const p = session.ballViews()[idx]!.pos;
    peakY = Math.max(peakY, p.y);
    deepestZ = Math.min(deepestZ, p.z);
  }
  assert.ok(peakY > start.y + 0.2, "the ball rises after an upward swipe");
  assert.ok(deepestZ < start.z - 0.5, "the ball drives well into the machine (−Z, toward the hoop)");
});

test("9. a weak swipe falls short (does not score) and recycles", () => {
  const session = new SwipeBasketballSession();
  swipeShot(session, 0, 0, -24); // a feeble upward flick
  for (let k = 0; k < 360; k += 1) {
    session.advance(idle);
  }
  assert.equal(session.score, 0, "a weak swipe cannot reach the hoop");
  assert.equal(session.ballViews()[0]!.mode, "rack", "the ball settles and recycles (no endless bounce)");
});

// ── 10. arcade score-attack loop ──────────────────────────────────────────────

/** Grab the centre rack ball, drag up ~300px over 10 ticks, release, and let it fly. */
const CENTER_BALL = Math.floor(BALL_COUNT / 2); // the x ≈ 0 ball, aligned with the hoop
const scoreShot = (session: SwipeBasketballSession): void => {
  const at = projectRackBall(CENTER_BALL);
  session.advance({ pointer: at, pressed: true, released: false, reset: false, viewport: VIEWPORT });
  for (let k = 1; k <= 10; k += 1) {
    session.advance({ pointer: vec2(at.x, at.y - 30 * k), pressed: false, released: false, reset: false, viewport: VIEWPORT });
  }
  session.advance({ pointer: vec2(at.x, at.y - 300), pressed: false, released: true, reset: false, viewport: VIEWPORT });
  for (let k = 0; k < 60; k += 1) {
    session.advance(idle);
  }
};

/** Drive `n` makes into a fresh playing round (arcade state only). */
const makes = (n: number, quality: "swish" | "bank" | "rim" = "swish", golden = false) => {
  const state = newRound(0);
  startIfReady(state);
  for (let k = 0; k < n; k += 1) {
    registerMake(state, quality, golden);
  }
  return state;
};

test("10a. swish detection requires no rim or backboard contact", () => {
  assert.equal(classifyShot(false, false), "swish");
  assert.equal(classifyShot(true, false), "rim"); // touched rim → not a swish
  assert.equal(classifyShot(false, true), "bank"); // touched board → not a swish
  assert.equal(basePoints("swish", false), POINTS_SWISH);
});

test("10b. bank detection requires backboard contact before the score", () => {
  assert.equal(classifyShot(false, true), "bank");
  assert.equal(classifyShot(true, true), "bank"); // board wins over rim
  assert.equal(basePoints("bank", false), POINTS_SWISH); // bank and swish are both 3
});

test("10c. a made shot scores exactly once, however it rattles", () => {
  const session = new SwipeBasketballSession();
  scoreShot(session);
  assert.ok(session.score > 0, "the scripted swipe is a make");
  assert.equal(session.streak, 1, "exactly one make is counted");
  const scoreAfter = session.score;
  for (let k = 0; k < 120; k += 1) {
    session.advance(idle);
  }
  assert.equal(session.score, scoreAfter, "the same ball never scores twice");
  assert.equal(session.streak, 1, "still a single make");
});

test("10d. a miss breaks the streak", () => {
  const state = makes(3); // multiplier now 2×, 3 consecutive
  assert.equal(state.multiplier, 2);
  registerMiss(state);
  assert.equal(state.consecutiveMakes, 0, "streak count reset");
  assert.equal(state.multiplier, 1, "multiplier reset to 1×");
});

test("10e. the streak multiplier rises every 3 makes and caps at 4×", () => {
  const state = newRound(0);
  startIfReady(state);
  const seen: number[] = [];
  for (let k = 0; k < 15; k += 1) {
    registerMake(state, "swish", false);
    seen.push(state.multiplier);
  }
  // makes 1,2,3 → …,…,2 ; 4,5,6 → 3 at 6 ; 9 → 4 ; then capped.
  assert.equal(seen[2], 2, "3rd make → 2×");
  assert.equal(seen[5], 3, "6th make → 3×");
  assert.equal(seen[8], STREAK_MULT_CAP, "9th make → 4×");
  assert.equal(seen[14], STREAK_MULT_CAP, "capped at 4×");
});

test("10f. the final 10 seconds doubles the awarded points", () => {
  const normal = newRound(0);
  startIfReady(normal);
  registerMake(normal, "swish", false); // 3 × 1× = 3
  assert.equal(normal.score, POINTS_SWISH);

  const finalState = newRound(0);
  startIfReady(finalState);
  finalState.timeRemaining = FINAL_TICKS; // inside the doubling window
  assert.ok(inFinalWindow(finalState));
  registerMake(finalState, "swish", false); // 3 × 1× × 2 = 6
  assert.equal(finalState.score, POINTS_SWISH * FINAL_MULTIPLIER);
});

test("10g. a golden ball awards golden points and every 5th spawn is golden", () => {
  assert.equal(basePoints("swish", true), POINTS_GOLDEN, "golden trumps quality base");
  const state = newRound(0);
  startIfReady(state);
  registerMake(state, "rim", true); // golden, one make
  assert.equal(state.score, POINTS_GOLDEN);
  assert.equal(isGoldenSpawn(GOLDEN_EVERY), true);
  assert.equal(isGoldenSpawn(GOLDEN_EVERY - 1), false);
});

test("10h. the round timer runs out to game over", () => {
  const state = newRound(0);
  startIfReady(state);
  assert.equal(state.phase, "playing");
  for (let k = 0; k < ROUND_TICKS; k += 1) {
    arcadeTick(state);
  }
  assert.equal(state.phase, "gameover");
  assert.equal(state.timeRemaining, 0);
  registerShot(state); // shots may still be tallied, but time stays out
  arcadeTick(state);
  assert.equal(state.timeRemaining, 0, "the clock never goes negative");
});

test("10i. reset restores score, streak, timer, and the ball rack", () => {
  const session = new SwipeBasketballSession();
  scoreShot(session); // build up some score + a streak, start the clock
  session.advance({ ...idle, reset: true });
  assert.equal(session.score, 0);
  assert.equal(session.streak, 0);
  assert.equal(session.multiplier, 1);
  assert.equal(session.phase, "ready");
  assert.equal(session.timeRemaining, ROUND_TICKS / FIXED_HZ, "clock back to 60 s");
  for (const ball of session.ballViews()) {
    assert.equal(ball.mode, "rack");
  }
});

test("10j. the clock runs out to game over, then a tap restarts", () => {
  const session = new SwipeBasketballSession();
  scoreShot(session); // starts the round (phase → playing) and banks some score
  assert.equal(session.phase, "playing");
  for (let k = 0; k < ROUND_TICKS; k += 1) {
    session.advance(idle);
  }
  assert.equal(session.phase, "gameover");
  assert.ok(session.best > 0, "the best score is banked at game over");
  // A tap (pointer-down edge) on game-over restarts a fresh round.
  session.advance({ pointer: vec2(360, 300), pressed: true, released: false, reset: false, viewport: VIEWPORT });
  assert.equal(session.phase, "ready");
  assert.equal(session.score, 0);
  assert.equal(session.timeRemaining, ROUND_TICKS / FIXED_HZ);
});
