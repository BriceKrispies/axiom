/*
 * definition.ts — Fishing Cast: aim a reticle over a bright pond and cast.
 * Single-reveal mechanic; the region under the cast is presentation CONTEXT
 * (it chooses the reward family that surfaces at the dock), never the odds —
 * one Bernoulli draw at targetWinRate commits the win before the line lands.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { FishingSpec } from "./game.ts";
import { DEFAULT_FISHING_REGIONS, fishingCues, initialFishingExtra, stepFishing } from "./game.ts";
import { FISHING_RESOURCES, fishingScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<FishingSpec> =>
  baseConfig("fishing-cast", "Fishing Cast", "showcase", { regions: DEFAULT_FISHING_REGIONS }, { targetWinRate: 0.42 });

const validateSpec = (spec: FishingSpec): readonly ConfigIssue[] => {
  if (!Array.isArray(spec.regions) || spec.regions.length < 2 || spec.regions.length > 8) {
    return [{ message: "regions must be an array of 2–8 entries", path: "gameSpecific.regions" }];
  }
  const issues: ConfigIssue[] = [];
  spec.regions.forEach((region, i) => {
    if (typeof region.label !== "string" || region.label.length === 0) {
      issues.push({ message: "region label must be a non-empty string", path: `gameSpecific.regions[${i}].label` });
    }
    if (!Number.isFinite(region.x) || !Number.isFinite(region.z)) {
      issues.push({ message: "region x and z must be finite numbers", path: `gameSpecific.regions[${i}]` });
    }
    if (!Number.isFinite(region.radius) || region.radius <= 0) {
      issues.push({ message: "region radius must be a finite number > 0", path: `gameSpecific.regions[${i}].radius` });
    }
  });
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<FishingSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialFishingExtra,
    instructionOf: (state) =>
      state.session.phase === "ready" ? "Aim over the pond (move / arrows) and cast — Space or click" : null,
    mechanic: { kind: "single" },
    resources: FISHING_RESOURCES,
    sound: (prev, next) => fishingCues(runtime.settings.reducedMotion, prev, next),
    step: (state, input, ctx) => stepFishing(runtime, state, input, ctx),
    viewScene: (state) => fishingScene(runtime, state),
  });

export const FISHING_CAST: CasinoGameDefinition<FishingSpec> = {
  categories: ["physical", "reveal"],
  defaultConfig,
  displayName: "Fishing Cast",
  id: "fishing-cast",
  instruction: "Aim over the pond and cast — reel in whatever bites.",
  interaction: "aim + cast",
  machineInterior: false,
  mechanic: "single-reveal",
  mount: mount as CasinoGameDefinition<FishingSpec>["mount"],
  renderMode: "2d",
  shortDescription: "A bright little pond, three fishing spots, one cast. Arc the line out and see what surfaces.",
  thumbnail: { accent: "#ff8a66", bottom: "#2f9e9e", glyph: "bobber", top: "#bfeee9" },
  validateSpec,
};
