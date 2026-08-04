/*
 * render-quality.test.ts — the chest game's rendering-quality SETUP contract:
 * that the game ships the defaults it documents, that the Set Up panel's edits
 * and its Reset Quality button touch nothing but rendering, and — the one that
 * actually matters — that no quality setting can move a round's outcome.
 *
 * The engine's own arithmetic (clamping, pixel-ratio modes, backing-store size)
 * is covered in packages/axiom-web-engine/src/render-quality.test.ts. What is
 * tested here is the wiring: the values this game chose, and the fairness
 * boundary they must never cross.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { RENDER_SCALES, clampRenderQuality } from "@axiom/web-engine";
import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { validateConfig } from "../../chance-engine/configuration/validation.ts";
import { planChoicePopulation } from "../../chance-engine/probability/choice-population.ts";
import { TREASURE_CHEST_PICK } from "./definition.ts";
import type { ChestSpec } from "./game.ts";

const config = (): CasinoGameConfig<ChestSpec> => TREASURE_CHEST_PICK.defaultConfig() as CasinoGameConfig<ChestSpec>;

test("the game ships the rendering-quality defaults it documents", () => {
  const quality = clampRenderQuality(config().renderQuality);
  // The default deliberately reproduces the resolution the engine drew at before
  // these controls existed, so adding them costs no player a frame.
  assert.equal(quality.renderScale, 0.5, "the engine's former fixed software resolution");
  assert.equal(quality.pixelRatioMode, "fixed-1x", "a software rasterizer does not follow a 2x display by default");
  assert.equal(quality.curveDetail, 1, "backend-native tessellation");
  assert.equal(quality.lineJoin, "round");
  assert.equal(quality.lineCap, "round");
});

test("the shipped default is one of the values the Set Up control offers", () => {
  // A default the panel cannot represent would silently snap to another rung the
  // first time the player opened Set Up.
  const quality = clampRenderQuality(config().renderQuality);
  assert.ok(RENDER_SCALES.includes(quality.renderScale), `${quality.renderScale} is a selectable rung`);
});

test("every offered supersampling rung survives validation on this game's config", () => {
  for (const renderScale of RENDER_SCALES) {
    const edited = { ...config(), renderQuality: { ...config().renderQuality, renderScale } };
    assert.equal(clampRenderQuality(edited.renderQuality).renderScale, renderScale);
    assert.deepEqual(validateConfig(edited), [], `renderScale ${renderScale} is a valid config`);
  }
});

test("editing rendering quality changes nothing else in the config", () => {
  // This is the Set Up panel's patch, and the shape Reset Quality restores
  // through: replacing `renderQuality` wholesale must leave every other field —
  // win rate, tiers, choice count, brand — byte-identical.
  const before = config();
  const after = { ...before, renderQuality: { ...before.renderQuality, renderScale: 2 } };
  const { renderQuality: _dropped, ...restBefore } = before;
  const { renderQuality: _alsoDropped, ...restAfter } = after;
  assert.deepEqual(restAfter, restBefore, "only renderQuality moved");
});

test("Reset Quality restores the game's defaults and only those", () => {
  const tweaked: CasinoGameConfig<ChestSpec> = {
    ...config(),
    renderQuality: { curveDetail: 0.25, lineCap: "butt", lineJoin: "miter", pixelRatioMode: "device", renderScale: 2 },
    targetWinRate: 0.31,
  };
  // What the RESET QUALITY button does: put back `defaultConfig().renderQuality`.
  const reset = { ...tweaked, renderQuality: config().renderQuality };
  assert.deepEqual(clampRenderQuality(reset.renderQuality), clampRenderQuality(config().renderQuality));
  assert.equal(reset.targetWinRate, 0.31, "an unrelated setup value the operator changed is NOT reset");
});

test("rendering quality cannot move a round's outcome", () => {
  // The fairness path takes the seed, the win rate, and the tier weights. Quality
  // is not among them, and this is the test that keeps it that way: the same seed
  // must resolve the same populated chests at the cheapest and the most expensive
  // rendering settings there are.
  const seed = 470_573_198;
  const base = config();
  const roundFor = (renderScale: number): readonly (string | null)[] => {
    const cfg = { ...base, renderQuality: { ...base.renderQuality, renderScale } };
    return planChoicePopulation(cfg, cfg.choiceCount ?? 9, seed, 1).winnersByIndex;
  };
  assert.deepEqual(roundFor(2), roundFor(0.5), "cheapest and most expensive quality resolve the identical round");
});
