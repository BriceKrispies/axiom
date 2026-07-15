/*
 * definition.ts — Prize Elevator: press the button, ride the tower, watch the
 * doors open on your floor. Destination mechanic; the reward floors are the
 * destination slots and the winning floor is committed at the press.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { ElevatorSpec } from "./game.ts";
import { destinationSlotsOf, initialElevatorExtra, stepElevator } from "./game.ts";
import { elevatorResources, elevatorScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<ElevatorSpec> =>
  baseConfig(
    "prize-elevator",
    "Prize Elevator",
    "showcase",
    {
      floors: [
        { label: "Lobby", mass: 3, tierId: null },
        { label: "Workshop", mass: 3, tierId: null },
        { label: "Star Token", mass: 3, tierId: "common" },
        { label: "Ticket Bundle", mass: 2, tierId: "uncommon" },
        { label: "Gem Trophy", mass: 1, tierId: "rare" },
        { label: "Golden Capsule", mass: 0.4, tierId: "jackpot" },
      ],
    },
    { targetWinRate: 0.4 },
  );

const validateSpec = (spec: ElevatorSpec): readonly ConfigIssue[] => {
  const issues: ConfigIssue[] = [];
  if (!Array.isArray(spec.floors) || spec.floors.length < 3 || spec.floors.length > 10) {
    return [{ message: "floors must be an array of 3–10 entries", path: "gameSpecific.floors" }];
  }
  spec.floors.forEach((floor, i) => {
    if (!Number.isFinite(floor.mass) || floor.mass <= 0) {
      issues.push({ message: "floor mass must be a finite number > 0", path: `gameSpecific.floors[${i}].mass` });
    }
  });
  if (!spec.floors.some((floor) => floor.tierId === null)) {
    issues.push({ message: "the tower needs at least one non-winning floor", path: "gameSpecific.floors" });
  }
  if (!spec.floors.some((floor) => floor.tierId !== null)) {
    issues.push({ message: "the tower needs at least one winning floor", path: "gameSpecific.floors" });
  }
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<ElevatorSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialElevatorExtra,
    instructionOf: (state) => (state.session.phase === "ready" ? "Press SPACE (or click the button) to send the car up" : null),
    mechanic: { kind: "destination", slots: destinationSlotsOf(runtime.config.gameSpecific) },
    resources: elevatorResources(runtime),
    step: (state, input, ctx) => stepElevator(runtime, state, input, ctx),
    viewScene: (state) => elevatorScene(runtime, state),
  });

export const PRIZE_ELEVATOR: CasinoGameDefinition<ElevatorSpec> = {
  categories: ["physical", "reveal"],
  defaultConfig,
  displayName: "Prize Elevator",
  id: "prize-elevator",
  instruction: "Press the button and ride — the car stops on your floor and the doors open.",
  interaction: "press the button",
  machineInterior: false,
  mechanic: "destination",
  mount: mount as CasinoGameDefinition<ElevatorSpec>["mount"],
  renderMode: "3d",
  shortDescription: "A glass-fronted car climbs a prize tower and stops on a floor. The lamps count up as it rises.",
  thumbnail: { accent: "#ffd15a", bottom: "#334060", glyph: "elevator", top: "#ff9a86" },
  validateSpec,
};
