/*
 * definition.ts — Prize Drop: aim the release column, drop the token, watch it
 * bounce down the pegs into a reward slot. Destination mechanic; the slot widths
 * are drawn from the compiled probabilities, so the board face IS the odds table.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { DropSpec } from "./game.ts";
import { destinationSlotsOf, initialDropExtra, stepDrop } from "./game.ts";
import { dropResources, dropScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<DropSpec> =>
  baseConfig(
    "prize-drop",
    "Prize Drop",
    "showcase",
    {
      slots: [
        { label: "Miss", mass: 3, tierId: null },
        { label: "Star Token", mass: 3, tierId: "common" },
        { label: "Miss", mass: 2, tierId: null },
        { label: "Ticket Bundle", mass: 2, tierId: "uncommon" },
        { label: "Golden Capsule", mass: 0.4, tierId: "jackpot" },
        { label: "Gem Trophy", mass: 1, tierId: "rare" },
        { label: "Miss", mass: 3, tierId: null },
      ],
    },
    { targetWinRate: 0.45 },
  );

const validateSpec = (spec: DropSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (!Array.isArray(spec.slots) || spec.slots.length < 3 || spec.slots.length > 12) {
    return [{ message: "slots must be an array of 3–12 entries", path: "gameSpecific.slots" }];
  }
  spec.slots.forEach((slot, i) => {
    if (!Number.isFinite(slot.mass) || slot.mass <= 0) {
      issues.push({ message: "slot mass must be a finite number > 0", path: `gameSpecific.slots[${i}].mass` });
    }
  });
  if (!spec.slots.some((slot) => slot.tierId === null)) {
    issues.push({ message: "the board needs at least one non-winning slot", path: "gameSpecific.slots" });
  }
  if (!spec.slots.some((slot) => slot.tierId !== null)) {
    issues.push({ message: "the board needs at least one winning slot", path: "gameSpecific.slots" });
  }
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<DropSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialDropExtra,
    instructionOf: (state) =>
      state.session.phase === "ready" ? "Move with ←/→ or the pointer, press SPACE (or click) to drop" : null,
    mechanic: { kind: "destination", slots: destinationSlotsOf(runtime.config.gameSpecific) },
    resources: dropResources(runtime),
    step: (state, input, ctx) => stepDrop(runtime, state, input, ctx),
    viewScene: (state) => dropScene(runtime, state),
  });

export const PRIZE_DROP: CasinoGameDefinition<DropSpec> = {
  categories: ["physical"],
  defaultConfig,
  displayName: "Prize Drop",
  id: "prize-drop",
  instruction: "Aim the drop and release — the pegs carry the token to a slot.",
  interaction: "aim + drop",
  machineInterior: false,
  mechanic: "destination",
  mount: mount as CasinoGameDefinition<DropSpec>["mount"],
  renderMode: "2d",
  shortDescription: "A pachinko wall whose slot widths are exactly its odds. Aim the column, ride every peg down.",
  thumbnail: { accent: "#ffcf5a", bottom: "#2f3c58", glyph: "token", top: "#8fd0ff" },
  validateSpec,
};
