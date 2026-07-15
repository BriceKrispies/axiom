/*
 * ball-machine.test.ts — the focused fairness proofs for the Ball Machine:
 *  1. the committed plan (win/tier) is fixed BEFORE the extraction animation
 *     begins and is unchanged when the reveal ends;
 *  2. the dispensed ball's chute path is CONTINUOUS — the per-tick displacement
 *     never exceeds a small bound (no teleport / final-frame snap).
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { InputFrame } from "@axiom/web-engine";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { PresentationSettings } from "../../chance-engine/registry/definition.ts";
import { createSession } from "../../chance-engine/sessions/session.ts";
import type { CasinoMountSpec, RoundEnvironment } from "../round-state.ts";
import { foldRoundTick } from "../round-state.ts";
import type { BallExtra, BallMachineSpec, BallState } from "./game.ts";
import { ballTimeline, ballWorldPosition, dispensedIndexOf, initialBallExtra, stepBallMachine } from "./game.ts";

const SETTINGS: PresentationSettings = {
  cameraShake: false,
  highContrast: false,
  masterVolume: 1,
  particleScale: 1,
  reducedMotion: false,
  sfxVolume: 1,
};

const spec: BallMachineSpec = { agitationTicks: 70, ballCount: 14 };

const envFor = (seed: number): RoundEnvironment => ({
  config: baseConfig("ball-machine", "Ball Machine", "machine-interior", spec, { targetWinRate: 0.9 }),
  seed,
  settings: SETTINGS,
  source: new SeededChanceResultSource(seed),
});

const mountSpecFor = (env: RoundEnvironment): CasinoMountSpec<BallExtra> => ({
  initExtra: initialBallExtra,
  mechanic: { kind: "single" },
  resources: { materials: {}, meshes: {} },
  step: (state, input, ctx) =>
    stepBallMachine(
      { config: env.config, onHud: (): void => {}, round: 0, seed: env.seed, settings: env.settings, source: env.source },
      state,
      input,
      ctx,
    ),
  viewScene: (): never => {
    throw new Error("view not used in tests");
  },
});

const frame = (pressPrimary: boolean): InputFrame => ({
  down: new Set<string>(),
  look: { x: 0, y: 0 },
  pointer: undefined,
  pressed: new Set<string>(pressPrimary ? ["primary"] : []),
  released: new Set<string>(),
});

const runToPhase = (env: RoundEnvironment, target: BallState["session"]["phase"], maxTicks = 4000): BallState => {
  const session = createSession(env.config, env.seed, 0, env.source, { kind: "single" });
  let state: BallState = { extra: initialBallExtra(session), pendingContext: null, pendingReset: null, session };
  const mountSpec = mountSpecFor(env);
  for (let tick = 0; tick < maxTicks; tick += 1) {
    const press = state.session.phase === "ready" && tick > 2;
    state = foldRoundTick(env, mountSpec, state, frame(press), { dt: 1 / 60, tick: tick + 1 });
    if (state.session.phase === target) {
      return state;
    }
  }
  throw new Error(`never reached ${target}`);
};

test("the committed plan is fixed before extraction and unchanged at reveal end", () => {
  const env = envFor(1234);
  const atReveal = runToPhase(env, "revealing");
  assert.notEqual(atReveal.session.committed, null, "committed must exist on entering revealing");
  const committedAtStart = atReveal.session.committed;
  const atCelebrating = runToPhase(env, "celebrating");
  assert.deepEqual(atCelebrating.session.committed, committedAtStart, "committed plan must not change during the reveal");
});

test("the dispensed ball's chute path is continuous (no teleport)", () => {
  const env = envFor(777);
  const atReveal = runToPhase(env, "revealing");
  const plan = atReveal.session.committed;
  assert.notEqual(plan, null);
  const seed = (plan as NonNullable<typeof plan>).presentationSeed;
  const dispensed = dispensedIndexOf(spec.ballCount, seed);
  const timeline = ballTimeline(spec, env.config.presentationSpeed, false);

  const startTick = atReveal.session.phaseStartTick;
  let prev = ballWorldPosition(spec, atReveal.session, false, dispensed);
  let maxStep = 0;
  for (let age = 1; age <= timeline.total; age += 1) {
    const at = ballWorldPosition(spec, { ...atReveal.session, tick: startTick + age }, false, dispensed);
    maxStep = Math.max(maxStep, Math.hypot(at.x - prev.x, at.y - prev.y, at.z - prev.z));
    prev = at;
  }
  assert.ok(maxStep < 0.2, `per-tick displacement ${maxStep.toFixed(3)} must stay below 0.2 (continuity)`);
});
