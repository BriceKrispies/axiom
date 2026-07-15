/*
 * definition.ts — Safe Cracker: a cheerful toy prize-vault with three dials.
 * COMBINATION mechanic with the afterCommit "interact" hand-off — the first
 * stop press commits the exact three-symbol combination and stops dial 1; the
 * next two presses stop dials 2 and 3, each easing onto the committed symbol.
 * A matching three-of-a-kind pops the door open.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { thumpCue } from "../../presentation/audio/cues.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { SafeSpec, SafeState } from "./game.ts";
import { DEFAULT_SAFE_SPEC, DIAL_COUNT, initialSafeExtra, safeCues, safeSpace, SAFE_SYMBOLS, stepSafe, stopsMade } from "./game.ts";
import { SAFE_RESOURCES, safeScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<SafeSpec> =>
  baseConfig("safe-cracker", "Safe Cracker", "showcase", DEFAULT_SAFE_SPEC, { targetWinRate: 0.35 });

const validateSpec = (spec: SafeSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (spec.symbols !== SAFE_SYMBOLS) {
    issues.push({ message: `symbols must be exactly ${SAFE_SYMBOLS}`, path: "gameSpecific.symbols" });
  }
  if (!Array.isArray(spec.combos) || spec.combos.length === 0) {
    issues.push({ message: "combos must declare at least one winning combination", path: "gameSpecific.combos" });
    return issues;
  }
  spec.combos.forEach((combo, i) => {
    if (!Array.isArray(combo.combo) || combo.combo.length !== DIAL_COUNT) {
      issues.push({ message: `combo must list exactly ${DIAL_COUNT} symbols`, path: `gameSpecific.combos[${i}].combo` });
    } else if (combo.combo.some((s: number) => !Number.isInteger(s) || s < 0 || s >= spec.symbols)) {
      issues.push({ message: `combo symbols must be integers in 0–${spec.symbols - 1}`, path: `gameSpecific.combos[${i}].combo` });
    }
    if (typeof combo.tierId !== "string" || combo.tierId.length === 0) {
      issues.push({ message: "combo tierId must be a non-empty string", path: `gameSpecific.combos[${i}].tierId` });
    }
  });
  return issues;
};

const instructionOf = (state: SafeState): string | null => {
  if (state.session.phase === "ready") {
    return "Press SPACE (or click) to spin the dials";
  }
  if (state.session.phase === "interacting") {
    const remaining = DIAL_COUNT - stopsMade(state.extra);
    return remaining > 0 ? `Press to stop dial ${DIAL_COUNT - remaining + 1}` : "Cracking…";
  }
  return null;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<SafeSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    afterCommit: "interact",
    initExtra: initialSafeExtra,
    instructionOf,
    mechanic: { kind: "combination", space: safeSpace(runtime.config.gameSpecific) },
    resources: SAFE_RESOURCES,
    sound: (prev, next) => safeCues(prev, next, thumpCue),
    step: (state, input, ctx) => stepSafe(runtime, state, input, ctx),
    viewScene: (state) => safeScene(runtime, state),
  });

export const SAFE_CRACKER: CasinoGameDefinition<SafeSpec> = {
  categories: ["machine", "reveal"],
  defaultConfig,
  displayName: "Safe Cracker",
  id: "safe-cracker",
  instruction: "Stop each dial in turn — line up three and the vault opens.",
  interaction: "time the dials",
  machineInterior: false,
  mechanic: "combination",
  mount: mount as CasinoGameDefinition<SafeSpec>["mount"],
  renderMode: "3d",
  shortDescription: "A pastel prize-vault with three toy dials. Stop each in turn; three matching notches spring the bolts.",
  thumbnail: { accent: "#ffd54d", bottom: "#8497c9", glyph: "dial", top: "#cbb8f7" },
  validateSpec,
};
