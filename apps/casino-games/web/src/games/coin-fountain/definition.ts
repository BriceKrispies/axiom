/*
 * definition.ts — Coin Fountain: aim a token, charge the arc, toss it into the
 * basin. Single-reveal mechanic; the aim point and launch strength are
 * presentation context (they shape the arc and where it splashes), never the
 * odds — one gameplay draw commits the round as the token leaves the hand.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { FountainSpec } from "./game.ts";
import { DEFAULT_FOUNTAIN_SPEC, fountainCues, initialFountainExtra, stepFountain } from "./game.ts";
import { FOUNTAIN_RESOURCES, fountainScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<FountainSpec> =>
  baseConfig("coin-fountain", "Coin Fountain", "showcase", DEFAULT_FOUNTAIN_SPEC, { targetWinRate: 0.4 });

const validateSpec = (spec: FountainSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (!Number.isFinite(spec.basinRadius) || spec.basinRadius <= 0) {
    issues.push({ message: "basinRadius must be a finite number > 0", path: "gameSpecific.basinRadius" });
  }
  if (!Number.isFinite(spec.maxArcHeight) || spec.maxArcHeight <= 0) {
    issues.push({ message: "maxArcHeight must be a finite number > 0", path: "gameSpecific.maxArcHeight" });
  }
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<FountainSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialFountainExtra,
    instructionOf: (state) =>
      state.session.phase === "ready"
        ? state.extra.chargeTicks > 0
          ? "Release to toss!"
          : "Aim over the water, hold to charge the arc, release to toss"
        : null,
    mechanic: { kind: "single" },
    resources: FOUNTAIN_RESOURCES,
    sound: (prev, next) => fountainCues(runtime.settings.reducedMotion, prev, next),
    step: (state, input, ctx) => stepFountain(runtime, state, input, ctx),
    viewScene: (state) => fountainScene(runtime, state),
  });

export const COIN_FOUNTAIN: CasinoGameDefinition<FountainSpec> = {
  categories: ["physical"],
  defaultConfig,
  displayName: "Coin Fountain",
  id: "coin-fountain",
  instruction: "Aim, charge the arc, and toss your token into the fountain.",
  interaction: "aim + toss",
  machineInterior: false,
  mechanic: "single-reveal",
  mount: mount as CasinoGameDefinition<FountainSpec>["mount"],
  renderMode: "3d",
  shortDescription: "A pastel wishing fountain. Aim your token, charge the arc, and make a splash.",
  thumbnail: { accent: "#ffd24d", bottom: "#3f8fc0", glyph: "fountain", top: "#cdeafb" },
  validateSpec,
};
