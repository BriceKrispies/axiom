/*
 * round-flow.test.ts — the shared harness fold every game runs on: intro
 * auto-advance, hard input-locking during protected phases, the commitment
 * hand-off, the reset flow producing a genuinely clean session, replay
 * preserving the seed, and outcome-independence from decorative settings
 * (particle density cannot change who wins).
 */

import assert from "node:assert/strict";
import test from "node:test";
import type { InputFrame, TickContext } from "@axiom/web-engine";

import { baseConfig } from "../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../chance-engine/outcomes/result-source.ts";
import type { PresentationSettings } from "../chance-engine/registry/definition.ts";
import { transition } from "../chance-engine/sessions/session.ts";
import type { CasinoMountSpec, CasinoState, RoundEnvironment } from "./round-state.ts";
import { foldRoundTick, freshRoundState, hudOf } from "./round-state.ts";

const SETTINGS: PresentationSettings = {
  cameraShake: true,
  highContrast: false,
  masterVolume: 1,
  particleScale: 1,
  reducedMotion: false,
  sfxVolume: 1,
};

interface ProbeExtra {
  /** Every InputFrame the game's step actually observed, by phase. */
  readonly seen: readonly { readonly phase: string; readonly pressed: readonly string[] }[];
}

/** A minimal probe game: selects index 0 on "primary", finishes its reveal
 * after 30 ticks, and records every input frame its step is shown. */
const probeSpec: CasinoMountSpec<ProbeExtra> = {
  initExtra: () => ({ seen: [] }),
  mechanic: { choiceCount: 9, kind: "choice" },
  resources: { materials: {}, meshes: {} },
  step: (state, input, _ctx) => {
    const seen = [...state.extra.seen, { phase: state.session.phase, pressed: [...input.pressed] }];
    const s: CasinoState<ProbeExtra> = { ...state, extra: { seen } };
    if (state.session.phase === "ready" && input.pressed.has("primary")) {
      return { ...s, pendingContext: { selectedIndex: 0 }, session: transition(state.session, "committing") };
    }
    if (state.session.phase === "revealing" && state.session.tick - state.session.phaseStartTick >= 30) {
      return { ...s, session: transition(state.session, "celebrating") };
    }
    return s;
  },
  viewScene: () => {
    throw new Error("view is never called in these tests");
  },
};

const frame = (...pressed: readonly string[]): InputFrame => ({
  down: new Set(pressed),
  look: { x: 0, y: 0 },
  pointer: undefined,
  pressed: new Set(pressed),
  released: new Set(),
});

const ctx: TickContext = { dt: 1 / 60, tick: 0 };

const environment = (seed: number, settings: PresentationSettings = SETTINGS): RoundEnvironment => ({
  config: baseConfig("flow-test", "Flow Test", "showcase", {}, { choiceCount: 9, targetWinRate: 0.5 }),
  seed,
  settings,
  source: new SeededChanceResultSource(seed),
});

/** Drive the fold until a predicate holds (bounded). The input callback sees
 * the CURRENT state so a test can press keys only in the phases it means to
 * (pressing primary on the complete screen starts a new round by design). */
const runUntil = (
  env: RoundEnvironment,
  state: CasinoState<ProbeExtra>,
  input: (s: CasinoState<ProbeExtra>) => InputFrame,
  done: (s: CasinoState<ProbeExtra>) => boolean,
  limit = 2000,
): CasinoState<ProbeExtra> => {
  let s = state;
  for (let i = 0; i < limit && !done(s); i += 1) {
    s = foldRoundTick(env, probeSpec, s, input(s), ctx);
  }
  assert.ok(done(s), "run did not reach the expected state in time");
  return s;
};

/** Press primary while the round is playable; idle otherwise. */
const primaryInReady = (s: CasinoState<ProbeExtra>): InputFrame => (s.session.phase === "ready" ? frame("primary") : frame());

