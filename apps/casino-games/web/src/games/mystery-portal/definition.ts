/*
 * definition.ts — Mystery Portal: three glowing gateways, one step through.
 * Choice-population mechanic: every portal's destination is fixed before the
 * player chooses; walking through only reveals what was already waiting.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { shimmerCue } from "../../presentation/audio/cues.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { PortalSpec } from "./game.ts";
import { initialPortalExtra, portalChoiceCountOf, portalCues, PORTAL_DEFAULT_CHOICES, stepPortal } from "./game.ts";
import { PORTAL_RESOURCES, portalScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<PortalSpec> =>
  baseConfig(
    "mystery-portal",
    "Mystery Portal",
    "showcase",
    { shimmerLiveliness: 0.7 },
    { choiceCount: PORTAL_DEFAULT_CHOICES },
  );

const validateSpec = (spec: PortalSpec): readonly ConfigIssue[] =>
  typeof spec.shimmerLiveliness === "number" &&
  Number.isFinite(spec.shimmerLiveliness) &&
  spec.shimmerLiveliness >= 0 &&
  spec.shimmerLiveliness <= 1
    ? []
    : [{ message: "shimmerLiveliness must be a finite number in [0, 1]", path: "gameSpecific.shimmerLiveliness" }];

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<PortalSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialPortalExtra,
    instructionOf: (state) =>
      state.session.phase === "ready" ? "Choose a portal — arrows + Enter, or click one" : null,
    mechanic: { choiceCount: portalChoiceCountOf(runtime.config.choiceCount), kind: "choice" },
    resources: PORTAL_RESOURCES,
    sound: (prev, next) => portalCues(prev, next, runtime.settings.reducedMotion, shimmerCue),
    step: (state, input, ctx) => stepPortal(runtime, state, input, ctx),
    viewScene: (state) => portalScene(runtime, state),
  });

export const MYSTERY_PORTAL: CasinoGameDefinition<PortalSpec> = {
  categories: ["choice"],
  defaultConfig,
  displayName: "Mystery Portal",
  id: "mystery-portal",
  instruction: "Step through one of the glowing portals — its destination was sealed before you chose.",
  interaction: "step through one",
  machineInterior: false,
  mechanic: "choice-population",
  mount: mount as CasinoGameDefinition<PortalSpec>["mount"],
  renderMode: "3d",
  shortDescription: "Three shimmering gateways hover in a row. Pick one, fall through the light, and see where it leads.",
  thumbnail: { accent: "#8be9ff", bottom: "#5a3fa6", glyph: "portal", top: "#ffd0f0" },
  validateSpec,
};
