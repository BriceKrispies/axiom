/*
 * definition.ts — Present Pop: a grid of wrapped presents, one pick. Choice-
 * population mechanic: the configured target win rate controls how many
 * presents hold a prize (stochastic rounding of n·p), assigned before the
 * player chooses; the burst only manifests the preassigned contents.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { PresentPopSpec } from "./game.ts";
import { initialPresentPopExtra, presentPopCues, stepPresentPop } from "./game.ts";
import { PRESENT_POP_RESOURCES, presentPopScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<PresentPopSpec> =>
  baseConfig("present-pop", "Present Pop", "showcase", { hopLiveliness: 0.7 }, { choiceCount: 6, targetWinRate: 0.42 });

const validateSpec = (spec: PresentPopSpec): readonly ConfigIssue[] =>
  typeof spec.hopLiveliness === "number" &&
  Number.isFinite(spec.hopLiveliness) &&
  spec.hopLiveliness >= 0 &&
  spec.hopLiveliness <= 1
    ? []
    : [{ message: "hopLiveliness must be a finite number in [0, 1]", path: "gameSpecific.hopLiveliness" }];

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<PresentPopSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialPresentPopExtra,
    instructionOf: (state) =>
      state.session.phase === "ready" ? "Pick a present — arrows + Enter, or click one" : null,
    mechanic: { choiceCount: runtime.config.choiceCount ?? 6, kind: "choice" },
    resources: PRESENT_POP_RESOURCES,
    sound: (prev, next) => presentPopCues(prev, next, runtime.settings.reducedMotion),
    step: (state, input, ctx) => stepPresentPop(runtime, state, input, ctx),
    viewScene: (state) => presentPopScene(runtime, state),
  });

export const PRESENT_POP: CasinoGameDefinition<PresentPopSpec> = {
  categories: ["choice"],
  defaultConfig,
  displayName: "Present Pop",
  id: "present-pop",
  instruction: "Pick a wrapped present. Give it a shake and pop it open — the wrapping tells you nothing.",
  interaction: "pick a present",
  machineInterior: false,
  mechanic: "choice-population",
  mount: mount as CasinoGameDefinition<PresentPopSpec>["mount"],
  renderMode: "3d",
  shortDescription: "A table of wrapped gifts, one pick. Shake it, pop it, and watch the paper fly.",
  thumbnail: { accent: "#ffd24d", bottom: "#3d7ecc", glyph: "gift", top: "#ff9d8a" },
  validateSpec,
};
