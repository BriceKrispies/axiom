/*
 * agent.test.ts — node:test suite over the axiom-agent-style driver (agent.ts):
 * the observe → decide → emit-intents loop is deterministic, player-equivalent
 * (control codes lower to the same Intent edges a keyboard produces), and the
 * ApexScorerBrain actually scores.
 */

import assert from "node:assert/strict";
import test from "node:test";

import {
  CONTROL_GATHER,
  CONTROL_MOVE_POS_Z,
  CONTROL_PASS_RIGHT,
  FACT_DEFENDER,
  FACT_HOOP,
  FACT_PHASE,
  FACT_SELF_POSE,
  intentOfControlCode,
  observe,
  runAgent,
} from "./agent.ts";
import { Mini3v3Session } from "./session.ts";

test("intentOfControlCode derives press/release edges from the previous mask", () => {
  const press = intentOfControlCode(CONTROL_GATHER, 0);
  assert.equal(press.gatherPressed, true);
  assert.equal(press.gatherHeld, true);
  assert.equal(press.gatherReleased, false);

  const hold = intentOfControlCode(CONTROL_GATHER, CONTROL_GATHER);
  assert.equal(hold.gatherPressed, false);
  assert.equal(hold.gatherHeld, true);

  const release = intentOfControlCode(0, CONTROL_GATHER);
  assert.equal(release.gatherReleased, true);
  assert.equal(release.gatherHeld, false);

  const move = intentOfControlCode(CONTROL_MOVE_POS_Z, 0);
  assert.equal(move.moveZ, 1);
  assert.equal(move.moveX, 0);

  const pass = intentOfControlCode(CONTROL_PASS_RIGHT, CONTROL_PASS_RIGHT);
  assert.equal(pass.passRight, false, "held pass key does not re-fire the edge");
});

test("observe translates the session into the neutral fact vocabulary", () => {
  const session = new Mini3v3Session();
  const obs = observe(session);
  const kinds = obs.facts.map((f) => f.kind);
  assert.ok(kinds.includes(FACT_SELF_POSE));
  assert.ok(kinds.includes(FACT_HOOP));
  assert.ok(kinds.includes(FACT_PHASE));
  assert.equal(obs.facts.filter((f) => f.kind === FACT_DEFENDER).length, 3);
  const self = obs.facts.find((f) => f.kind === FACT_SELF_POSE)!;
  assert.equal(self.z, 4_000_000, "micro-unit fixed point (reset slot z=4)");
});

test("the agent run is deterministic (identical replay hash)", () => {
  const a = runAgent(8);
  const b = runAgent(8);
  assert.equal(a.hash, b.hash);
  assert.equal(a.ticks, b.ticks);
  assert.deepEqual(a.outcomes, b.outcomes);
});

test("the agent scores within 8 possessions, releasing at the apex every time", () => {
  const run = runAgent(8);
  assert.ok(run.makes >= 1, "the agent scored");
  assert.ok(run.outcomes.length <= 8);
  const shots = run.outcomes.filter((o) => o.result === "made" || o.result === "miss");
  assert.ok(shots.length >= 1);
  for (const shot of shots) {
    assert.equal(shot.timing, "perfect", "every agent release is at the jump apex");
  }
  const last = run.outcomes[run.outcomes.length - 1]!;
  assert.equal(last.result, "made", "the run ends on the bucket");
});
