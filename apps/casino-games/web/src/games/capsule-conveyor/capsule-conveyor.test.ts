/*
 * capsule-conveyor.test.ts — the focused fairness proofs for Capsule Conveyor:
 *  1. the commitment context's stopPosition equals the capsule nearest the
 *     station at the stop tick;
 *  2. belt progress during deceleration is CONTINUOUS (bounded per-tick delta —
 *     no snap);
 *  3. the capsule that opens IS `plan.manifestation.destinationIndex`.
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import type { InputFrame } from "@axiom/web-engine";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { PresentationSettings } from "../../chance-engine/registry/definition.ts";
import { commitOutcome, createSession, transition } from "../../chance-engine/sessions/session.ts";
import type { CapsuleConveyorSpec, ConveyorState } from "./game.ts";
import {
  BELT_SPEED,
  beltProgress,
  conveyorTimeline,
  destinationIndexOf,
  initialConveyorExtra,
  nearestCapsuleToStation,
  openingCapsuleIndex,
  slotsOf,
  stepCapsuleConveyor,
} from "./game.ts";

const SETTINGS: PresentationSettings = {
  cameraShake: false,
  highContrast: false,
  masterVolume: 1,
  particleScale: 1,
  reducedMotion: false,
  sfxVolume: 1,
};

const spec: CapsuleConveyorSpec = {
  capsuleCount: 8,
  capsuleTiers: ["common", null, "uncommon", null, "common", "rare", null, "jackpot"],
};

const runtimeFor = (seed: number, winRate: number) => ({
  config: baseConfig("capsule-conveyor", "Capsule Conveyor", "machine-interior", spec, { targetWinRate: winRate }),
  onHud: (): void => {},
  round: 0,
  seed,
  settings: SETTINGS,
  source: new SeededChanceResultSource(seed),
});

const frame = (over: Partial<InputFrame> = {}): InputFrame => ({
  down: new Set<string>(),
  look: { x: 0, y: 0 },
  pointer: undefined,
  pressed: new Set<string>(),
  released: new Set<string>(),
  ...over,
});

/** An interacting state whose belt has been running to `stopTick`. */
const interactingAt = (runtime: ReturnType<typeof runtimeFor>, stopTick: number): ConveyorState => {
  const base = createSession(runtime.config, runtime.seed, 0, runtime.source, { kind: "destination", slots: slotsOf(spec) });
  const session = { ...transition(transition(base, "ready"), "interacting"), tick: stopTick };
  return { extra: initialConveyorExtra(session), pendingContext: null, pendingReset: null, session };
};

test("stop commits the capsule nearest the station", () => {
  const runtime = runtimeFor(9001, 0.5);
  for (const stopTick of [17, 44, 90, 133, 251]) {
    const state = interactingAt(runtime, stopTick);
    const stopped = stepCapsuleConveyor(runtime, state, frame({ pressed: new Set(["primary"]) }), { dt: 1 / 60, tick: stopTick });
    assert.equal(stopped.session.phase, "committing");
    const expected = nearestCapsuleToStation(spec.capsuleCount, stopTick * BELT_SPEED);
    assert.equal(stopped.pendingContext?.stopPosition, expected, `stopPosition at tick ${stopTick} must be ${expected}`);
  }
});

test("deceleration is continuous and lands the committed capsule at the station", () => {
  for (let seed = 1; seed <= 30; seed += 1) {
    const runtime = runtimeFor(seed, 0.5);
    const stopTick = 60 + seed * 7;
    const state = interactingAt(runtime, stopTick);
    const stopped = stepCapsuleConveyor(runtime, state, frame({ pressed: new Set(["primary"]) }), { dt: 1 / 60, tick: stopTick });
    const committed = commitOutcome(stopped.session, runtime.source, stopped.pendingContext ?? {});
    const revealSession = transition(committed, "revealing");
    const revealState: ConveyorState = { ...stopped, session: revealSession };
    const timeline = conveyorTimeline(runtime.config.presentationSpeed, false);

    // Continuity: per-tick belt-progress delta stays small across the braking.
    let prev = beltProgress(spec, { ...revealState, session: { ...revealSession, tick: revealSession.phaseStartTick } }, false);
    let maxStep = 0;
    for (let age = 1; age <= timeline.brakeEnd; age += 1) {
      const s = beltProgress(spec, { ...revealState, session: { ...revealSession, tick: revealSession.phaseStartTick + age } }, false);
      maxStep = Math.max(maxStep, Math.abs(s - prev));
      prev = s;
    }
    assert.ok(maxStep < 0.06, `per-tick belt delta ${maxStep.toFixed(4)} must stay below 0.06 (seed ${seed})`);

    // The capsule at the station once settled IS the committed destination.
    const settled: ConveyorState = { ...revealState, session: { ...revealSession, tick: revealSession.phaseStartTick + timeline.brakeEnd } };
    assert.equal(
      openingCapsuleIndex(spec, settled, false),
      destinationIndexOf(committed),
      `settled station capsule must be the committed destination (seed ${seed})`,
    );
  }
});
