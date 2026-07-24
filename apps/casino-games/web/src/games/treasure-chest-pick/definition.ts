/*
 * definition.ts — Treasure Chest Pick: nine carved-wood chests, one pick.
 * Choice-population mechanic: the configured target win rate controls how many
 * of the nine chests hold prizes (stochastic rounding of 9·p), assigned before
 * the player chooses.
 */

import type { CasinoGameConfig, RewardTier } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { ChestSpec } from "./game.ts";
import { CHEST_TIMING, chestCues, initialChestExtra, stepChest } from "./game.ts";
import { CHEST_RESOURCES, chestScene } from "./scene.ts";

/** A single small consolation prize — every win grants 5 points. */
const CONSOLATION_TIER: RewardTier = {
  countsAsWin: true,
  id: "consolation",
  label: "5 points",
  rarity: "common",
  reward: { amount: 5, kind: "points", label: "5 points" },
  weight: 1,
};

// Win every time, for a small consolation prize. `targetWinRate: 1` makes all
// nine chests winners (9·1 = 9), so any pick wins; the single reward tier means
// that win is always the same modest 5 points. Tune either in the Set Up panel.
const defaultConfig = (): CasinoGameConfig<ChestSpec> =>
  baseConfig("treasure-chest-pick", "Treasure Chest Pick", "tabletop", { danceLiveliness: 0.7 }, {
    choiceCount: 9,
    rewardTiers: [CONSOLATION_TIER],
    targetWinRate: 1,
  });

const validateSpec = (spec: ChestSpec): readonly ConfigIssue[] =>
  typeof spec.danceLiveliness === "number" && Number.isFinite(spec.danceLiveliness) && spec.danceLiveliness >= 0 && spec.danceLiveliness <= 1
    ? []
    : [{ message: "danceLiveliness must be a finite number in [0, 1]", path: "gameSpecific.danceLiveliness" }];

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<ChestSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    // The commit beat carries the chest's spiral into its hero framing, so it
    // runs for the length of that flight rather than the shared default pause.
    commitPauseTicks: CHEST_TIMING.spiralTicks,
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
