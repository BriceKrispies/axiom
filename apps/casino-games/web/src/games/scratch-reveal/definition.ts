/*
 * definition.ts — Scratch Reveal: a bright prize ticket with a foil mask.
 * SINGLE-REVEAL mechanic with the afterCommit "interact" hand-off — the
 * outcome is committed the instant the player starts (before any tile is
 * scratched); the player then scratches the foil to uncover the sealed symbol.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { hoverCue } from "../../presentation/audio/cues.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { ScratchSpec } from "./game.ts";
import { DEFAULT_SCRATCH_SPEC, initialScratchExtra, scratchCues, stepScratch } from "./game.ts";
import { SCRATCH_RESOURCES, scratchScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<ScratchSpec> =>
  baseConfig("scratch-reveal", "Scratch Reveal", "tabletop", DEFAULT_SCRATCH_SPEC, { targetWinRate: 0.45 });

const validateSpec = (spec: ScratchSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (!Number.isInteger(spec.columns) || spec.columns < 8 || spec.columns > 30) {
    issues.push({ message: "columns must be an integer in 8–30", path: "gameSpecific.columns" });
  }
  if (!Number.isInteger(spec.rows) || spec.rows < 6 || spec.rows > 16) {
    issues.push({ message: "rows must be an integer in 6–16", path: "gameSpecific.rows" });
  }
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<ScratchSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    afterCommit: "interact",
    initExtra: initialScratchExtra,
    instructionOf: (state) =>
      state.session.phase === "ready"
        ? "Scratch the foil — press SPACE, or drag on the card"
        : state.session.phase === "interacting"
          ? "Keep scratching to reveal your prize"
          : null,
    mechanic: { kind: "single" },
    resources: SCRATCH_RESOURCES,
    sound: (prev, next) => scratchCues(prev, next, hoverCue),
    step: (state, input, ctx) => stepScratch(runtime, state, input, ctx),
    viewScene: (state) => scratchScene(runtime, state),
  });

export const SCRATCH_REVEAL: CasinoGameDefinition<ScratchSpec> = {
  categories: ["reveal"],
  defaultConfig,
  displayName: "Scratch Reveal",
  id: "scratch-reveal",
  instruction: "Scratch away the foil to uncover the prize sealed underneath.",
  interaction: "scratch the foil",
  machineInterior: false,
  mechanic: "single-reveal",
  mount: mount as CasinoGameDefinition<ScratchSpec>["mount"],
  renderMode: "2d",
  shortDescription: "A bright prize ticket. The result is sealed before you scratch — clear the foil to see it.",
  thumbnail: { accent: "#ffd54d", bottom: "#7ec8a6", glyph: "ticket", top: "#ffe9a3" },
  validateSpec,
};
