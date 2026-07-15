/*
 * definition.ts — Dice Vault: press to roll 1–3 chunky dice in front of a
 * cheerful toy vault. COMBINATION mechanic: the winning-total rules compile
 * into a combination space, the engine commits one exact combination, and the
 * dice settle onto exactly those faces; a winning total pops the vault open.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { shimmerCue, thumpCue } from "../../presentation/audio/cues.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { DiceSpec } from "./game.ts";
import { DEFAULT_DICE_SPEC, diceCues, diceSpace, initialDiceExtra, stepDice } from "./game.ts";
import { DICE_RESOURCES, diceScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<DiceSpec> =>
  baseConfig("dice-vault", "Dice Vault", "tabletop", DEFAULT_DICE_SPEC, { targetWinRate: 0.4 });

const validateSpec = (spec: DiceSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (!Number.isInteger(spec.diceCount) || spec.diceCount < 1 || spec.diceCount > 3) {
    issues.push({ message: "diceCount must be an integer in 1–3", path: "gameSpecific.diceCount" });
  }
  const combos = spec.combos;
  const hasRule =
    (Array.isArray(combos?.totals) && combos.totals.length > 0) ||
    combos?.allSameTierId !== null ||
    combos?.allMaxTierId !== null;
  if (!hasRule) {
    issues.push({ message: "combos must declare at least one winning rule", path: "gameSpecific.combos" });
  }
  (combos?.totals ?? []).forEach((rule, i) => {
    if (!Number.isInteger(rule.total) || rule.total < spec.diceCount || rule.total > spec.diceCount * 6) {
      issues.push({ message: `total must be an integer in ${spec.diceCount}–${spec.diceCount * 6}`, path: `gameSpecific.combos.totals[${i}].total` });
    }
  });
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<DiceSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialDiceExtra,
    instructionOf: (state) => (state.session.phase === "ready" ? "Press SPACE (or click) to roll the dice" : null),
    mechanic: { kind: "combination", space: diceSpace(runtime.config.gameSpecific) },
    resources: DICE_RESOURCES,
    sound: (prev, next) => diceCues(prev, next, thumpCue, shimmerCue),
    step: (state, input, ctx) => stepDice(runtime, state, input, ctx),
    viewScene: (state) => diceScene(runtime, state),
  });

export const DICE_VAULT: CasinoGameDefinition<DiceSpec> = {
  categories: ["physical", "reveal"],
  defaultConfig,
  displayName: "Dice Vault",
  id: "dice-vault",
  instruction: "Roll the dice — hit a lucky total and the vault swings open.",
  interaction: "press to roll",
  machineInterior: false,
  mechanic: "combination",
  mount: mount as CasinoGameDefinition<DiceSpec>["mount"],
  renderMode: "3d",
  shortDescription: "Chunky toy dice tumble across the felt; a lucky total pops the pastel vault and its prize.",
  thumbnail: { accent: "#ffd54d", bottom: "#7f9bd6", glyph: "dice", top: "#c8b6ff" },
  validateSpec,
};
