/*
 * sky-drop.test.ts — the whole game core under `node --test`, with no wasm, no DOM and
 * no engine. Everything the balls actually do is decided in SDK-free modules, so a bare
 * Node process can throw a full rack and check the result.
 *
 * Four groups carry more weight than the rest:
 *   - §6 TUNING pins the physics consequences the design rests on: the fall lasts about
 *     as long as intended, every stand is reachable, the wind is worth compensating
 *     without being fatal, and a ball cannot be carried over the target and dropped.
 *   - §7 FRAMING asserts the stand, the target and the whole of arm's reach are on
 *     screen from a camera that never moves — invisible in a screenshot until broken.
 *   - §8 FEEL guards the throw mechanic: the throw comes off the ball's own motion, the
 *     camera never follows a ball, and the next ball is in hand the instant one leaves.
 *   - §9 SILENCE guards the rule that no score is revealed until the rack is down. It
 *     is a design constraint that is easy to erode one convenience at a time.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { type Vec2, type Vec3, vec2, vec3 } from "./vec.ts";
import { hash01, roundConditions } from "./conditions.ts";
import { BallMotion } from "./motion.ts";
import { pointerGrabsBall } from "./selection.ts";
import { type Mat4, project, viewProjection } from "./projection.ts";
import { horizontalDistance, predictLanding, stepBall, terminalSpeed } from "./physics.ts";
import { BANDS, isOnTarget, labelFor, pointsFor, ringFor } from "./target.ts";
import {
  ballsLeft,
  bestLanding,
  bullseyeCount,
  hasBallInHand,
  newRound,
  onTargetCount,
  recordLanding,
  recordThrow,
  settle,
  totalScore,
} from "./round.ts";
import { aimBasis, standView } from "./viewpoint.ts";
import { SkyDropSession } from "./session.ts";
import {
  BALLS_PER_ROUND,
  BALL_RADIUS,
  CAMERA_FAR,
  CAMERA_FOV_Y,
  CAMERA_NEAR,
  DRAG_REACH,
  DROP_ALTITUDE,
  DT,
  GRAVITY,
  GROUND_Y,
  LINEAR_DAMPING,
  MAX_RELEASE_SPEED,
  MOTION_HISTORY,
  RING_BULLSEYE,
  RING_OUTER,
  SEED,
  SETTLE_TICKS,
  STAND_OFFSET_MAX,
  STAND_OFFSET_MIN,
  WIND_ACCEL_MAX,
} from "./constants.ts";

const NO_WIND = vec3(0, 0, 0);
const VIEWPORT = vec2(720, 600);
const ORIGIN = vec3(0, GROUND_Y, 0);
const IDLE = { pointer: null, pressed: false, released: false, reset: false, viewport: VIEWPORT };
const ROUND0 = roundConditions(SEED, 0);

/** A ball simply released from the stand, with no throw at all. */
const deadDrop = (wind = NO_WIND) =>
  predictLanding(vec3(0, DROP_ALTITUDE, 0), NO_WIND, BALL_RADIUS, wind, DT);

/**
 * The view-projection the session uses for a round thrown from `stand`.
 *
 * The session's own matrices are private, and rightly so. This rebuilds them from the
 * same public pieces (`standView` + `viewProjection` + the camera constants) so tests
 * can drive the pointer in WORLD terms — "carry the ball this way at this speed" —
 * rather than hard-coding pixels that would stop meaning anything the moment the
 * framing changed.
 */
const roundViewProj = (stand: Vec3): Mat4 => {
  const view = standView(stand, ORIGIN);
  return viewProjection(
    {
      far: CAMERA_FAR,
      fovY: CAMERA_FOV_Y,
      near: CAMERA_NEAR,
      position: view.position,
      target: view.target,
      up: vec3(0, 1, 0),
    },
    VIEWPORT.x / VIEWPORT.y,
  );
};

/** Where a world point lands on the canvas for a round thrown from `stand`. */
const toScreen = (world: Vec3, stand: Vec3): Vec2 => project(world, roundViewProj(stand), VIEWPORT).pos;

