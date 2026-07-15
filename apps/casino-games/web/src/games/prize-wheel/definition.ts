/*
 * definition.ts — Prize Wheel: hold to charge, release to spin. Destination
 * mechanic; segment arcs are drawn from the compiled probabilities, so the
 * wheel face IS the odds table.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { WheelSpec } from "./game.ts";
import { destinationSlotsOf, initialWheelExtra, stepWheel, wheelCues } from "./game.ts";
import { wheelResources, wheelScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<WheelSpec> =>
  baseConfig(
    "prize-wheel",
    "Prize Wheel",
    "showcase",
    {
      segments: [
        { label: "Star Token", mass: 3, tierId: "common" },
        { label: "Spin Again", mass: 3, tierId: null },
        { label: "Ticket Bundle", mass: 2, tierId: "uncommon" },
        { label: "Spin Again", mass: 3, tierId: null },
        { label: "Star Token", mass: 3, tierId: "common" },
        { label: "Gem Trophy", mass: 1, tierId: "rare" },
        { label: "Spin Again", mass: 2, tierId: null },
        { label: "Golden Capsule", mass: 0.4, tierId: "jackpot" },
      ],
    },
    { targetWinRate: 0.45 },
  );

const validateSpec = (spec: WheelSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (!Array.isArray(spec.segments) || spec.segments.length < 3 || spec.segments.length > 16) {
    return [{ message: "segments must be an array of 3–16 entries", path: "gameSpecific.segments" }];
  }
  spec.segments.forEach((segment, i) => {
    if (!Number.isFinite(segment.mass) || segment.mass <= 0) {
      issues.push({ message: "segment mass must be a finite number > 0", path: `gameSpecific.segments[${i}].mass` });
    }
  });
  if (!spec.segments.some((segment) => segment.tierId === null)) {
    issues.push({ message: "the wheel needs at least one non-winning segment", path: "gameSpecific.segments" });
  }
  if (!spec.segments.some((segment) => segment.tierId !== null)) {
    issues.push({ message: "the wheel needs at least one winning segment", path: "gameSpecific.segments" });
  }
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<WheelSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialWheelExtra,
    instructionOf: (state) =>
      state.session.phase === "ready"
        ? state.extra.chargeTicks > 0
          ? "Release to spin!"
          : "Hold SPACE (or press the wheel) to charge, release to spin"
        : null,
    mechanic: { kind: "destination", slots: destinationSlotsOf(runtime.config.gameSpecific) },
    resources: wheelResources(runtime),
    sound: (prev, next) => wheelCues(runtime.config.gameSpecific, prev, next),
    step: (state, input, ctx) => stepWheel(runtime, state, input, ctx),
    viewScene: (state) => wheelScene(runtime, state),
  });

export const PRIZE_WHEEL: CasinoGameDefinition<WheelSpec> = {
  categories: ["physical"],
  defaultConfig,
  displayName: "Prize Wheel",
  id: "prize-wheel",
  instruction: "Charge the launch and release — the pointer decides.",
  interaction: "hold + release",
  machineInterior: false,
  mechanic: "destination",
  mount: mount as CasinoGameDefinition<WheelSpec>["mount"],
  renderMode: "2d",
  shortDescription: "A carnival wheel whose painted arcs are exactly its odds. Charge the spin, ride the ticks.",
  thumbnail: { accent: "#ff8a66", bottom: "#3d7ecc", glyph: "wheel", top: "#ffe9a3" },
  validateSpec,
};
