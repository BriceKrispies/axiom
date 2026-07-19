/*
 * base-running.test.ts — the base-running model and its session integration,
 * exercised with `node --test` (native TS type-stripping, no wasm/DOM/SDK) exactly
 * like `home-run.test.ts`. It proves: how a resolved hit maps to bases earned; the
 * pure runner advancement around the diamond (position, facing, scoring); and the
 * end-to-end session behavior — a homer scores a run and clears the bases, a
 * shallower hit leaves a persistent runner on base, and everything replays from the
 * seed.
 */

import assert from "node:assert/strict";
import test from "node:test";

import type { Intent, Outcome } from "./types.ts";
import { advanceRunner, basesForHit, newRunner, runnerFacing, runnerMoving, runnerWorld } from "./bases.ts";
import { HomeRunSession } from "./session.ts";
import * as C from "./constants.ts";

const IDLE: Intent = { moveX: 0, start: false, swing: false };

/** Play one pitch on `seed`, pressing swing at `swingTick` (optionally stepping the
 * batter first), then run until it resolves; returns the session mid-result (before
 * the runners have necessarily finished their trot). */
const playFirstPitch = (seed: number, swingTick: number, moveX = 0, moveTicks = 0): HomeRunSession => {
  const s = new HomeRunSession(seed);
  for (let t = 1; t < swingTick; t += 1) {
    s.advance({ moveX: t <= moveTicks ? moveX : 0, start: t === 1, swing: false });
  }
  s.advance({ moveX: 0, start: false, swing: true });
  let guard = 1400;
  while (s.results.length === 0 && guard > 0) {
    s.advance(IDLE);
    guard -= 1;
  }
  assert.ok(guard > 0, "the pitch must resolve");
  return s;
};

/** Run the session forward until it is ready for the next pitch (runners settled). */
const finishPlay = (s: HomeRunSession): void => {
  let guard = 1200;
  while (s.phase === "result" && guard > 0) {
    s.advance(IDLE);
    guard -= 1;
  }
  assert.ok(guard > 0, "the result phase (incl. base running) must complete");
};

/** Scan swing timings on `seed` for one whose first pitch's outcome matches. */
const findSwingFor = (seed: number, want: (o: Outcome, caught: boolean) => boolean): HomeRunSession | null => {
  for (const [move, mt] of [[0, 0], [-1, 12], [1, 12], [-1, 20], [1, 20]] as const) {
    for (let t = 28; t <= 155; t += 1) {
      const s = playFirstPitch(seed, t, move, mt);
      const r = s.results[0]!;
      if (want(r.outcome, r.caught)) {
        return s;
      }
    }
  }
  return null;
};

// ── bases earned ─────────────────────────────────────────────────────────────

test("bases earned: homer clears, fly-out is nothing, deeper hits earn more", () => {
  assert.equal(basesForHit("homer", 40, false), 4);
  assert.equal(basesForHit("clean", 40, true), 0, "a ball caught on the fly is an out");
  assert.equal(basesForHit("clean", C.TRIPLE_DIST + 2, false), 3);
  assert.equal(basesForHit("clean", C.DOUBLE_DIST + 2, false), 2);
  assert.equal(basesForHit("weak", 6, false), 1, "any fair non-caught ball is at least a single");
  assert.equal(basesForHit("grounder", 8, false), 1, "a grounder fielded (not caught) is a single");
  for (const o of ["foul", "ball", "miss"] as const) {
    assert.equal(basesForHit(o, 12, false), 0, `${o} is not in play`);
  }
});

// ── runner motion ────────────────────────────────────────────────────────────

test("runnerWorld maps base indices onto the painted bases (4 wraps to home)", () => {
  const at = (pos: number) => runnerWorld({ lane: 0, pos, scored: false, target: pos, traveled: 0 });
  assert.deepEqual(at(0), C.BASE_POINTS[0], "home");
  assert.deepEqual(at(1), C.BASE_POINTS[1], "1B");
  assert.deepEqual(at(2), C.BASE_POINTS[2], "2B");
  assert.deepEqual(at(3), C.BASE_POINTS[3], "3B");
  assert.deepEqual(at(4), C.BASE_POINTS[0], "home again");
  // Halfway from home to 1B is on the line between them.
  const half = at(0.5);
  assert.ok(Math.abs(half.x - (C.BASE_POINTS[0]!.x + C.BASE_POINTS[1]!.x) / 2) < 1e-9);
});

