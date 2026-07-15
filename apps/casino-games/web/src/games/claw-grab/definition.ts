/*
 * definition.ts — Claw Grab: steer the claw over the prize bed and drop.
 * Single-reveal mechanic; the prize under the claw at drop time is committed as
 * the targeted focus index, and `targetWinRate` alone decides whether the grip
 * holds. The reveal descends over THAT prize and never substitutes a distant one.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { ClawGrabSpec } from "./game.ts";
import { clawCues, initialClawExtra, stepClawGrab } from "./game.ts";
import { CLAW_RESOURCES, clawScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<ClawGrabSpec> =>
  baseConfig("claw-grab", "Claw Grab", "machine-interior", { prizeCount: 7, steerSpeed: 0.06 }, { targetWinRate: 0.35 });

const inRange = (value: unknown, min: number, max: number): value is number =>
  typeof value === "number" && Number.isFinite(value) && value >= min && value <= max;

const validateSpec = (spec: ClawGrabSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (!inRange(spec.prizeCount, 4, 10)) {
    issues.push({ message: "prizeCount must be a finite number in [4, 10]", path: "gameSpecific.prizeCount" });
  }
  if (!inRange(spec.steerSpeed, 0.01, 0.3)) {
    issues.push({ message: "steerSpeed must be a finite number in [0.01, 0.3]", path: "gameSpecific.steerSpeed" });
  }
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<ClawGrabSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialClawExtra,
    instructionOf: (state) =>
      state.session.phase === "interacting" ? "Steer with arrows — press SPACE to drop the claw" : null,
    mechanic: { kind: "single" },
    resources: CLAW_RESOURCES,
    sound: (prev, next) => clawCues(runtime, prev, next),
    step: (state, input, ctx) => stepClawGrab(runtime, state, input, ctx),
    viewScene: (state) => clawScene(runtime, state),
  });

export const CLAW_GRAB: CasinoGameDefinition<ClawGrabSpec> = {
  categories: ["machine", "physical"],
  defaultConfig,
  displayName: "Claw Grab",
  id: "claw-grab",
  instruction: "Steer the claw over a prize and drop — the grip decides.",
  interaction: "steer + drop",
  machineInterior: true,
  mechanic: "single-reveal",
  mount: mount as CasinoGameDefinition<ClawGrabSpec>["mount"],
  renderMode: "3d",
  shortDescription: "A claw machine from the inside: steer over the plush bed, drop, and hope the grip holds.",
  thumbnail: { accent: "#ffd24d", bottom: "#7c5ad6", glyph: "claw", top: "#8fd0ff" },
  validateSpec,
};
