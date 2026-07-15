/*
 * scratch-reveal.test.ts — the fairness/interaction contract for Scratch
 * Reveal, driven through the shared round fold (no DOM):
 *  - the outcome is committed BEFORE any tile is scratched (committed !== null
 *    the instant interacting begins, with an empty scratched set);
 *  - the reveal fires only when ≥ 55% of the SYMBOL-AREA tiles are scratched
 *    (a 50%-scratched state stays interacting; a 56%-scratched state reveals);
 *  - the revealed fraction counts symbol-area tiles only (scratching outside
 *    the symbol ellipse does not advance it).
 */

import assert from "node:assert/strict";
import { test } from "node:test";

import type { InputFrame } from "@axiom/web-engine";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { SeededChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import type { PresentationSettings } from "../../chance-engine/registry/definition.ts";
import type { CasinoMountSpec, RoundEnvironment } from "../round-state.ts";
import { foldRoundTick, freshRoundState } from "../round-state.ts";
import {
  DEFAULT_SCRATCH_SPEC,
  foilLayout,
  inSymbolArea,
  initialScratchExtra,
  REVEAL_THRESHOLD,
  revealedFraction,
  stepScratch,
  symbolAreaTiles,
  tileCenter,
} from "./game.ts";
import type { ScratchExtra, ScratchSpec, ScratchState } from "./game.ts";

const SETTINGS: PresentationSettings = {
  cameraShake: true,
  highContrast: false,
  masterVolume: 1,
  particleScale: 1,
  reducedMotion: false,
  sfxVolume: 1,
};

const configOf = (spec: ScratchSpec): CasinoGameConfig<ScratchSpec> =>
  baseConfig("scratch-reveal", "Scratch Reveal", "tabletop", spec, { targetWinRate: 0.5 });

const emptyFrame = (): InputFrame => ({
  down: new Set(),
  look: { x: 0, y: 0 },
  pointer: undefined,
  pressed: new Set(),
  released: new Set(),
});

const primaryFrame = (): InputFrame => ({ ...emptyFrame(), pressed: new Set(["primary"]) });

const CTX = { dt: 1 / 60, tick: 0 };

const buildEnv = (spec: ScratchSpec, seed: number): { readonly env: RoundEnvironment; readonly spec: CasinoMountSpec<ScratchExtra> } => {
  const config = configOf(spec);
  const source = new SeededChanceResultSource(seed);
  const env: RoundEnvironment = { config, seed, settings: SETTINGS, source };
  const runtime = { config, onHud: (): void => {}, round: 0, seed, settings: SETTINGS, source };
  const mountSpec: CasinoMountSpec<ScratchExtra> = {
    afterCommit: "interact",
    initExtra: initialScratchExtra,
    mechanic: { kind: "single" },
    resources: { materials: {}, meshes: {} },
    step: (state, input, ctx) => stepScratch(runtime as never, state as ScratchState, input, ctx),
    viewScene: () => ({ camera: { far: 1, fovY: 1, near: 0.1, position: { x: 0, y: 0, z: 1 }, target: { x: 0, y: 0, z: 0 } }, instances: [], lights: [] }),
  };
  return { env, spec: mountSpec };
};

const advance = (env: RoundEnvironment, spec: CasinoMountSpec<ScratchExtra>, state: ScratchState, input: InputFrame): ScratchState =>
  foldRoundTick(env, spec, state, input, { ...CTX, tick: state.session.tick + 1 }) as ScratchState;

/** Reach the interacting phase (commit) and report the state there. */
const reachInteracting = (spec: ScratchSpec, seed: number) => {
  const { env, spec: mountSpec } = buildEnv(spec, seed);
  let state = freshRoundState(env, mountSpec, 0, false) as ScratchState;
  let steps = 0;
  while (state.session.phase !== "ready" && steps < 2000) {
    state = advance(env, mountSpec, state, emptyFrame());
    steps += 1;
  }
  state = advance(env, mountSpec, state, primaryFrame());
  while (state.session.phase !== "interacting" && steps < 2000) {
    state = advance(env, mountSpec, state, emptyFrame());
    steps += 1;
  }
  return { env, spec: mountSpec, state };
};

test("outcome is committed before any tile is scratched", () => {
  for (let seed = 1; seed <= 20; seed += 1) {
    const { state } = reachInteracting(DEFAULT_SCRATCH_SPEC, seed);
    assert.equal(state.session.phase, "interacting");
    assert.notEqual(state.session.committed, null, "committed the instant scratching begins");
    assert.equal(state.extra.scratched.size, 0, "no tile scratched yet");
  }
});

test("reveal fires only at ≥55% of symbol-area tiles", () => {
  const spec = DEFAULT_SCRATCH_SPEC;
  const { env, spec: mountSpec, state } = reachInteracting(spec, 3);
  const layout = foilLayout(spec);
  const symbolTiles = symbolAreaTiles(layout);
  assert.ok(symbolTiles.length > 0);

  const scratchN = (n: number): ReadonlySet<number> => new Set(symbolTiles.slice(0, n));

  // 50% scratched: fraction below threshold → no reveal.
  const half = Math.floor(symbolTiles.length * 0.5);
  const belowFrac = revealedFraction(scratchN(half), symbolTiles);
  assert.ok(belowFrac < REVEAL_THRESHOLD, `expected ${belowFrac} < ${REVEAL_THRESHOLD}`);
  const below: ScratchState = { ...state, extra: { ...state.extra, scratched: scratchN(half) } };
  const afterBelow = advance(env, mountSpec, below, emptyFrame());
  assert.equal(afterBelow.session.phase, "interacting", "50% must not reveal");

  // 56% scratched: at or above threshold → reveal.
  const many = Math.ceil(symbolTiles.length * 0.56);
  const aboveFrac = revealedFraction(scratchN(many), symbolTiles);
  assert.ok(aboveFrac >= REVEAL_THRESHOLD, `expected ${aboveFrac} ≥ ${REVEAL_THRESHOLD}`);
  const above: ScratchState = { ...state, extra: { ...state.extra, scratched: scratchN(many) } };
  const afterAbove = advance(env, mountSpec, above, emptyFrame());
  assert.equal(afterAbove.session.phase, "revealing", "56% must reveal");
});

test("revealed fraction counts symbol-area tiles only", () => {
  const layout = foilLayout(DEFAULT_SCRATCH_SPEC);
  const symbolTiles = new Set(symbolAreaTiles(layout));
  // Scratching every NON-symbol tile leaves the fraction at zero.
  const outside: number[] = [];
  for (let row = 0; row < layout.rows; row += 1) {
    for (let col = 0; col < layout.columns; col += 1) {
      const index = row * layout.columns + col;
      if (!symbolTiles.has(index)) {
        assert.equal(inSymbolArea(layout, col, row), false);
        outside.push(index);
      }
    }
  }
  assert.ok(outside.length > 0, "there should be tiles outside the symbol ellipse");
  assert.equal(revealedFraction(new Set(outside), [...symbolTiles]), 0, "outside tiles do not count");

  // A symbol-area tile does count.
  const anySymbol = [...symbolTiles][0] as number;
  const oneFrac = revealedFraction(new Set([anySymbol]), [...symbolTiles]);
  assert.ok(oneFrac > 0);
  // Sanity: a symbol tile's center really is inside the ellipse.
  const col = anySymbol % layout.columns;
  const row = Math.floor(anySymbol / layout.columns);
  assert.equal(inSymbolArea(layout, col, row), true);
  const center = tileCenter(layout, col, row);
  assert.ok(center.x >= layout.x0 && center.x <= layout.x0 + layout.width);
});
