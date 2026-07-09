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
import { swipeToThrow } from "./throw.ts";
import { pickBall } from "./selection.ts";
import { scoredThroughHoop } from "./scoring.ts";
import { type Intent, SwipeBasketballSession, rackPositions } from "./session.ts";
import {
  BALL_COUNT,
  CAMERA_FAR,
  CAMERA_FOV_Y,
  CAMERA_NEAR,
  CAMERA_POS,
  CAMERA_TARGET,
  HOOP_X,
  HOOP_Y,
  HOOP_Z,
  MAX_POINTER_DELTA,
  POINTER_HISTORY,
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

// ── 2. swipe → throw mapping ──────────────────────────────────────────────────

test("2. an upward swipe lifts and carries forward (−Z)", () => {
  const throwVel = swipeToThrow(vec2(0, -40));
  assert.ok(throwVel.y > 0, "upward swipe gives positive lift");
  assert.ok(throwVel.z < 0, "throw goes into the machine (−Z)");
});

test("2b. a harder flick throws farther forward", () => {
  const soft = swipeToThrow(vec2(0, -20));
  const hard = swipeToThrow(vec2(0, -80));
  assert.ok(hard.z < soft.z, "harder upward flick has more forward speed");
  assert.ok(hard.y > soft.y, "harder upward flick has more lift");
});

test("2c. horizontal swipe steers X, a downward swipe gives no lift", () => {
  assert.ok(swipeToThrow(vec2(40, 0)).x > 0, "rightward swipe → +X");
  assert.ok(swipeToThrow(vec2(-40, 0)).x < 0, "leftward swipe → −X");
  assert.equal(swipeToThrow(vec2(0, 50)).y, 0, "a downward flick contributes no lift");
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

test("8. after release the ball flies under physics (up then falling, toward the hoop)", () => {
  const session = new SwipeBasketballSession();
  const idx = BALL_COUNT - 1;
  swipeShot(session, idx, 0, -160); // a strong upward flick
  const start = session.ballViews()[idx]!.pos;
  let peakY = start.y;
  for (let k = 0; k < 30; k += 1) {
    session.advance(idle);
    peakY = Math.max(peakY, session.ballViews()[idx]!.pos.y);
  }
  const end = session.ballViews()[idx]!.pos;
  assert.ok(peakY > start.y + 0.15, "the ball rises after an upward swipe");
  assert.ok(end.z < start.z, "the ball travels into the machine (−Z, toward the hoop)");
});