test("a runner advances to its target base and no further", () => {
  const r = newRunner(2, 0); // a double: home → 2B
  assert.ok(runnerMoving(r));
  let guard = 1000;
  while (runnerMoving(r) && guard > 0) {
    advanceRunner(r);
    guard -= 1;
  }
  assert.ok(guard > 0, "the runner reaches 2B in bounded time");
  assert.equal(r.pos, 2, "stops exactly on 2B");
  assert.equal(r.scored, false, "a double does not score");
  assert.ok(!runnerMoving(r));
});

test("a runner reaching home scores exactly once", () => {
  const r = newRunner(4, 0); // an inside-the-park / homer distance: home → home
  let scores = 0;
  let guard = 2000;
  while (guard > 0 && !r.scored) {
    scores += advanceRunner(r).justScored ? 1 : 0;
    guard -= 1;
  }
  assert.equal(scores, 1, "scored is flagged exactly once");
  assert.ok(r.scored);
  assert.ok(r.pos >= 4 - 1e-6);
});

test("a runner faces along the base leg it is running", () => {
  // Home → 1B heads toward (-x,+z): facing yaw = atan2(dir.x, dir.z), dir ≈ (-1,0,1).
  const facing = runnerFacing(newRunner(1, 0));
  assert.ok(Math.abs(facing - Math.atan2(-1, 1)) < 1e-6);
});

// ── session integration ──────────────────────────────────────────────────────

test("a home run scores one run, clears the bases, and gains four bases", () => {
  const s = findSwingFor(1, (o) => o === "homer");
  assert.ok(s !== null, "some swing on seed 1 clears the wall");
  finishPlay(s);
  assert.equal(s.runnersHome, 1, "the batter scores");
  assert.equal(s.runnersOnBase, 0, "nobody is left on base");
  assert.equal(s.basesGained, 4, "four bases gained");
});

test("a whiff (not in play) puts no runner on base and gains nothing", () => {
  // Swing early: the bat sweeps long before the ball arrives → a MISS.
  const s = playFirstPitch(1, 5);
  assert.equal(s.results[0]!.outcome, "miss");
  finishPlay(s);
  assert.equal(s.runnersOnBase, 0, "a whiff creates no runner");
  assert.equal(s.runnersHome, 0);
  assert.equal(s.basesGained, 0);
});

test("a hit that stays in the park leaves a persistent runner on base", () => {
  // A fair ball that is NOT a homer and NOT caught on the fly → a runner on base.
  const s = findSwingFor(1, (o, caught) => !caught && (o === "clean" || o === "weak" || o === "grounder" || o === "popup"));
  assert.ok(s !== null, "seed 1 produces an in-park hit at some timing");
  finishPlay(s);
  assert.ok(s.runnersOnBase >= 1, "the batter is on base");
  assert.equal(s.runnersHome, 0, "a single/double/triple does not itself score");
  assert.ok(s.basesGained >= 1);
  // The runner PERSISTS into the next pitch (still on base after the pitch begins).
  let guard = 400;
  while (s.phase !== "pitch" && s.phase !== "windup" && guard > 0) {
    s.advance(IDLE);
    guard -= 1;
  }
  assert.ok(s.runnersOnBase >= 1, "the runner is still on base for the next pitch");
});

test("base-running state replays bit-for-bit from the seed", () => {
  const run = (): string => {
    const s = findSwingFor(1, (o) => o === "homer");
    assert.ok(s !== null);
    finishPlay(s);
    return JSON.stringify([s.basesGained, s.runnersHome, s.runnersOnBase, s.score]);
  };
  assert.equal(run(), run());
});
