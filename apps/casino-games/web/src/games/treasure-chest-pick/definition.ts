/*
 * definition.ts — Treasure Chest Pick: nine carved-wood chests, one pick.
 * Choice-population mechanic: the configured target win rate controls how many
 * of the nine chests hold prizes (stochastic rounding of 9·p), assigned before
 * the player chooses.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { ChestSpec } from "./game.ts";
import { chestCues, initialChestExtra, stepChest } from "./game.ts";
import { CHEST_RESOURCES, chestScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<ChestSpec> =>
  baseConfig("treasure-chest-pick", "Treasure Chest Pick", "tabletop", { danceLiveliness: 0.7 }, { choiceCount: 9, targetWinRate: 0.42 });

const validateSpec = (spec: ChestSpec): readonly ConfigIssue[] =>
  typeof spec.danceLiveliness === "number" && Number.isFinite(spec.danceLiveliness) && spec.danceLiveliness >= 0 && spec.danceLiveliness <= 1
    ? []
    : [{ message: "danceLiveliness must be a finite number in [0, 1]", path: "gameSpecific.danceLiveliness" }];

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<ChestSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialChestExtra,
    instructionOf: (state) =>
      state.session.phase === "ready" ? "Pick a chest — arrows + Enter, or click one" : null,
    mechanic: { choiceCount: runtime.config.choiceCount ?? 9, kind: "choice" },
    resources: CHEST_RESOURCES,
    sound: (prev, next) => chestCues(prev, next),
    step: (state, input, ctx) => stepChest(runtime, state, input, ctx),
    viewScene: (state) => chestScene(runtime, state),
  });

export const TREASURE_CHEST_PICK: CasinoGameDefinition<ChestSpec> = {
  categories: ["choice"],
  defaultConfig,
  displayName: "Treasure Chest Pick",
  id: "treasure-chest-pick",
  instruction: "Pick one of nine chests. Some hold prizes — the latch tells you nothing.",
  interaction: "pick one of nine",
  machineInterior: false,
  mechanic: "choice-population",
  mount: mount as CasinoGameDefinition<ChestSpec>["mount"],
  renderMode: "3d",
  shortDescription: "Nine carved chests, golden latches, one choice. Dance all they like — only the pick decides.",
  thumbnail: { accent: "#ffcc4d", bottom: "#8a5a2b", glyph: "chest", top: "#8fd0ff" },
  validateSpec,
};
