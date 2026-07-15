/*
 * treasure-map.test.ts — the Treasure Map fairness pins: the committed
 * winnersByIndex is exactly the population assigned at session start (no
 * substitution anywhere between ready and complete), the winner count in the
 * manifestation matches the population, and the dig layout is a fixed pure
 * function of site index (no stream, no seed). Runs under bare `node --test`
 * with no DOM — the fold and the controller are pure.
 */

import assert from "node:assert/strict";
import test from "node:test";
import type { InputFrame } from "@axiom/web-engine";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { PresentationSettings } from "../../chance-engine/registry/definition.ts";
import type { CasinoMountSpec, RoundEnvironment } from "../round-state.ts";
import { foldRoundTick, freshRoundState } from "../round-state.ts";
import type { MapExtra, MapSpec } from "./game.ts";
import { digPosition, initialMapExtra, mapCamera, MAP_MAX_CHOICES, stepMap } from "./game.ts";

const SETTINGS: PresentationSettings = {
  cameraShake: false,
  highContrast: false,
  masterVolume: 1,
  particleScale: 1,
  reducedMotion: false,
  sfxVolume: 1,
};

const frame = (pressed: readonly string[] = []): InputFrame => ({
  down: new Set<string>(),
  look: { x: 0, y: 0 },
  pointer: undefined,
  pressed: new Set(pressed),
  released: new Set<string>(),
});

const CTX = { dt: 1 / 60, tick: 0 };

const harness = (seed: number) => {
  const config = baseConfig<MapSpec>(
    "treasure-map",
    "Treasure Map",
    "tabletop",
    { compassLiveliness: 0.6, markerPulse: 0.6 },
    { choiceCount: 6 },
  );
  const source = new SeededChanceResultSource(seed);
  const env: RoundEnvironment = { config, seed, settings: SETTINGS, source };
  const runtime = { config, onHud: (): void => {}, round: 1, seed, settings: SETTINGS, source };
  const spec: CasinoMountSpec<MapExtra> = {
    initExtra: initialMapExtra,
    mechanic: { choiceCount: 6, kind: "choice" },
    resources: { materials: {}, meshes: {} },
    step: (state, input, ctx) => stepMap(runtime, state, input, ctx),
    viewScene: () => ({ camera: mapCamera(6), instances: [], lights: [] }),
  };
  return { env, spec };
};

test("treasure-map: committed winnersByIndex is the ready-phase population, unsubstituted", () => {
  const { env, spec } = harness(0xd16_c0de);
  let state = freshRoundState(env, spec, 1, false);

  let guard = 0;
  while (state.session.phase !== "ready" && guard < 300) {
    state = foldRoundTick(env, spec, state, frame(), CTX);
    guard += 1;
  }
  assert.equal(state.session.phase, "ready");

  const plan = state.session.mechanicPlan;
  assert.equal(plan.kind, "choice");
  const population = plan.kind === "choice" ? plan.population : null;
  assert.notEqual(population, null);
  const readyWinners = [...(population?.winnersByIndex ?? [])];
  assert.equal(readyWinners.length, 6);

  // Select the focused site with the primary action.
  state = foldRoundTick(env, spec, state, frame(["primary"]), CTX);
  assert.equal(state.session.phase, "committing");

  guard = 0;
  while (state.session.phase !== "complete" && guard < 3000) {
    state = foldRoundTick(env, spec, state, frame(), CTX);
    guard += 1;
  }
  assert.equal(state.session.phase, "complete");

  const committed = state.session.committed;
  assert.notEqual(committed, null);
  const manifestation = committed?.manifestation;
  assert.equal(manifestation?.kind, "choice");
  if (manifestation?.kind === "choice") {
    assert.deepEqual([...manifestation.winnersByIndex], readyWinners);
    assert.equal(
      manifestation.winnersByIndex.filter((tier) => tier !== null).length,
      manifestation.winnerCount,
    );
    assert.equal(manifestation.winnerCount, population?.winnerCount);
  }
});

test("treasure-map: dig layout is a fixed pure function of index", () => {
  const seen = new Set<string>();
  for (let index = 0; index < MAP_MAX_CHOICES; index += 1) {
    const a = digPosition(index);
    const b = digPosition(index);
    assert.deepEqual(a, b);
    seen.add(`${a.x},${a.y},${a.z}`);
  }
  // All sites are distinct authored spots — a real layout, not a collapsed one.
  assert.equal(seen.size, MAP_MAX_CHOICES);
});