test("intro auto-advances to ready; primary commits and reveals", () => {
  const env = environment(11);
  let s = freshRoundState(env, probeSpec, 1, false);
  s = runUntil(env, s, primaryInReady, (x) => x.session.phase === "complete");
  assert.notEqual(s.session.committed, null);
  assert.equal(s.session.commitPhase, "committing");
});

test("input is hard-locked during committing and revealing", () => {
  const env = environment(12);
  let s = freshRoundState(env, probeSpec, 1, false);
  s = runUntil(env, s, primaryInReady, (x) => x.session.phase === "complete");
  const lockedSeen = s.extra.seen.filter((entry) => entry.phase === "committing" || entry.phase === "revealing");
  assert.ok(lockedSeen.length > 0, "the probe must have run during locked phases");
  assert.ok(lockedSeen.every((entry) => entry.pressed.length === 0), "no pressed input may leak into locked phases");
});

test("new round resets to a clean session with the next round number", () => {
  const env = environment(13);
  let s = freshRoundState(env, probeSpec, 1, false);
  s = runUntil(env, s, primaryInReady, (x) => x.session.phase === "complete");
  const finishedPlan = s.session.committed;
  s = foldRoundTick(env, probeSpec, s, frame("newRound"), ctx);
  assert.equal(s.session.phase, "resetting");
  s = runUntil(env, s, () => frame(), (x) => x.session.phase === "ready");
  assert.equal(s.session.round, 2);
  assert.equal(s.session.committed, null);
  assert.equal(s.extra.seen.filter((e) => e.phase === "complete").length, 0, "extra state is fresh");
  // A new round is a different draw space: the plan may differ; the seed does not.
  assert.equal(s.session.seed, env.seed);
  assert.notEqual(finishedPlan, null);
});

test("replay same seed reproduces the identical committed outcome", () => {
  const env = environment(14);
  const playRound = (state: CasinoState<ProbeExtra>): CasinoState<ProbeExtra> =>
    runUntil(env, state, primaryInReady, (x) => x.session.phase === "complete");

  let s = playRound(freshRoundState(env, probeSpec, 1, false));
  const first = JSON.stringify(s.session.committed);
  s = foldRoundTick(env, probeSpec, s, frame("replaySeed"), ctx);
  assert.equal(s.session.phase, "resetting");
  s = runUntil(env, s, () => frame(), (x) => x.session.phase === "ready");
  assert.equal(s.session.round, 1, "replay keeps the round number");
  assert.equal(s.session.replay, true);
  s = playRound(s);
  assert.equal(JSON.stringify(s.session.committed), first, "replay must reproduce the outcome bit-for-bit");
});

test("particle density and reduced motion cannot change the outcome", () => {
  const play = (settings: PresentationSettings): string => {
    const env = environment(15, settings);
    const s = runUntil(env, freshRoundState(env, probeSpec, 1, false), primaryInReady, (x) => x.session.phase === "complete");
    return JSON.stringify(s.session.committed);
  };
  const baseline = play(SETTINGS);
  assert.equal(play({ ...SETTINGS, particleScale: 0.35 }), baseline);
  assert.equal(play({ ...SETTINGS, reducedMotion: true }), baseline);
});

test("the hud never exposes the outcome before the reveal", () => {
  const env = environment(16);
  let s = freshRoundState(env, probeSpec, 1, false);
  let sawCommittedPhases = 0;
  for (let i = 0; i < 2000 && s.session.phase !== "complete"; i += 1) {
    s = foldRoundTick(env, probeSpec, s, frame("primary"), ctx);
    const hud = hudOf(probeSpec, "seeded", s);
    const preReveal = s.session.phase === "committing" || s.session.phase === "revealing";
    if (preReveal && s.session.committed !== null) {
      sawCommittedPhases += 1;
      assert.equal(hud.resultText, null, "no result text before celebrating");
      assert.equal(hud.win, null, "no win state before celebrating");
      assert.equal(hud.tierId, null, "no tier before celebrating");
    }
  }
  assert.ok(sawCommittedPhases > 0, "must have observed committed pre-reveal ticks");
});
