/*
 * definition.ts — Gem Mine: a cluster of rough rocks, one pickaxe, one pick.
 * Choice-population mechanic: which rocks hold gems is fixed before the player
 * chooses; the strikes only break open what was already inside.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { shimmerCue, thumpCue } from "../../presentation/audio/cues.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { MineSpec } from "./game.ts";
import { initialMineExtra, mineChoiceCountOf, mineCues, MINE_DEFAULT_CHOICES, stepMine } from "./game.ts";
import { MINE_RESOURCES, mineScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<MineSpec> =>
  baseConfig("gem-mine", "Gem Mine", "showcase", { wobbleLiveliness: 0.6 }, { choiceCount: MINE_DEFAULT_CHOICES });

const validateSpec = (spec: MineSpec): readonly ConfigIssue[] =>
  typeof spec.wobbleLiveliness === "number" &&
  Number.isFinite(spec.wobbleLiveliness) &&
  spec.wobbleLiveliness >= 0 &&
  spec.wobbleLiveliness <= 1
    ? []
    : [{ message: "wobbleLiveliness must be a finite number in [0, 1]", path: "gameSpecific.wobbleLiveliness" }];

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<MineSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialMineExtra,
    instructionOf: (state) =>
      state.session.phase === "ready" ? "Pick a rock to crack — arrows + Enter, or click one" : null,
    mechanic: { choiceCount: mineChoiceCountOf(runtime.config.choiceCount), kind: "choice" },
    resources: MINE_RESOURCES,
    sound: (prev, next) => mineCues(prev, next, runtime.settings.reducedMotion, thumpCue, shimmerCue),
    step: (state, input, ctx) => stepMine(runtime, state, input, ctx),
    viewScene: (state) => mineScene(runtime, state),
  });

export const GEM_MINE: CasinoGameDefinition<MineSpec> = {
  categories: ["choice"],
  defaultConfig,
  displayName: "Gem Mine",
  id: "gem-mine",
  instruction: "Choose a rock — swing the pickaxe and see whether a gem was hiding inside.",
  interaction: "pick a rock",
  machineInterior: false,
  mechanic: "choice-population",
  mount: mount as CasinoGameDefinition<MineSpec>["mount"],
  renderMode: "3d",
  shortDescription: "Rough rocks on a lantern-lit mine floor. Swing the pickaxe, crack one open, and hope for a gem.",
  thumbnail: { accent: "#8be9c8", bottom: "#4a4038", glyph: "gem", top: "#c9b79a" },
  validateSpec,
};