/** Find the launch speed that lands a wind-free throw on the target centre. */
const speedToCentre = (stand: Vec3): number => {
  const basis = aimBasis(stand, ORIGIN);
  let lo = 0;
  let hi = MAX_RELEASE_SPEED;
  for (let i = 0; i < 48; i += 1) {
    const mid = (lo + hi) / 2;
    const velocity = vec3(basis.forward.x * mid, 0, basis.forward.z * mid);
    const landing = predictLanding(stand, velocity, BALL_RADIUS, NO_WIND, DT);
    const overshot =
      (landing.point.x - ORIGIN.x) * basis.forward.x + (landing.point.z - ORIGIN.z) * basis.forward.z > 0;
    hi = overshot ? mid : hi;
    lo = overshot ? lo : mid;
  }
  return (lo + hi) / 2;
};

const intentAt = (world: Vec3, stand: Vec3, pressed: boolean, released: boolean) => ({
  pointer: toScreen(world, stand),
  pressed,
  released,
  reset: false,
  viewport: VIEWPORT,
});

/**
 * Throw one ball: grab it, carry it along `dir` at `speed` m/s, release. The pointer is
 * placed by projecting the intended world position, so this exercises the real grab
 * test, the real unprojection and the real carry — the path the browser drives.
 */
const throwOne = (
  session: SkyDropSession,
  dir: Vec3,
  speed: number,
  carryTicks = 16,
  stand: Vec3 = ROUND0.stand,
): void => {
  const carriedTo = (ticks: number): Vec3 => {
    const d = speed * DT * ticks;
    return vec3(stand.x + dir.x * d, stand.y, stand.z + dir.z * d);
  };
  session.advance(intentAt(stand, stand, true, false));
  for (let i = 1; i <= carryTicks; i += 1) {
    session.advance(intentAt(carriedTo(i), stand, false, false));
  }
  session.advance(intentAt(carriedTo(carryTicks), stand, false, true));
};

/** Throw one ball straight at the target, hard enough to reach it. */
const throwAtTarget = (session: SkyDropSession, stand: Vec3 = ROUND0.stand): void =>
  throwOne(session, aimBasis(stand, ORIGIN).forward, speedToCentre(stand), 16, stand);

/** Idle until the session leaves `phase`, or give up. */
const runUntilLeaves = (session: SkyDropSession, phase: string, limit = 3000): number => {
  let ticks = 0;
  while (session.phase === phase && ticks < limit) {
    session.advance(IDLE);
    ticks += 1;
  }
  return ticks;
};

/** Throw the whole rack at the target and wait for the scoreboard. */
const throwWholeRack = (session: SkyDropSession): void => {
  for (let i = 0; i < BALLS_PER_ROUND; i += 1) {
    throwAtTarget(session);
  }
  runUntilLeaves(session, "settling");
};

// ── §1. deterministic round conditions ────────────────────────────────────────

test("1a. hash01 stays in [0, 1) across many inputs", () => {
  for (let i = 0; i < 400; i += 1) {
    const v = hash01(SEED, i, (i % 7) + 1);
    assert.ok(v >= 0 && v < 1, `hash01 out of range at ${i}: ${v}`);
  }
});

test("1b. conditions are fully determined by (seed, round)", () => {
  for (let i = 0; i < 6; i += 1) {
    assert.deepEqual(roundConditions(SEED, i), roundConditions(SEED, i));
  }
  assert.notDeepEqual(roundConditions(SEED, 0), roundConditions(SEED, 1));
  assert.notDeepEqual(roundConditions(SEED, 0), roundConditions(SEED + 1, 0));
});

test("1c. every stand is at throwing altitude, inside the offset range", () => {
  for (let i = 0; i < 20; i += 1) {
    const conditions = roundConditions(SEED, i);
    assert.equal(conditions.stand.y, DROP_ALTITUDE);
    const distance = horizontalDistance(conditions.stand);
    assert.ok(
      distance >= STAND_OFFSET_MIN - 1e-9 && distance <= STAND_OFFSET_MAX + 1e-9,
      `round ${i} stand ${distance} outside [${STAND_OFFSET_MIN}, ${STAND_OFFSET_MAX}]`,
    );
    assert.ok(Math.abs(distance - conditions.standDistance) < 1e-9);
  }
});

