/*
 * card-flip.test.ts — the fairness-focused proofs for Card Flip:
 *
 *  1. NO SUBSTITUTION — the unselected cards' contents (the committed
 *     winnersByIndex) are exactly the population assigned before the player
 *     could choose, identical from deal through completion for one seed/round.
 *  2. SEALED FLIP — the flip animation reaches face-up (angle > 0) only once a
 *     committed outcome exists; before commitment the selected card never
 *     turns, and the pose is a pure function of reveal age.
 *
 * Runs under bare `node --test` (no DOM): the round is driven through the pure
 * fold in round-state.ts; casino-mount.ts is never imported.
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import type { ChoicePopulation } from "../../chance-engine/probability/choice-population.ts";
import { createSession } from "../../chance-engine/sessions/session.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { PresentationSettings } from "../../chance-engine/registry/definition.ts";
import type { InputFrame, TickContext } from "@axiom/web-engine";
import type { CasinoMountSpec, RoundEnvironment } from "../round-state.ts";
import { foldRoundTick, freshRoundState } from "../round-state.ts";
import type { CardFlipExtra, CardFlipSpec } from "./game.ts";
import { cardFlipPose, cardTimeline, FACE_UP_ANGLE, initialCardFlipExtra, revealAgeOf, stepCardFlip } from "./game.ts";

const SETTINGS: PresentationSettings = {
  cameraShake: false,
  highContrast: false,
  masterVolume: 0,
  particleScale: 1,
  reducedMotion: false,
  sfxVolume: 0,
};

const config = (): CasinoGameConfig<CardFlipSpec> =>
  baseConfig("card-flip", "Card Flip", "tabletop", { columns: 4 }, { choiceCount: 8, targetWinRate: 0.42 });

const emptyInput = (): InputFrame => ({
  down: new Set(),
  look: { x: 0, y: 0 },
  pointer: undefined,
  pressed: new Set(),
  released: new Set(),
});

const withPress = (code: string): InputFrame => ({ ...emptyInput(), pressed: new Set([code]) });

interface Rig {
  readonly env: RoundEnvironment;
  readonly spec: CasinoMountSpec<CardFlipExtra>;
}

const rig = (seed: number, round: number): Rig => {
  const cfg = config();
  const source = new SeededChanceResultSource(seed);
  const runtime = { config: cfg, onHud: (): void => {}, round, seed, settings: SETTINGS, source };
  const env: RoundEnvironment = { config: cfg, seed, settings: SETTINGS, source };
  const spec: CasinoMountSpec<CardFlipExtra> = {
    initExtra: initialCardFlipExtra,
    mechanic: { choiceCount: 8, kind: "choice" },
    resources: { materials: {}, meshes: {} },
    step: (state, input, ctx) => stepCardFlip(runtime, state, input, ctx),
    viewScene: (state) => ({ camera: { far: 1, fovY: 1, near: 0.1, position: { x: 0, y: 0, z: 0 }, target: { x: 0, y: 0, z: 0 } }, instances: [], lights: [] }),
  };
  return { env, spec };
};

test("committed winnersByIndex equals the pre-deal population, unchanged at completion", () => {
  const { env, spec } = rig(0x51ce, 3);
  const start = freshRoundState(env, spec, 3, false);
  const plan = start.session.mechanicPlan;
  assert.equal(plan.kind, "choice");
  const population = (plan as { readonly population: ChoicePopulation }).population;
  const dealt = [...population.winnersByIndex];

  let state = start;
  let tick = 0;
  const advance = (input: InputFrame): void => {
    tick += 1;
    const ctx: TickContext = { dt: 1 / 60, tick };
    state = foldRoundTick(env, spec, state, input, ctx);
  };

  let guard = 0;
  while (state.session.phase === "intro" && guard < 200) {
    advance(emptyInput());
    guard += 1;
  }
  assert.equal(state.session.phase, "ready");

  // Select the focused card (index 0) with the primary action.
  advance(withPress("primary"));
  assert.equal(state.session.phase, "committing");

  guard = 0;
  while (state.session.phase !== "complete" && guard < 2000) {
    advance(emptyInput());
    guard += 1;
  }
  assert.equal(state.session.phase, "complete");

  const committed = state.session.committed;
  assert.ok(committed !== null);
  assert.equal(committed.manifestation.kind, "choice");
  const revealed = committed.manifestation.kind === "choice" ? committed.manifestation.winnersByIndex : [];
  assert.deepEqual([...revealed], dealt, "the revealed population is exactly what was dealt");
});

test("flip pose is a pure function of reveal age, face-up only after commitment", () => {
  const timeline = cardTimeline(1, false);
  // Before any reveal exists, the card is face-down and unlifted.
  assert.deepEqual(cardFlipPose(-1, timeline), { angle: 0, lift: 0, squash: 0 });
  // Purity: same age → identical pose.
  assert.deepEqual(cardFlipPose(timeline.liftEnd, timeline), cardFlipPose(timeline.liftEnd, timeline));
  // The flip completes to a full face-up turn by the end of the timeline.
  assert.ok(Math.abs(cardFlipPose(timeline.total, timeline).angle - FACE_UP_ANGLE) < 1e-9);
});

test("throughout a driven round, any face-up turn implies a committed outcome", () => {
  const { env, spec } = rig(0xa11ce, 1);
  const timeline = cardTimeline(1, false);
  let state = freshRoundState(env, spec, 1, false);
  let tick = 0;
  const advance = (input: InputFrame): void => {
    tick += 1;
    state = foldRoundTick(env, spec, state, input, { dt: 1 / 60, tick });
  };

  let guard = 0;
  while (state.session.phase === "intro" && guard < 200) {
    advance(emptyInput());
    guard += 1;
  }
  advance(withPress("primary"));

  guard = 0;
  while (state.session.phase !== "complete" && guard < 2000) {
    const revealAge = revealAgeOf(state.session, timeline.total);
    const angle = cardFlipPose(revealAge, timeline).angle;
    if (angle > 0) {
      assert.ok(state.session.committed !== null, "no face-up turn without a committed outcome");
    }
    advance(emptyInput());
    guard += 1;
  }
  assert.equal(state.session.phase, "complete");
});
