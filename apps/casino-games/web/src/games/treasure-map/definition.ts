/*
 * definition.ts — Treasure Map: an illustrated island seen from above, six
 * X-marked dig sites, one pick. Choice-population mechanic: every site's
 * contents are preassigned before the round starts; the dig ceremony only
 * unearths what was already buried.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { shimmerCue, thumpCue } from "../../presentation/audio/cues.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { MapSpec } from "./game.ts";
import { initialMapExtra, mapChoiceCountOf, mapCues, MAP_DEFAULT_CHOICES, stepMap } from "./game.ts";
import { MAP_RESOURCES, mapScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<MapSpec> =>
  baseConfig(
    "treasure-map",
    "Treasure Map",
    "tabletop",
    { compassLiveliness: 0.6, markerPulse: 0.6 },
    { choiceCount: MAP_DEFAULT_CHOICES },
  );

const unitIssue = (value: unknown, path: string, label: string): readonly ConfigIssue[] =>
  typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1
    ? []
    : [{ message: `${label} must be a finite number in [0, 1]`, path }];

const validateSpec = (spec: MapSpec): readonly ConfigIssue[] => [
  ...unitIssue(spec.compassLiveliness, "gameSpecific.compassLiveliness", "compassLiveliness"),
  ...unitIssue(spec.markerPulse, "gameSpecific.markerPulse", "markerPulse"),
];

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<MapSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialMapExtra,
    instructionOf: (state) =>
      state.session.phase === "ready" ? "Pick an X to dig — arrows + Enter, or click a marker" : null,
    mechanic: { choiceCount: mapChoiceCountOf(runtime.config.choiceCount), kind: "choice" },
    resources: MAP_RESOURCES,
    sound: (prev, next) => mapCues(prev, next, runtime.settings.reducedMotion, thumpCue, shimmerCue),
    step: (state, input, ctx) => stepMap(runtime, state, input, ctx),
    viewScene: (state) => mapScene(runtime, state),
  });

export const TREASURE_MAP: CasinoGameDefinition<MapSpec> = {
  categories: ["choice"],
  defaultConfig,
  displayName: "Treasure Map",
  id: "treasure-map",
  instruction: "Choose an X on the island map — the crew digs up whatever was already buried there.",
  interaction: "pick a dig site",
  machineInterior: false,
  mechanic: "choice-population",
  mount: mount as CasinoGameDefinition<MapSpec>["mount"],
  renderMode: "2d",
  shortDescription: "An illustrated island, six red X's, one shovel — the treasure was buried before you ever picked.",
  thumbnail: { accent: "#ff5a4d", bottom: "#3fa66f", glyph: "map", top: "#ffe9c4" },
  validateSpec,
};
