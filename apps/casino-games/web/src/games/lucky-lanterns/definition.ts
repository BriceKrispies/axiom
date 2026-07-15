/*
 * definition.ts — Lucky Lanterns: release a paper lantern into a twilight sky
 * of tier color bands. Destination mechanic; one gameplay draw commits which
 * band the lantern belongs to before it leaves the platform. The rise and the
 * wind sway are presentation only — they can never move the committed band.
 */

import type { CasinoGameConfig } from "../../chance-engine/configuration/schema.ts";
import { baseConfig } from "../../chance-engine/configuration/schema.ts";
import type { ConfigIssue } from "../../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition, GameRuntime, RunningCasinoGame } from "../../chance-engine/registry/definition.ts";
import { mountCasinoGame } from "../casino-mount.ts";
import type { LanternSpec } from "./game.ts";
import { DEFAULT_LANTERN_BANDS, destinationSlotsOf, initialLanternExtra, lanternCues, stepLantern } from "./game.ts";
import { lanternResources, lanternScene } from "./scene.ts";

const defaultConfig = (): CasinoGameConfig<LanternSpec> =>
  baseConfig("lucky-lanterns", "Lucky Lanterns", "showcase", { bands: DEFAULT_LANTERN_BANDS }, { targetWinRate: 0.45 });

const validateSpec = (spec: LanternSpec): readonly ConfigIssue[] => {
  if (!Array.isArray(spec.bands) || spec.bands.length < 2 || spec.bands.length > 8) {
    return [{ message: "bands must be an array of 2–8 entries", path: "gameSpecific.bands" }];
  }
  const issues: ConfigIssue[] = [];
  spec.bands.forEach((band, i) => {
    if (typeof band.label !== "string" || band.label.length === 0) {
      issues.push({ message: "band label must be a non-empty string", path: `gameSpecific.bands[${i}].label` });
    }
    if (!Number.isFinite(band.mass) || band.mass <= 0) {
      issues.push({ message: "band mass must be a finite number > 0", path: `gameSpecific.bands[${i}].mass` });
    }
  });
  if (!spec.bands.some((band) => band.tierId !== null)) {
    issues.push({ message: "the sky needs at least one winning band", path: "gameSpecific.bands" });
  }
  if (!spec.bands.some((band) => band.tierId === null)) {
    issues.push({ message: "the sky needs at least one non-winning (drift away) band", path: "gameSpecific.bands" });
  }
  return issues;
};

const mount = (canvas: HTMLCanvasElement, runtime: GameRuntime<LanternSpec>): RunningCasinoGame =>
  mountCasinoGame(canvas, runtime, {
    initExtra: initialLanternExtra,
    instructionOf: (state) =>
      state.session.phase === "ready"
        ? state.extra.breathTicks > 0
          ? "Release to let it rise!"
          : "Hold SPACE (or press) for a taller breath, release to let the lantern go"
        : null,
    mechanic: { kind: "destination", slots: destinationSlotsOf(runtime.config.gameSpecific) },
    resources: lanternResources(runtime),
    sound: (prev, next) => lanternCues(runtime.settings.reducedMotion, prev, next),
    step: (state, input, ctx) => stepLantern(runtime, state, input, ctx),
    viewScene: (state) => lanternScene(runtime, state),
  });

export const LUCKY_LANTERNS: CasinoGameDefinition<LanternSpec> = {
  categories: ["reveal"],
  defaultConfig,
  displayName: "Lucky Lanterns",
  id: "lucky-lanterns",
  instruction: "Release your lantern and watch which color band it drifts into.",
  interaction: "release skyward",
  machineInterior: false,
  mechanic: "destination",
  mount: mount as CasinoGameDefinition<LanternSpec>["mount"],
  renderMode: "3d",
  shortDescription: "A twilight sky of color bands. Release a paper lantern and follow its glow upward.",
  thumbnail: { accent: "#ffcf6b", bottom: "#5a4f8a", glyph: "lantern", top: "#b9a9e0" },
  validateSpec,
};
