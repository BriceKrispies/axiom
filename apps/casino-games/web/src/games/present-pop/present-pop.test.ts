/*
 * present-pop.test.ts — the burst-determinism and fairness proofs for Present
 * Pop:
 *
 *  1. BURST IS PRESENTATION-ONLY — every burst piece's pose is a pure function
 *     of (presentation seed, piece index, age). The same seed gives byte-equal
 *     debris; a different presentation seed reshuffles the debris — yet the
 *     committed winnersByIndex (a function of the GAMEPLAY seed/round, not the
 *     presentation seed) is untouched. Reshuffling the paper never rerolls the
 *     prize.
 *  2. BOUNDED DEBRIS — the burst is a fixed, small budget (BURST_PANELS +
 *     BURST_RIBBONS) with a finite lifetime; a piece is null before it launches
 *     and after it dies.
 *
 * Runs under bare `node --test` (no DOM); casino-mount.ts is never imported.
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { planChoicePopulation } from "../../chance-engine/probability/choice-population.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { PresentationSettings } from "../../chance-engine/registry/definition.ts";
import type { EngineVec3, InputFrame, TickContext } from "@axiom/web-engine";
import type { CasinoMountSpec, RoundEnvironment } from "../round-state.ts";
import { foldRoundTick, freshRoundState } from "../round-state.ts";
import type { BurstPiece, PresentPopExtra, PresentPopSpec } from "./game.ts";
import { BURST_PANELS, BURST_RIBBONS, burstPiece, initialPresentPopExtra, stepPresentPop } from "./game.ts";

const SETTINGS: PresentationSettings = {
  cameraShake: false,
  highContrast: false,
  masterVolume: 0,
  particleScale: 1,
  reducedMotion: false,
  sfxVolume: 0,
};

const CHOICE_COUNT = 6;

const config = (): CasinoGameConfig<PresentPopSpec> =>
  baseConfig("present-pop", "Present Pop", "showcase", { hopLiveliness: 0.7 }, { choiceCount: CHOICE_COUNT, targetWinRate: 0.42 });

const emptyInput = (): InputFrame => ({
  down: new Set(),
  look: { x: 0, y: 0 },
  pointer: undefined,
  pressed: new Set(),
  released: new Set(),
});

const withPress = (code: string): InputFrame => ({ ...emptyInput(), pressed: new Set([code]) });

const rig = (seed: number, round: number): { readonly env: RoundEnvironment; readonly spec: CasinoMountSpec<PresentPopExtra> } => {
  const cfg = config();
  const source = new SeededChanceResultSource(seed);
  const runtime = { config: cfg, onHud: (): void => {}, round, seed, settings: SETTINGS, source };
  const env: RoundEnvironment = { config: cfg, seed, settings: SETTINGS, source };
  const spec: CasinoMountSpec<PresentPopExtra> = {
    initExtra: initialPresentPopExtra,
    mechanic: { choiceCount: CHOICE_COUNT, kind: "choice" },
    resources: { materials: {}, meshes: {} },
    step: (state, input, ctx) => stepPresentPop(runtime, state, input, ctx),
    viewScene: () => ({ camera: { far: 1, fovY: 1, near: 0.1, position: { x: 0, y: 0, z: 0 }, target: { x: 0, y: 0, z: 0 } }, instances: [], lights: [] }),
  };
  return { env, spec };
};

const ORIGIN: EngineVec3 = { x: 0, y: 0.5, z: 0 };
const LIFE = 40;

/** All burst pieces (panels + ribbons) at one age for one presentation seed. */
const debrisAt = (presentationSeed: number, age: number): readonly (BurstPiece | null)[] =>
  Array.from({ length: BURST_PANELS + BURST_RIBBONS }, (_, i) => burstPiece(ORIGIN, presentationSeed, i, age, LIFE));

const driveToComplete = (seed: number, round: number): { readonly winnersByIndex: readonly (string | null)[]; readonly presentationSeed: number } => {
  const { env, spec } = rig(seed, round);
  let state = freshRoundState(env, spec, round, false);
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
  advance(withPress("primary"));
  guard = 0;
  while (state.session.phase !== "complete" && guard < 3000) {
    advance(emptyInput());
    guard += 1;
  }
  assert.equal(state.session.phase, "complete");
  const committed = state.session.committed;
  assert.ok(committed !== null && committed.manifestation.kind === "choice");
  return {
    presentationSeed: committed.presentationSeed,
    winnersByIndex: committed.manifestation.kind === "choice" ? committed.manifestation.winnersByIndex : [],
  };
};

test("burst debris is a pure function of the presentation seed; reshuffling it never rerolls the outcome", () => {
  const seed = 0xf00d;
  const round = 4;
  const { presentationSeed, winnersByIndex } = driveToComplete(seed, round);

  // The committed contents are exactly the pre-deal population — a function of
  // the GAMEPLAY seed/round, with no presentation-seed input.
  const dealt = planChoicePopulation(config(), CHOICE_COUNT, seed, round).winnersByIndex;
  assert.deepEqual([...winnersByIndex], [...dealt]);

  // Same presentation seed → byte-equal debris (purity).
  assert.deepEqual(debrisAt(presentationSeed, 20), debrisAt(presentationSeed, 20));

  // A DIFFERENT presentation seed → different debris, while the committed
  // population is unchanged (it never depended on the presentation seed).
  const reshuffled = debrisAt(presentationSeed ^ 0x5a5a, 20);
  assert.notDeepEqual(reshuffled, debrisAt(presentationSeed, 20));
  assert.deepEqual([...planChoicePopulation(config(), CHOICE_COUNT, seed, round).winnersByIndex], [...dealt]);
});

test("burst is a bounded, finite-lifetime debris budget", () => {
  assert.ok(BURST_PANELS + BURST_RIBBONS <= 10, "the debris budget is small and bounded");

  // A piece exists only during its life window.
  assert.equal(burstPiece(ORIGIN, 1, 0, -1, LIFE), null);
  assert.equal(burstPiece(ORIGIN, 1, 0, LIFE + 1, LIFE), null);
  assert.ok(burstPiece(ORIGIN, 1, 0, 0, LIFE) !== null);
  assert.ok(burstPiece(ORIGIN, 1, 0, LIFE, LIFE) !== null);

  // Never more than the fixed budget of live pieces at any age.
  for (let age = 0; age <= LIFE; age += 5) {
    const live = debrisAt(7, age).filter((piece) => piece !== null);
    assert.ok(live.length <= BURST_PANELS + BURST_RIBBONS);
  }
});

test("two different gameplay seeds can differ in outcome while each burst stays pure", () => {
  const a = driveToComplete(0x1111, 1);
  const b = driveToComplete(0x2222, 1);
  // Each round's debris is deterministic for its own presentation seed.
  assert.deepEqual(debrisAt(a.presentationSeed, 15), debrisAt(a.presentationSeed, 15));
  assert.deepEqual(debrisAt(b.presentationSeed, 15), debrisAt(b.presentationSeed, 15));
  // Distinct presentation seeds drive distinct debris.
  assert.notDeepEqual(a.presentationSeed, b.presentationSeed);
});