test("1d. wind is horizontal, and windSpeed is the drift the damping converges to", () => {
  for (let i = 0; i < 20; i += 1) {
    const conditions = roundConditions(SEED, i);
    assert.equal(conditions.wind.y, 0);
    const accel = Math.hypot(conditions.wind.x, conditions.wind.z);
    assert.ok(Math.abs(conditions.windSpeed - accel / LINEAR_DAMPING) < 1e-9);
    assert.ok(accel <= WIND_ACCEL_MAX + 1e-9);
  }
});

test("1e. the wind is the same for every ball in a round", () => {
  // The whole skill is reading ONE crosswind across a rack. Re-rolling it per ball
  // would make the grouping unlearnable.
  const session = new SkyDropSession();
  const wind = session.windVector;
  const stand = session.standDistance;
  for (let i = 0; i < 3; i += 1) {
    throwAtTarget(session);
    assert.deepEqual(session.windVector, wind);
    assert.equal(session.standDistance, stand);
  }
});

// ── §2. the throw — reading the ball's own motion ─────────────────────────────

test("2a. motion velocity is metres per SECOND, not per tick", () => {
  const motion = new BallMotion();
  motion.push(vec3(0, 100, 0), 0);
  motion.push(vec3(0.5, 100, 0), 1);
  assert.ok(Math.abs(motion.releaseVelocity(DT).x - 30) < 1e-9);
});

test("2b. a steady carry releases at exactly the speed it was carried", () => {
  const motion = new BallMotion();
  const speed = 22;
  for (let i = 0; i <= 8; i += 1) {
    motion.push(vec3(0, 100, -speed * DT * i), i);
  }
  const v = motion.releaseVelocity(DT);
  assert.ok(Math.abs(v.z + speed) < 1e-6, `expected ${-speed} m/s along z, got ${v.z}`);
  assert.ok(Math.abs(v.x) < 1e-9);
});

test("2c. a ball grabbed and released without moving is simply dropped", () => {
  const motion = new BallMotion();
  motion.push(vec3(3, 100, 4), 0);
  assert.deepEqual(motion.releaseVelocity(DT), vec3(0, 0, 0));
  for (let i = 1; i <= 10; i += 1) {
    motion.push(vec3(3, 100, 4), i);
  }
  const v = motion.releaseVelocity(DT);
  assert.ok(Math.hypot(v.x, v.y, v.z) < 1e-9);
});

test("2d. the motion history is bounded and keeps the most recent samples", () => {
  const motion = new BallMotion();
  for (let i = 0; i < MOTION_HISTORY * 3; i += 1) {
    motion.push(vec3(i, 100, 0), i);
  }
  assert.equal(motion.size, MOTION_HISTORY);
  assert.ok(Math.abs(motion.releaseVelocity(DT).x - 1 / DT) < 1e-9);
  motion.clear();
  assert.equal(motion.size, 0);
});

test("2e. a late stumble cannot cancel a throw that was already moving", () => {
  const motion = new BallMotion();
  for (let i = 0; i <= 6; i += 1) {
    motion.push(vec3(0, 100, -20 * DT * i), i);
  }
  const clean = Math.abs(motion.releaseVelocity(DT).z);
  motion.push(vec3(0, 100, -20 * DT * 6), 7);
  const stumbled = Math.abs(motion.releaseVelocity(DT).z);
  assert.ok(stumbled > 0, "the throw must survive a single stalled sample");
  assert.ok(stumbled > clean * 0.5, `one stalled sample cost too much: ${clean} → ${stumbled}`);
});

test("2f. a ball can be grabbed by touching it, and not from across the screen", () => {
  const viewProj = roundViewProj(ROUND0.stand);
  const onBall = toScreen(ROUND0.stand, ROUND0.stand);
  assert.equal(pointerGrabsBall(onBall, ROUND0.stand, viewProj, VIEWPORT), true);
  assert.equal(
    pointerGrabsBall(vec2(onBall.x + 300, onBall.y + 220), ROUND0.stand, viewProj, VIEWPORT),
    false,
  );
});

// ── §3. physics ───────────────────────────────────────────────────────────────

