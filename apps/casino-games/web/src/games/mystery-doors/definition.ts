/*
 * definition.ts — Mystery Doors: a short row of freestanding doors, one pick.
 * Choice-population mechanic: the configured target win rate controls how many
 * doors hide a prize (stochastic rounding of n·p), assigned before the player
 * chooses; the swing only manifests the preassigned contents.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { DoorsSpec } from "./game.ts";
import { doorsCues, initialDoorsExtra, stepDoors } from "./game.ts";
import { DOORS_RESOURCES, doorsScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<DoorsSpec> =>
  baseConfig("mystery-doors", "Mystery Doors", "showcase", { breatheLiveliness: 0.7 }, { choiceCount: 3, targetWinRate: 0.42 });

const validateSpec = (spec: DoorsSpec): readonly ConfigIssue[] =>
  typeof spec.breatheLiveliness === "number" &&
  Number.isFinite(spec.breatheLiveliness) &&
  spec.breatheLiveliness >= 0 &&
  spec.breatheLiveliness <= 1
    ? []
    : [{ message: "breatheLiveliness must be a finite number in [0, 1]", path: "gameSpecific.breatheLiveliness" }];

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<DoorsSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialDoorsExtra,
    instructionOf: (state) =>
      state.session.phase === "ready" ? "Choose a door — arrows + Enter, or click one" : null,
    mechanic: { choiceCount: runtime.config.choiceCount ?? 3, kind: "choice" },
    resources: DOORS_RESOURCES,
    sound: (prev, next) => doorsCues(prev, next, runtime.settings.reducedMotion),
    step: (state, input, ctx) => stepDoors(runtime, state, input, ctx),
    viewScene: (state) => doorsScene(runtime, state),
  });

export const MYSTERY_DOORS: CasinoGameDefinition<DoorsSpec> = {
  categories: ["choice"],
  defaultConfig,
  displayName: "Mystery Doors",
  id: "mystery-doors",
  instruction: "Choose a door. Open it and see what was waiting — the paint tells you nothing.",
  interaction: "choose a door",
  machineInterior: false,
  mechanic: "choice-population",
  mount: mount as CasinoGameDefinition<DoorsSpec>["mount"],
  renderMode: "3d",
  shortDescription: "Three painted doors, one choice. Turn the knob and swing it wide.",
  thumbnail: { accent: "#ffd24d", bottom: "#5a3d99", glyph: "door", top: "#ff9d8a" },
  validateSpec,
};
