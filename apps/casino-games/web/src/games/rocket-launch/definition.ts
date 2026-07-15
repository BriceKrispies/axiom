/*
 * definition.ts — Rocket Launch: hold to charge the countdown, release to fly.
 * Destination mechanic; the destination planets are the slots and the winning
 * planet is committed when the countdown fires.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { RocketSpec } from "./game.ts";
import { destinationSlotsOf, initialRocketExtra, stepRocket } from "./game.ts";
import { rocketResources, rocketScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<RocketSpec> =>
  baseConfig(
    "rocket-launch",
    "Rocket Launch",
    "showcase",
    {
      planets: [
        { label: "Gray Moon", mass: 3, tierId: null },
        { label: "Star Token", mass: 3, tierId: "common" },
        { label: "Ticket Bundle", mass: 2, tierId: "uncommon" },
        { label: "Gem Trophy", mass: 1, tierId: "rare" },
        { label: "Golden Capsule", mass: 0.4, tierId: "jackpot" },
      ],
    },
    { targetWinRate: 0.4 },
  );

const validateSpec = (spec: RocketSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (!Array.isArray(spec.planets) || spec.planets.length < 3 || spec.planets.length > 9) {
    return [{ message: "planets must be an array of 3–9 entries", path: "gameSpecific.planets" }];
  }
  spec.planets.forEach((planet, i) => {
    if (!Number.isFinite(planet.mass) || planet.mass <= 0) {
      issues.push({ message: "planet mass must be a finite number > 0", path: `gameSpecific.planets[${i}].mass` });
    }
  });
  if (!spec.planets.some((planet) => planet.tierId === null)) {
    issues.push({ message: "the sky needs at least one non-winning moon", path: "gameSpecific.planets" });
  }
  if (!spec.planets.some((planet) => planet.tierId !== null)) {
    issues.push({ message: "the sky needs at least one winning planet", path: "gameSpecific.planets" });
  }
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<RocketSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialRocketExtra,
    instructionOf: (state) =>
      state.session.phase === "ready"
        ? state.extra.chargeTicks > 0
          ? "Release to launch!"
          : "Hold SPACE (or press) to charge the countdown, release to fly"
        : null,
    mechanic: { kind: "destination", slots: destinationSlotsOf(runtime.config.gameSpecific) },
    resources: rocketResources(runtime),
    step: (state, input, ctx) => stepRocket(runtime, state, input, ctx),
    viewScene: (state) => rocketScene(runtime, state),
  });

export const ROCKET_LAUNCH: CasinoGameDefinition<RocketSpec> = {
  categories: ["physical", "reveal"],
  defaultConfig,
  displayName: "Rocket Launch",
  id: "rocket-launch",
  instruction: "Hold to build the countdown, release to launch — the rocket docks with one planet.",
  interaction: "hold + release",
  machineInterior: false,
  mechanic: "destination",
  mount: mount as CasinoGameDefinition<RocketSpec>["mount"],
  renderMode: "3d",
  shortDescription: "Charge the countdown and fly a toy rocket to a ring of prize planets. It docks with exactly one.",
  thumbnail: { accent: "#ff8a5a", bottom: "#0f1428", glyph: "rocket", top: "#8fd0ff" },
  validateSpec,
};