test("3a. drag bounds the fall speed without dominating the drop", () => {
  let pos = vec3(0, DROP_ALTITUDE, 0);
  let vel = NO_WIND;
  let fastest = 0;
  for (let i = 0; i < 600; i += 1) {
    const step = stepBall(pos, vel, BALL_RADIUS, NO_WIND, DT);
    if (step.contact !== null) {
      break;
    }
    pos = step.pos;
    vel = step.vel;
    fastest = Math.max(fastest, -vel.y);
  }
  const seconds = deadDrop().seconds;
  assert.ok(fastest <= terminalSpeed() + 1e-6, `${fastest} exceeded terminal ${terminalSpeed()}`);
  assert.ok(fastest < Math.abs(GRAVITY) * seconds, "drag is not slowing the fall");
  assert.ok(fastest > terminalSpeed() * 0.4, `the fall never got going: ${fastest}`);
  assert.ok(fastest < terminalSpeed() * 0.8, "the fall is saturating — it could be shorter");
});

test("3b. a ball reports exactly one first ground contact, at ball-radius height", () => {
  const landing = deadDrop();
  assert.ok(Math.abs(landing.point.x) < 1e-9 && Math.abs(landing.point.z) < 1e-9);
  assert.equal(landing.point.y, GROUND_Y);

  let pos = vec3(0, DROP_ALTITUDE, 0);
  let vel = NO_WIND;
  let contacts = 0;
  for (let i = 0; i < Math.round(landing.seconds / DT); i += 1) {
    const step = stepBall(pos, vel, BALL_RADIUS, NO_WIND, DT);
    contacts += step.contact === null ? 0 : 1;
    pos = step.pos;
    vel = step.vel;
  }
  assert.equal(contacts, 1);
  assert.ok(Math.abs(pos.y - (GROUND_Y + BALL_RADIUS)) < 1e-9);
});

test("3c. a landed ball bounces upward, losing energy", () => {
  const step = stepBall(vec3(0, GROUND_Y + BALL_RADIUS * 0.5, 0), vec3(0, -40, 0), BALL_RADIUS, NO_WIND, DT);
  assert.ok(step.contact !== null);
  assert.ok(step.vel.y > 0 && step.vel.y < 40);
});

test("3d. wind pushes a ball downwind, and harder wind pushes further", () => {
  const gentle = deadDrop(vec3(0.4, 0, 0));
  const strong = deadDrop(vec3(WIND_ACCEL_MAX, 0, 0));
  assert.ok(gentle.point.x > 0);
  assert.ok(strong.point.x > gentle.point.x);
  assert.ok(Math.abs(strong.point.z) < 1e-9);
});

// ── §4. the target ────────────────────────────────────────────────────────────

test("4a. each band scores its own points, and boundaries are inclusive", () => {
  for (const band of BANDS) {
    assert.equal(ringFor(band.radius), band.ring);
    assert.equal(pointsFor(band.radius), band.points);
    assert.equal(labelFor(band.radius), band.label);
  }
});

test("4b. bands ascend, so the tightest match always wins", () => {
  for (let i = 1; i < BANDS.length; i += 1) {
    assert.ok(BANDS[i]!.radius > BANDS[i - 1]!.radius);
    assert.ok(BANDS[i]!.points < BANDS[i - 1]!.points);
  }
  assert.equal(ringFor(0), "dead-centre");
});

test("4c. landing past the outer ring scores nothing", () => {
  assert.equal(ringFor(RING_OUTER + 0.001), "off");
  assert.equal(pointsFor(RING_OUTER + 0.001), 0);
  assert.equal(labelFor(999), "OFF TARGET");
  assert.equal(isOnTarget(RING_OUTER + 0.001), false);
  assert.equal(isOnTarget(RING_OUTER), true);
});

// ── §5. the round ─────────────────────────────────────────────────────────────

test("5a. the rack empties one throw at a time", () => {
  const state = newRound(0);
  assert.equal(ballsLeft(state), BALLS_PER_ROUND);
  assert.equal(hasBallInHand(state), true);
  for (let i = 0; i < BALLS_PER_ROUND; i += 1) {
    recordThrow(state);
  }
  assert.equal(ballsLeft(state), 0);
  assert.equal(hasBallInHand(state), false);
  assert.equal(state.phase, "settling");
});

