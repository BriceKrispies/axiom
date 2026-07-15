/*
 * claw-grab.test.ts — the focused fairness proofs for Claw Grab:
 *  1. the context passed at commitment equals the prize under the claw at drop
 *     time (place the claw over prize k, run the committing step, assert
 *     pendingContext.targetedPrizeIndex === k);
 *  2. on a LOSING seed the reveal still involves index k — the committed focus
 *     prize and the carried prize are never a distant substitute.
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import type { InputFrame } from "@axiom/web-engine";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { PresentationSettings } from "../../chance-engine/registry/definition.ts";
import { commitOutcome, createSession, transition } from "../../chance-engine/sessions/session.ts";
import type { ClawGrabSpec, ClawState } from "./game.ts";
import {
  carriedPrizeIndex,
  clawTimeline,
  focusIndexOf,
  initialClawExtra,
  prizePosition,
  stepClawGrab,
} from "./game.ts";

const SETTINGS: PresentationSettings = {
  cameraShake: false,
  highContrast: false,
  masterVolume: 1,
  particleScale: 1,
  reducedMotion: false,
  sfxVolume: 1,
};

const spec: ClawGrabSpec = { prizeCount: 7, steerSpeed: 0.06 };

const runtimeFor = (seed: number, winRate: number) => ({
  config: baseConfig("claw-grab", "Claw Grab", "machine-interior", spec, { targetWinRate: winRate }),
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

/** A state with the claw parked exactly over prize `k`, already in "interacting". */
const stateOverPrize = (runtime: ReturnType<typeof runtimeFor>, k: number): ClawState => {
  const base = createSession(runtime.config, runtime.seed, 0, runtime.source, { kind: "single" });
  const session = transition(transition(base, "ready"), "interacting");
  const at = prizePosition(k, spec.prizeCount);
  return { extra: { clawX: at.x, clawZ: at.z }, pendingContext: null, pendingReset: null, session };
};

test("the drop commits the prize under the claw", () => {
  const runtime = runtimeFor(4242, 0.5);
  for (let k = 0; k < spec.prizeCount; k += 1) {
    const state = stateOverPrize(runtime, k);
    const dropped = stepClawGrab(runtime, state, frame({ pressed: new Set(["primary"]) }), { dt: 1 / 60, tick: 10 });
    assert.equal(dropped.session.phase, "committing", "a drop enters the committing phase");
    assert.equal(dropped.pendingContext?.targetedPrizeIndex, k, `drop over prize ${k} must target ${k}`);
  }
});

test("a losing round never substitutes a distant prize", () => {
  // targetWinRate 0 guarantees a loss; sweep seeds so every loss flavor appears.
  const k = 3;
  for (let seed = 1; seed <= 40; seed += 1) {
    const runtime = runtimeFor(seed, 0);
    const state = stateOverPrize(runtime, k);
    const dropped = stepClawGrab(runtime, state, frame({ pressed: new Set(["primary"]) }), { dt: 1 / 60, tick: 10 });
    const committedSession = commitOutcome(dropped.session, runtime.source, dropped.pendingContext ?? {});
    assert.equal(committedSession.committed?.win, false, "targetWinRate 0 must lose");
    assert.equal(focusIndexOf(committedSession), k, `losing focus prize must stay ${k} (seed ${seed})`);

    // Sweep the whole reveal: any carried prize must be exactly k, never a substitute.
    const timeline = clawTimeline(runtime.config.presentationSpeed, false);
    const revealSession = transition(committedSession, "revealing");
    for (let age = 0; age <= timeline.total; age += 1) {
      const carried = carriedPrizeIndex({ ...revealSession, tick: revealSession.phaseStartTick + age }, false);
      assert.ok(carried === null || carried === k, `carried prize must be null or ${k} (seed ${seed}, age ${age})`);
    }
  }
});
