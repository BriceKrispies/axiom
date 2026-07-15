/*
 * definition.ts — Ball Machine: a gumball-style dispenser seen from inside.
 * Single-reveal mechanic: one Bernoulli gameplay draw at `targetWinRate`
 * commits the win; the reveal agitates the chamber and rolls one ball down the
 * chute to a capsule that pops open on the committed tier.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { BallMachineSpec } from "./game.ts";
import { ballCues, initialBallExtra, stepBallMachine } from "./game.ts";
import { BALL_RESOURCES, ballScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<BallMachineSpec> =>
  baseConfig("ball-machine", "Ball Machine", "machine-interior", { agitationTicks: 70, ballCount: 14 }, { targetWinRate: 0.4 });

const inRange = (value: unknown, min: number, max: number): value is number =>
  typeof value === "number" && Number.isFinite(value) && value >= min && value <= max;

const validateSpec = (spec: BallMachineSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (!inRange(spec.ballCount, 6, 24)) {
    issues.push({ message: "ballCount must be a finite number in [6, 24]", path: "gameSpecific.ballCount" });
  }
  if (!inRange(spec.agitationTicks, 20, 240)) {
    issues.push({ message: "agitationTicks must be a finite number in [20, 240]", path: "gameSpecific.agitationTicks" });
  }
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<BallMachineSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialBallExtra,
    instructionOf: (state) => (state.session.phase === "ready" ? "Press SPACE or click the button to dispense" : null),
    mechanic: { kind: "single" },
    resources: BALL_RESOURCES,
    sound: (prev, next) => ballCues(runtime, prev, next),
    step: (state, input, ctx) => stepBallMachine(runtime, state, input, ctx),
    viewScene: (state) => ballScene(runtime, state),
  });

export const BALL_MACHINE: CasinoGameDefinition<BallMachineSpec> = {
  categories: ["machine", "reveal"],
  defaultConfig,
  displayName: "Ball Machine",
  id: "ball-machine",
  instruction: "Press the button — watch the chamber churn and one capsule roll out.",
  interaction: "press to dispense",
  machineInterior: true,
  mechanic: "single-reveal",
  mount: mount as CasinoGameDefinition<BallMachineSpec>["mount"],
  renderMode: "3d",
  shortDescription: "A gumball machine from the inside: capsules churn, one rolls to the chute, and pops open.",
  thumbnail: { accent: "#ffd24d", bottom: "#e8896f", glyph: "globe", top: "#8fd0ff" },
  validateSpec,
};