test("5b. landings accumulate and total up", () => {
  const state = newRound(0);
  recordLanding(state, 0, 0);
  recordLanding(state, 1, RING_BULLSEYE);
  recordLanding(state, 2, RING_OUTER + 5);
  assert.equal(state.landed, 3);
  assert.equal(totalScore(state), pointsFor(0) + pointsFor(RING_BULLSEYE));
  assert.equal(bullseyeCount(state), 2);
  assert.equal(onTargetCount(state), 2);
  assert.equal(bestLanding(state), 0);
});

test("5c. the scoreboard waits for every ball to be thrown AND land", () => {
  const state = newRound(0);
  for (let i = 0; i < BALLS_PER_ROUND; i += 1) {
    recordThrow(state);
  }
  // All thrown, but still in the air: settling must not advance.
  for (let i = 0; i < SETTLE_TICKS * 2; i += 1) {
    assert.equal(settle(state), false);
  }
  assert.equal(state.phase, "settling");

  for (let i = 0; i < BALLS_PER_ROUND; i += 1) {
    recordLanding(state, i, 1);
  }
  for (let i = 0; i < SETTLE_TICKS - 1; i += 1) {
    assert.equal(settle(state), false);
  }
  assert.equal(settle(state), true);
  assert.equal(state.phase, "results");
  assert.equal(state.best, totalScore(state));
});

// ── §6. tuning — the numbers the design rests on ──────────────────────────────

test("6a. the fall is long enough to watch and short enough to sit through", () => {
  const seconds = deadDrop().seconds;
  assert.ok(seconds > 2.2 && seconds < 3.4, `fall took ${seconds}s, expected ~2.8s`);
});

test("6b. every stand is reachable, with headroom left for the wind", () => {
  for (const distance of [STAND_OFFSET_MIN, (STAND_OFFSET_MIN + STAND_OFFSET_MAX) / 2, STAND_OFFSET_MAX]) {
    const stand = vec3(distance, DROP_ALTITUDE, 0);
    const needed = speedToCentre(stand);
    assert.ok(needed < MAX_RELEASE_SPEED * 0.75, `stand ${distance} m needs ${needed} m/s`);

    const basis = aimBasis(stand, ORIGIN);
    const velocity = vec3(basis.forward.x * needed, 0, basis.forward.z * needed);
    const landing = predictLanding(stand, velocity, BALL_RADIUS, NO_WIND, DT);
    assert.ok(horizontalDistance(landing.point) < RING_BULLSEYE);
  }
});

test("6c. maximum wind is worth compensating, but not an automatic miss", () => {
  const drift = horizontalDistance(deadDrop(vec3(WIND_ACCEL_MAX, 0, 0)).point);
  assert.ok(drift > RING_BULLSEYE * 2, `max wind drift ${drift} m is too weak to matter`);
  assert.ok(drift < RING_OUTER, `max wind drift ${drift} m blows an ignored ball clean off`);
});

test("6d. a ball simply released always misses, so every throw must be a throw", () => {
  for (let i = 0; i < 20; i += 1) {
    const conditions = roundConditions(SEED, i);
    const landing = predictLanding(conditions.stand, NO_WIND, BALL_RADIUS, conditions.wind, DT);
    assert.equal(isOnTarget(horizontalDistance(landing.point)), false, `round ${i} scores unthrown`);
  }
});

test("6e. a ball can never simply be carried over the target and let go", () => {
  assert.ok(
    DRAG_REACH < STAND_OFFSET_MIN - RING_OUTER,
    `reach ${DRAG_REACH} m from a ${STAND_OFFSET_MIN} m stand can touch the ${RING_OUTER} m target`,
  );
});

// ── §7. framing — from a camera that never moves ──────────────────────────────

test("7a. the aim basis is orthonormal and points at the target", () => {
  const stand = vec3(40, DROP_ALTITUDE, -25);
  const { forward, right } = aimBasis(stand, ORIGIN);
  assert.ok(Math.abs(Math.hypot(forward.x, forward.z) - 1) < 1e-9);
  assert.ok(Math.abs(Math.hypot(right.x, right.z) - 1) < 1e-9);
  assert.ok(Math.abs(forward.x * right.x + forward.z * right.z) < 1e-9);
  assert.ok(horizontalDistance(vec3(stand.x + forward.x, stand.y, stand.z + forward.z)) < horizontalDistance(stand));
});

