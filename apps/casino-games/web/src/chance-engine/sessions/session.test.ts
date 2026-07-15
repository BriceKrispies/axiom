/*
 * session.test.ts — the session fairness contract: only legal phase
 * transitions; the reveal is sealed behind commitment; an outcome commits
 * exactly once and never rerolls; determinism across identical sessions;
 * the injected source only ever manifests what was supplied; the audit
 * record captures the round faithfully.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { baseConfig } from "../configuration/schema.ts";
import { InjectedChanceResultSource, SeededChanceResultSource } from "../outcomes/result-source.ts";
import { GAME_PHASES, INPUT_LOCKED_PHASES, isLegalTransition } from "./phases.ts";
import { auditOf, commitOutcome, createSession, inputLocked, tickSession, transition } from "./session.ts";

const config = (p = 0.4) => baseConfig("session-test", "Session Test", "showcase", {}, { choiceCount: 9, targetWinRate: p });
const mechanic = { choiceCount: 9, kind: "choice" } as const;

const readySession = (seed = 7) => {
  const source = new SeededChanceResultSource(seed);
  let s = createSession(config(), seed, 1, source, mechanic);
  s = transition(s, "ready");
  return { source, state: s };
};

test("phase transitions follow the legal graph only", () => {
  for (const from of GAME_PHASES) {
    for (const to of GAME_PHASES) {
      const { state } = readySession();
      const at = { ...state, committed: null, phase: from };
      if (!isLegalTransition(from, to)) {
        assert.throws(() => transition(at, to), /illegal phase transition/, `${from} → ${to} must throw`);
      }
    }
  }
});

test("revealing is unreachable without a committed outcome", () => {
  const { state } = readySession();
  const committing = transition(state, "committing");
  assert.throws(() => transition(committing, "revealing"), /without a committed outcome/);
});

test("input is locked exactly in the protected phases", () => {
  assert.deepEqual(INPUT_LOCKED_PHASES, ["committing", "revealing", "resetting"]);
  const { source, state } = readySession();
  assert.equal(inputLocked(state), false);
  const committing = transition(state, "committing");
  assert.equal(inputLocked(committing), true);
  const committed = commitOutcome(committing, source, { selectedIndex: 2 });
  const revealing = transition(committed, "revealing");
  assert.equal(inputLocked(revealing), true);
});

test("an outcome cannot be committed twice and never rerolls", () => {
  const { source, state } = readySession();
  const committing = transition(state, "committing");
  const once = commitOutcome(committing, source, { selectedIndex: 4 });
  assert.notEqual(once.committed, null);
  assert.throws(() => commitOutcome(once, source, { selectedIndex: 5 }), /cannot reroll/);
  // The committed plan is exactly the preassigned population lookup.
  const population = once.mechanicPlan.kind === "choice" ? once.mechanicPlan.population : null;
  assert.ok(population);
  assert.equal(once.committed?.tierId, population.winnersByIndex[4]);
});

test("commitOutcome outside the committing phase throws", () => {
  const { source, state } = readySession();
  assert.throws(() => commitOutcome(state, source, {}), /outside the committing phase/);
});

test("same seed, config, and inputs produce the same outcome plan", () => {
  const runOnce = (): string => {
    const { source, state } = readySession(1234);
    const committed = commitOutcome(transition(state, "committing"), source, { selectedIndex: 3 });
    return JSON.stringify(committed.committed);
  };
  assert.equal(runOnce(), runOnce());
});

test("a fresh session is clean and replay preserves the seed and round", () => {
  const source = new SeededChanceResultSource(99);
  const original = createSession(config(), 99, 5, source, mechanic);
  const replay = createSession(config(), 99, 5, source, mechanic, true);
  assert.equal(replay.seed, original.seed);
  assert.equal(replay.round, original.round);
  assert.equal(replay.phase, "intro");
  assert.equal(replay.committed, null);
  assert.equal(replay.tick, 0);
  assert.equal(replay.replay, true);
  // Same seed + round → identical mechanic plan (the replayed round is the same round).
  assert.deepEqual(replay.mechanicPlan, original.mechanicPlan);
});

test("the audit record captures commitment, result, and stream seeds", () => {
  const { source, state } = readySession(55);
  let s = transition(tickSession(tickSession(state)), "committing");
  s = commitOutcome(s, source, { selectedIndex: 1 });
  s = transition(s, "revealing");
  s = transition(s, "celebrating");
  s = transition(s, "complete");
  const audit = auditOf(s, "seeded");
  assert.equal(audit.gameId, "session-test");
  assert.equal(audit.seedOrRoundId, "55");
  assert.equal(audit.commitPhase, "committing");
  assert.equal(audit.commitTick, 2);
  assert.deepEqual(audit.inputContext, { selectedIndex: 1 });
  assert.equal(typeof audit.win, "boolean");
  assert.equal(Object.keys(audit.streamSeeds).length, 8);
  assert.equal(audit.completePhase, "complete");
});

test("the injected source resolves null until supplied, then only manifests the supplied result", () => {
  const source = new InjectedChanceResultSource();
  let s = createSession(config(), 0, 3, source, mechanic);
  s = transition(s, "ready");
  s = transition(s, "committing");
  const pending = commitOutcome(s, source, { selectedIndex: 2 });
  assert.equal(pending.committed, null, "unresolved commitment leaves the session uncommitted");

  source.supply(3, { presentationSeed: 4242, roundId: "srv-3", tierId: "rare", win: true });
  const resolved = commitOutcome(pending, source, { selectedIndex: 2 });
  assert.equal(resolved.committed?.win, true);
  assert.equal(resolved.committed?.tierId, "rare");
  assert.equal(resolved.committed?.roundId, "srv-3");
  assert.equal(resolved.committed?.presentationSeed, 4242);
  const manifestation = resolved.committed?.manifestation;
  assert.equal(manifestation?.kind, "choice");
  assert.equal(manifestation.kind === "choice" ? manifestation.winnersByIndex[2] : null, "rare");
});
