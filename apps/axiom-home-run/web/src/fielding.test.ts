/*
 * fielding.test.ts — the defensive force-play model. Deterministic, no wasm/DOM.
 * Proves the baseball force rule + double-play logic and the infield/outfield split.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { vec3 } from "./vec.ts";
import { newRunner } from "./bases.ts";
import { resolveForcePlay } from "./fielding.ts";
import * as C from "./constants.ts";

/** A runner resting on integer base `b`. */
const runnerAt = (b: number) => {
  const r = newRunner(4, 0);
  r.pos = b;
  r.target = b;
  return r;
};

const INFIELD = vec3(2.6, 0, 11); // a routine grounder spot inside the infield
const SHALLOW = vec3(1, 0, 9); // shallow enough to turn two
const OUTFIELD = vec3(0, 0, 20); // a grounder that got through to the outfield

test("a ground ball fielded in the infield forces the batter out at first", () => {
  const play = resolveForcePlay(INFIELD, true, []);
  assert.equal(play.batterOut, true);
  assert.equal(play.outs, 1);
  assert.deepEqual([...play.throwBases], [1], "a direct throw to first");
});

test("a ground ball that reaches the outfield is a hit (safe)", () => {
  const play = resolveForcePlay(OUTFIELD, true, []);
  assert.equal(play.batterOut, false);
  assert.equal(play.outs, 0);
  assert.deepEqual([...play.throwBases], []);
});

test("a ball that is NOT on the ground (a fly/liner that drops) is never a force out", () => {
  const play = resolveForcePlay(INFIELD, false, []);
  assert.equal(play.outs, 0, "fly balls are caught or drop for hits, not force outs");
});

test("a runner on first + a shallow infield grounder is a double play (2nd, relay to 1st)", () => {
  const r1 = runnerAt(1);
  const play = resolveForcePlay(SHALLOW, true, [r1]);
  assert.equal(play.doublePlay, true);
  assert.equal(play.outs, 2);
  assert.equal(play.batterOut, true);
  assert.deepEqual([...play.outRunners], [r1], "the runner from first is the lead force at second");
  assert.deepEqual([...play.throwBases], [2, 1], "lead force at 2nd, relay to 1st");
});

test("a runner on first but a DEEP infield grounder gets only the batter (no time to turn two)", () => {
  const deepInfield = vec3(0, 0, 14.5); // in the infield but too deep for a double play
  const play = resolveForcePlay(deepInfield, true, [runnerAt(1)]);
  assert.equal(play.doublePlay, false);
  assert.equal(play.outs, 1);
  assert.equal(play.batterOut, true);
  assert.deepEqual([...play.throwBases], [1]);
});

test("a runner NOT forced (on second, first empty) is never thrown out", () => {
  const r2 = runnerAt(2);
  const play = resolveForcePlay(SHALLOW, true, [r2]);
  assert.equal(play.batterOut, true, "the batter is still forced at first");
  assert.equal(play.outRunners.length, 0, "the runner on second is not forced");
  assert.deepEqual([...play.throwBases], [1]);
});

test("bases loaded + a shallow infield grounder forces the lead runner home", () => {
  const play = resolveForcePlay(SHALLOW, true, [runnerAt(1), runnerAt(2), runnerAt(3)]);
  assert.equal(play.doublePlay, true);
  assert.equal(play.throwBases[0], 4, "the ball goes home first for the lead force");
  assert.equal(play.outRunners[0]!.pos, 3, "the runner from third is the lead force out");
});

test("the infield/outfield split is a clean boundary", () => {
  assert.equal(resolveForcePlay(vec3(0, 0, C.INFIELD_FORCE_RADIUS - 0.5), true, []).batterOut, true);
  assert.equal(resolveForcePlay(vec3(0, 0, C.INFIELD_FORCE_RADIUS + 0.5), true, []).batterOut, false);
});