test("7b. a stand directly over the target still yields a usable basis", () => {
  const { forward, right } = aimBasis(vec3(0, DROP_ALTITUDE, 0), ORIGIN);
  assert.ok(Math.abs(Math.hypot(forward.x, forward.z) - 1) < 1e-9);
  assert.ok(Math.abs(Math.hypot(right.x, right.z) - 1) < 1e-9);
});

test("7c. BOTH the stand and the target are on screen, for every round", () => {
  const halfFov = CAMERA_FOV_Y / 2;
  const angleFromAxis = (from: Vec3, to: Vec3, axis: Vec3): number => {
    const d = vec3(to.x - from.x, to.y - from.y, to.z - from.z);
    const len = Math.hypot(d.x, d.y, d.z);
    const dot = (d.x * axis.x + d.y * axis.y + d.z * axis.z) / len;
    return Math.acos(Math.min(1, Math.max(-1, dot)));
  };

  for (let i = 0; i < 20; i += 1) {
    const stand = roundConditions(SEED, i).stand;
    const view = standView(stand, ORIGIN);
    const a = vec3(
      view.target.x - view.position.x,
      view.target.y - view.position.y,
      view.target.z - view.position.z,
    );
    const len = Math.hypot(a.x, a.y, a.z);
    const axis = vec3(a.x / len, a.y / len, a.z / len);
    assert.ok(angleFromAxis(view.position, stand, axis) < halfFov, `round ${i}: stand off screen`);
    assert.ok(angleFromAxis(view.position, ORIGIN, axis) < halfFov, `round ${i}: target off screen`);
  }
});

test("7d. the whole reach stays on screen, so a ball is never carried out of frame", () => {
  for (let i = 0; i < 20; i += 1) {
    const stand = roundConditions(SEED, i).stand;
    for (const [dx, dz] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      const rim = vec3(stand.x + dx! * DRAG_REACH, stand.y, stand.z + dz! * DRAG_REACH);
      const at = toScreen(rim, stand);
      assert.ok(
        at.x > 0 && at.x < VIEWPORT.x && at.y > 0 && at.y < VIEWPORT.y,
        `round ${i}: reach rim projects off-canvas at ${at.x},${at.y}`,
      );
    }
  }
});

test("7e. the camera sits above the ground and looks down at the target", () => {
  for (let i = 0; i < 20; i += 1) {
    const stand = roundConditions(SEED, i).stand;
    const view = standView(stand, ORIGIN);
    assert.ok(view.position.y > GROUND_Y);
    assert.ok(view.position.y > view.target.y, "the camera must look downward");
  }
});

// ── §8. feel — the throw mechanic's own invariants ────────────────────────────

test("8a. a new session starts with a full rack, in hand, off the target", () => {
  const session = new SkyDropSession();
  assert.equal(session.phase, "throwing");
  assert.equal(session.ballsLeft, BALLS_PER_ROUND);
  assert.equal(session.ballNumber, 1);
  assert.equal(session.inFlight, 0);
  assert.ok(session.standDistance >= STAND_OFFSET_MIN);
  assert.equal(session.readyBall()?.state, "ready");
});

test("8b. pressing away from the ball does not pick it up", () => {
  const session = new SkyDropSession();
  const onBall = toScreen(ROUND0.stand, ROUND0.stand);
  session.advance({
    pointer: vec2(onBall.x + 320, onBall.y + 230),
    pressed: true,
    released: false,
    reset: false,
    viewport: VIEWPORT,
  });
  assert.equal(session.holding, false);
  assert.equal(session.ballsLeft, BALLS_PER_ROUND);
});

