/*
 * definition.ts — Card Flip: a grid of face-down cards, one pick. Choice-
 * population mechanic: the configured target win rate controls how many of the
 * cards hold a prize (stochastic rounding of n·p), assigned before the player
 * chooses; the flip only manifests the preassigned face.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { CardFlipSpec } from "./game.ts";
import { cardFlipCues, initialCardFlipExtra, stepCardFlip } from "./game.ts";
import { CARD_FLIP_RESOURCES, cardFlipScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<CardFlipSpec> =>
  baseConfig("card-flip", "Card Flip", "tabletop", { columns: 4 }, { choiceCount: 8, targetWinRate: 0.42 });

const validateSpec = (spec: CardFlipSpec): readonly ConfigIssue[] =>
  Number.isInteger(spec.columns) && spec.columns >= 2 && spec.columns <= 6
    ? []
    : [{ message: "columns must be an integer in [2, 6]", path: "gameSpecific.columns" }];

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<CardFlipSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialCardFlipExtra,
    instructionOf: (state) =>
      state.session.phase === "ready" ? "Pick a card — arrows + Enter, or click one" : null,
    mechanic: { choiceCount: runtime.config.choiceCount ?? 8, kind: "choice" },
    resources: CARD_FLIP_RESOURCES,
    sound: (prev, next) => cardFlipCues(prev, next, runtime.settings.reducedMotion),
    step: (state, input, ctx) => stepCardFlip(runtime, state, input, ctx),
    viewScene: (state) => cardFlipScene(runtime, state),
  });

export const CARD_FLIP: CasinoGameDefinition<CardFlipSpec> = {
  categories: ["choice"],
  defaultConfig,
  displayName: "Card Flip",
  id: "card-flip",
  instruction: "Pick a face-down card. The flip decides — the backs are all identical.",
  interaction: "pick one card",
  machineInterior: false,
  mechanic: "choice-population",
  mount: mount as CasinoGameDefinition<CardFlipSpec>["mount"],
  renderMode: "2d",
  shortDescription: "A table of matching cards, one flip. Turn yours over and see what it was hiding.",
  thumbnail: { accent: "#ffcc4d", bottom: "#1f6b82", glyph: "card", top: "#bfeaff" },
  validateSpec,
};
