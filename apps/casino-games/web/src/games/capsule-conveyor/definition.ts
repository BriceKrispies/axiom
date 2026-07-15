/*
 * definition.ts — Capsule Conveyor: stop the looping belt and let it glide the
 * committed capsule into the opening station. Destination mechanic; each capsule
 * is a slot (mass 1) whose tier comes from `capsuleTiers`. The deceleration
 * profile carries the committed capsule to the station — continuous, never a snap.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { CapsuleConveyorSpec } from "./game.ts";
import { conveyorCues, initialConveyorExtra, slotsOf, stepCapsuleConveyor } from "./game.ts";
import { CONVEYOR_RESOURCES, conveyorScene } from "./scene.ts";

const DEFAULT_TIERS: readonly (string | null)[] = [
  "common",
  null,
  "uncommon",
  null,
  "common",
  "rare",
  null,
  "jackpot",
];

const defaultConfig = (): CasinoGameConfig<CapsuleConveyorSpec> =>
  baseConfig(
    "capsule-conveyor",
    "Capsule Conveyor",
    "machine-interior",
    { capsuleCount: 8, capsuleTiers: DEFAULT_TIERS },
    { targetWinRate: 0.45 },
  );

const validateSpec = (spec: CapsuleConveyorSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (typeof spec.capsuleCount !== "number" || !Number.isFinite(spec.capsuleCount) || spec.capsuleCount < 4 || spec.capsuleCount > 12) {
    issues.push({ message: "capsuleCount must be a finite number in [4, 12]", path: "gameSpecific.capsuleCount" });
  }
  if (!Array.isArray(spec.capsuleTiers) || spec.capsuleTiers.length !== spec.capsuleCount) {
    return [{ message: "capsuleTiers must be an array of length capsuleCount", path: "gameSpecific.capsuleTiers" }];
  }
  if (!spec.capsuleTiers.some((tier) => tier !== null)) {
    issues.push({ message: "at least one capsule must hold a prize (non-null tier)", path: "gameSpecific.capsuleTiers" });
  }
  if (!spec.capsuleTiers.some((tier) => tier === null)) {
    issues.push({ message: "at least one capsule must be empty (null tier)", path: "gameSpecific.capsuleTiers" });
  }
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<CapsuleConveyorSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialConveyorExtra,
    instructionOf: (state) => (state.session.phase === "interacting" ? "Press SPACE (or the button) to stop the belt" : null),
    mechanic: { kind: "destination", slots: slotsOf(runtime.config.gameSpecific) },
    resources: CONVEYOR_RESOURCES,
    sound: (prev, next) => conveyorCues(runtime, prev, next),
    step: (state, input, ctx) => stepCapsuleConveyor(runtime, state, input, ctx),
    viewScene: (state) => conveyorScene(runtime, state),
  });

export const CAPSULE_CONVEYOR: CasinoGameDefinition<CapsuleConveyorSpec> = {
  categories: ["machine", "physical"],
  defaultConfig,
  displayName: "Capsule Conveyor",
  id: "capsule-conveyor",
  instruction: "Stop the belt — it coasts a capsule into the station and twists it open.",
  interaction: "stop the belt",
  machineInterior: true,
  mechanic: "destination",
  mount: mount as CasinoGameDefinition<CapsuleConveyorSpec>["mount"],
  renderMode: "3d",
  shortDescription: "A capsule belt from the inside: stop it, watch it coast one capsule to the station, and pop.",
  thumbnail: { accent: "#ffd24d", bottom: "#4c8f7a", glyph: "capsule", top: "#8fd0ff" },
  validateSpec,
};