test("8c. a held ball follows the finger, and an untouched one hangs still", () => {
  const session = new SkyDropSession();
  const resting = session.readyBall()!.pos;
  for (let i = 0; i < 20; i += 1) {
    session.advance(IDLE);
  }
  assert.deepEqual(session.readyBall()!.pos, resting);

  const basis = aimBasis(ROUND0.stand, ORIGIN);
  session.advance(intentAt(ROUND0.stand, ROUND0.stand, true, false));
  assert.equal(session.holding, true);
  for (let i = 1; i <= 10; i += 1) {
    const d = 0.4 * i;
    session.advance(
      intentAt(vec3(ROUND0.stand.x + basis.forward.x * d, ROUND0.stand.y, ROUND0.stand.z + basis.forward.z * d), ROUND0.stand, false, false),
    );
  }
  const carried = session.readyBall()!.pos;
  assert.ok(Math.hypot(carried.x - resting.x, carried.z - resting.z) > 1);
  assert.ok(Math.abs(carried.y - ROUND0.stand.y) < 1e-6, "the carry stays on a level plane");
});

test("8d. the throw comes off the ball's own motion — carry harder, launch faster", () => {
  const forward = aimBasis(ROUND0.stand, ORIGIN).forward;

  const slow = new SkyDropSession();
  throwOne(slow, forward, 8);
  const gentle = slow.ballViews()[0]!;

  const fast = new SkyDropSession();
  throwOne(fast, forward, 26);

  // Step both the same number of ticks and compare how far each has travelled.
  for (let i = 0; i < 20; i += 1) {
    slow.advance(IDLE);
    fast.advance(IDLE);
  }
  const slowTravel = horizontalDistance(slow.ballViews()[0]!.pos) ;
  const fastTravel = horizontalDistance(fast.ballViews()[0]!.pos);
  assert.ok(gentle.state === "flying" || gentle.state === "down");
  assert.ok(
    fastTravel < slowTravel,
    `the harder throw must be further along toward the target (${fastTravel} vs ${slowTravel})`,
  );
});

test("8e. the camera never moves — not while carrying, not while balls fall", () => {
  // The reason `viewpoint.ts` exists. A chase camera would hold a falling ball at
  // constant size and yank the frame off the ball still in hand.
  const session = new SkyDropSession();
  const before = session.camera;

  const basis = aimBasis(ROUND0.stand, ORIGIN);
  session.advance(intentAt(ROUND0.stand, ROUND0.stand, true, false));
  for (let i = 1; i <= 12; i += 1) {
    const d = 0.8 * i;
    session.advance(
      intentAt(vec3(ROUND0.stand.x + basis.forward.x * d, ROUND0.stand.y, ROUND0.stand.z + basis.forward.z * d), ROUND0.stand, false, false),
    );
  }
  assert.deepEqual(session.camera, before, "carrying must not move the camera");

  throwAtTarget(session);
  for (let i = 0; i < 120; i += 1) {
    session.advance(IDLE);
  }
  assert.deepEqual(session.camera, before, "a falling ball must not move the camera");
});

test("8f. a ball cannot be carried beyond arm's reach", () => {
  const session = new SkyDropSession();
  const basis = aimBasis(ROUND0.stand, ORIGIN);
  session.advance(intentAt(ROUND0.stand, ROUND0.stand, true, false));
  for (let i = 0; i < 60; i += 1) {
    const far = vec3(ROUND0.stand.x + basis.forward.x * 400, ROUND0.stand.y, ROUND0.stand.z + basis.forward.z * 400);
    session.advance(intentAt(far, ROUND0.stand, false, false));
  }
  const held = session.readyBall()!.pos;
  const carried = Math.hypot(held.x - ROUND0.stand.x, held.z - ROUND0.stand.z);
  assert.ok(carried <= DRAG_REACH + 1e-6, `carried ${carried} m past the ${DRAG_REACH} m reach`);
  assert.ok(horizontalDistance(held) > RING_OUTER, "and still cannot be held over the target");
});

test("8g. the next ball is in hand the instant one leaves it", () => {
  const session = new SkyDropSession();
  throwAtTarget(session);
  assert.equal(session.ballNumber, 2, "the rack advances immediately");
  assert.equal(session.ballsLeft, BALLS_PER_ROUND - 1);
  assert.equal(session.readyBall()?.state, "ready", "a fresh ball is waiting at the stand");
  assert.equal(session.inFlight, 1, "and the thrown one is still in the air");

  // It is grabbable straight away — no wait for the previous ball to land.
  session.advance(intentAt(ROUND0.stand, ROUND0.stand, true, false));
  assert.equal(session.holding, true);
});

