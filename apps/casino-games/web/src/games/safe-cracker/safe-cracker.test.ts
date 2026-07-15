/*
 * safe-cracker.test.ts — the fairness/animation contract for Safe Cracker,
 * driven through the shared round fold (no DOM):
 *  - the outcome commits at the FIRST stop (committed !== null before dial 2
 *    stops), and is never re-rolled;
 *  - after all three stops the settled dial symbols equal the committed
 *    combination exactly;
 *  - the bolts retract sequentially (bolt k starts before bolt k+1).
 */

import assert from "node:assert/strict";
import { test } from "node:test";

import type { InputFrame } from "@axiom/web-engine";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { CasinoMountSpec, RoundEnvironment } from "../round-state.ts";
import { foldRoundTick, freshRoundState } from "../round-state.ts";
import type { PresentationSettings } from "../../chance-engine/registry/definition.ts";
import {
  boltRetractStart,
  DEFAULT_SAFE_SPEC,
  DIAL_COUNT,
  initialSafeExtra,
  safeSpace,
  safeTimeline,
  settledDialSymbol,
  stepSafe,
  stopsMade,
} from "./game.ts";
import type { SafeExtra, SafeSpec, SafeState } from "./game.ts";

const SETTINGS: PresentationSettings = {
  cameraShake: true,
  highContrast: false,
  masterVolume: 1,
  particleScale: 1,
  reducedMotion: false,
  sfxVolume: 1,
};

const configOf = (spec: SafeSpec): CasinoGameConfig<SafeSpec> =>
  baseConfig("safe-cracker", "Safe Cracker", "showcase", spec, { targetWinRate: 0.5 });

const emptyFrame = (): InputFrame => ({
  down: new Set(),
  look: { x: 0, y: 0 },
  pointer: undefined,
  pressed: new Set(),
  released: new Set(),
});

const primaryFrame = (): InputFrame => ({ ...emptyFrame(), pressed: new Set(["primary"]) });

const CTX = { dt: 1 / 60, tick: 0 };

/** Advance the fold one tick with the given input. */
const advance = (
  environment: RoundEnvironment,
  spec: CasinoMountSpec<SafeExtra>,
  state: SafeState,
  input: InputFrame,
): SafeState => foldRoundTick(environment, spec, state, input, { ...CTX, tick: state.session.tick + 1 }) as SafeState;

/** Play a full round: reach ready, press to commit + stop dial 1, then press
 * to stop dials 2 and 3, capturing the tick each stop is registered. */
const playRound = (spec: SafeSpec, seed: number) => {
  const config = configOf(spec);
  const source = new SeededChanceResultSource(seed);
  const environment: RoundEnvironment = { config, seed, settings: SETTINGS, source };
  const mountSpec: CasinoMountSpec<SafeExtra> = {
    afterCommit: "interact",
    initExtra: initialSafeExtra,
    mechanic: { kind: "combination", space: safeSpace(spec) },
    resources: { materials: {}, meshes: {} },
    step: (state, input, ctx) =>
      stepSafe({ config, round: 0, seed, settings: SETTINGS, source, onHud: () => {} } as never, state as SafeState, input, ctx),
    viewScene: () => ({ camera: { far: 1, fovY: 1, near: 0.1, position: { x: 0, y: 0, z: 1 }, target: { x: 0, y: 0, z: 0 } }, instances: [], lights: [] }),
  };

  let state = freshRoundState(environment, mountSpec, 0, false) as SafeState;
  const guard = 4000;
  let steps = 0;

  // 1) Intro auto-advances to ready.
  while (state.session.phase !== "ready" && steps < guard) {
    state = advance(environment, mountSpec, state, emptyFrame());
    steps += 1;
  }
  assert.equal(state.session.phase, "ready");
  assert.equal(state.session.committed, null);

  // 2) First press: commit + (on the interacting hand-off) stop dial 1.
  state = advance(environment, mountSpec, state, primaryFrame());
  // Run the committing pause until interacting begins and dial 1 registers.
  while (stopsMade(state.extra) < 1 && steps < guard) {
    state = advance(environment, mountSpec, state, emptyFrame());
    steps += 1;
  }
  assert.equal(state.session.phase, "interacting");
  const committedAtFirstStop = state.session.committed;
  assert.notEqual(committedAtFirstStop, null, "outcome must commit by the first stop");
  assert.equal(stopsMade(state.extra), 1);

  // 3) Press to stop dials 2 and 3.
  state = advance(environment, mountSpec, state, primaryFrame());
  assert.equal(stopsMade(state.extra), 2);
  assert.notEqual(state.session.committed, null, "still committed before dial 3");
  state = advance(environment, mountSpec, state, primaryFrame());
  assert.equal(stopsMade(state.extra), 3);

  const stops = state.extra.stops.map((s) => s as number);

  // 4) Let the eases finish → revealing → celebrating.
  while (state.session.phase !== "celebrating" && state.session.phase !== "complete" && steps < guard) {
    state = advance(environment, mountSpec, state, emptyFrame());
    steps += 1;
  }

  return { committedAtFirstStop, seed, state, stops };
};

test("outcome commits at the first stop and never re-rolls", () => {
  for (let seed = 1; seed <= 20; seed += 1) {
    const { committedAtFirstStop, state } = playRound(DEFAULT_SAFE_SPEC, seed);
    assert.notEqual(committedAtFirstStop, null);
    // The committed plan the reveal celebrates is the very one from the first stop.
    assert.equal(state.session.committed?.roundId, committedAtFirstStop?.roundId);
    assert.deepEqual(state.session.committed?.manifestation, committedAtFirstStop?.manifestation);
  }
});

test("settled dial symbols equal the committed combination", () => {
  let sawWin = false;
  let sawLoss = false;
  for (let seed = 1; seed <= 30; seed += 1) {
    const { state, stops } = playRound(DEFAULT_SAFE_SPEC, seed);
    const plan = state.session.committed;
    assert.ok(plan !== null);
    const combination = plan.manifestation.kind === "combination" ? plan.manifestation.combination : [];
    assert.equal(combination.length, DIAL_COUNT);
    sawWin ||= plan.win;
    sawLoss ||= !plan.win;
    for (let k = 0; k < DIAL_COUNT; k += 1) {
      const settled = settledDialSymbol(stops[k] as number, k, combination, state.session.seed, DEFAULT_SAFE_SPEC.symbols);
      assert.equal(settled, combination[k], `seed ${seed} dial ${k}`);
    }
  }
  assert.ok(sawWin, "expected at least one winning round");
  assert.ok(sawLoss, "expected at least one losing round");
});

test("bolts retract one at a time (bolt k starts before bolt k+1)", () => {
  const timeline = safeTimeline(1, false);
  for (let k = 0; k + 1 < 4; k += 1) {
    assert.ok(
      boltRetractStart(k, timeline) < boltRetractStart(k + 1, timeline),
      `bolt ${k} must start before bolt ${k + 1}`,
    );
  }
});