test("8h. several balls are airborne at once, each on its own trajectory", () => {
  const session = new SkyDropSession();
  const forward = aimBasis(ROUND0.stand, ORIGIN).forward;
  throwOne(session, forward, 10);
  throwOne(session, forward, 20);
  throwOne(session, forward, 30);
  assert.equal(session.inFlight, 3, "three balls should be falling together");

  const flying = session.ballViews().filter((ball) => ball.state === "flying");
  const spread = new Set(flying.map((ball) => horizontalDistance(ball.pos).toFixed(3)));
  assert.equal(spread.size, 3, "each ball must carry its own throw, not a shared one");
});

test("8i. landed balls stay on the ground for the rest of the round", () => {
  // The only feedback before the scoreboard: your grouping, visible where it fell.
  const session = new SkyDropSession();
  throwAtTarget(session);
  runUntilLeaves(session, "throwing", 400);
  const down = session.ballViews().filter((ball) => ball.state === "down");
  assert.ok(down.length >= 1, "a landed ball must remain in the scene");
  assert.ok(down.every((ball) => ball.pos.y <= BALL_RADIUS + 1e-6), "and must be resting on the ground");
});

// ── §9. silence — nothing is scored out loud until the rack is down ───────────

test("9a. the phase stays 'throwing' no matter how many balls have already landed", () => {
  const session = new SkyDropSession();
  throwAtTarget(session);
  runUntilLeaves(session, "throwing", 400);
  assert.equal(session.phase, "throwing", "a landing must not interrupt the rack");
  assert.ok(session.ballsLeft > 0);
});

test("9b. the scoreboard appears only once every ball is thrown and down", () => {
  const session = new SkyDropSession();
  for (let i = 0; i < BALLS_PER_ROUND - 1; i += 1) {
    throwAtTarget(session);
  }
  // Let everything thrown so far land, with one ball still in hand.
  for (let i = 0; i < 400; i += 1) {
    session.advance(IDLE);
  }
  assert.equal(session.phase, "throwing", "a ball still in the rack keeps the round open");

  throwAtTarget(session);
  assert.equal(session.phase, "settling", "the last throw closes the rack");
  runUntilLeaves(session, "settling");
  assert.equal(session.phase, "results");
});

test("9c. every throw is scored, and the total is the sum of them", () => {
  const session = new SkyDropSession();
  throwWholeRack(session);
  assert.equal(session.results.length, BALLS_PER_ROUND, "every ball gets a verdict");
  const summed = session.results.reduce((sum, result) => sum + result.points, 0);
  assert.equal(session.score, summed);
  assert.equal(session.best, session.score);
});

test("9d. a fresh rack resets the round but keeps the best score", () => {
  const session = new SkyDropSession();
  throwWholeRack(session);
  const earned = session.score;
  assert.ok(earned > 0);

  session.advance({ pointer: null, pressed: false, released: false, reset: true, viewport: VIEWPORT });
  assert.equal(session.phase, "throwing");
  assert.equal(session.score, 0);
  assert.equal(session.results.length, 0);
  assert.equal(session.ballsLeft, BALLS_PER_ROUND);
  assert.equal(session.best, earned);
});

test("9e. a tap on the scoreboard throws a fresh rack", () => {
  const session = new SkyDropSession();
  throwWholeRack(session);
  assert.equal(session.phase, "results");
  session.advance({ pointer: vec2(360, 300), pressed: true, released: false, reset: false, viewport: VIEWPORT });
  assert.equal(session.phase, "throwing");
  assert.equal(session.ballNumber, 1);
});

test("9f. a new rack is a new round — new stand, new wind", () => {
  const session = new SkyDropSession();
  const firstStand = session.standDistance;
  const firstWind = session.windVector;
  throwWholeRack(session);
  session.advance({ pointer: null, pressed: false, released: false, reset: true, viewport: VIEWPORT });
  const changed =
    session.standDistance !== firstStand ||
    session.windVector.x !== firstWind.x ||
    session.windVector.z !== firstWind.z;
  assert.ok(changed, "consecutive rounds must not present identical conditions");
});
