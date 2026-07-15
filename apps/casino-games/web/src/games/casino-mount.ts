/*
 * casino-mount.ts — the thin IMPURE shell that runs the shared round fold
 * (`round-state.ts`) inside @axiom/web-engine's `runGame`. This file owns the
 * only engine effects a game mount needs: the render loop (pointer lock off —
 * these are cursor games), volume-scaled tone playback, the celebration
 * camera shake wrap, and the per-frame HUD report to the DOM shell. Every
 * rule of the fairness contract lives in the pure fold, not here.
 */

import type { Scene, ToneSpec, ViewContext } from "@axiom/web-engine";
import { runGame } from "@axiom/web-engine";
import type { CasinoHud, GameRuntime, RunningCasinoGame } from "../chance-engine/registry/definition.ts";
import { cameraShakeOffset } from "../presentation/cameras/presets.ts";
import { commitCue, tryAgainCue, winCue } from "../presentation/audio/cues.ts";
import type { CasinoMountSpec, CasinoState } from "./round-state.ts";
import { celebrationFor, COMMON_ACTIONS, foldRoundTick, freshRoundState, hudOf, outcomeRarity } from "./round-state.ts";

export type { CasinoMountSpec, CasinoState } from "./round-state.ts";
export { celebrationFor, COMMON_ACTIONS, outcomeRarity, speedTicks } from "./round-state.ts";

/** Mount one game on `canvas` under `runtime`. */
export const mountCasinoGame = <TSpec, TExtra>(
  canvas: HTMLCanvasElement,
  runtime: GameRuntime<TSpec>,
  spec: CasinoMountSpec<TExtra>,
): RunningCasinoGame => {
  const env = {
    config: runtime.config,
    seed: runtime.seed,
    settings: runtime.settings,
    source: runtime.source,
  };

  const view = (state: CasinoState<TExtra>, ctx: ViewContext): Scene => {
    const scene = spec.viewScene(state, ctx);
    const session = state.session;
    if (session.phase === "celebrating" && session.committed !== null) {
      const profile = celebrationFor(runtime.settings, session);
      if (profile.shake > 0) {
        return {
          ...scene,
          camera: cameraShakeOffset(scene.camera, session.committed.presentationSeed, session.tick, profile.shake),
        };
      }
    }
    return scene;
  };

  const volume = runtime.settings.masterVolume * runtime.settings.sfxVolume;
  const scaled = (tones: readonly ToneSpec[]): readonly ToneSpec[] =>
    volume <= 0 ? [] : tones.map((tone) => ({ ...tone, volume: (tone.volume ?? 0.15) * volume }));

  const sound = (prev: CasinoState<TExtra>, next: CasinoState<TExtra>): readonly ToneSpec[] => {
    const cues: ToneSpec[] = [];
    const a = prev.session.phase;
    const b = next.session.phase;
    const seed = next.session.committed?.presentationSeed ?? next.session.seed;
    if (a !== b && b === "committing") {
      cues.push(...commitCue(seed, next.session.round));
    }
    if (a !== b && b === "celebrating") {
      const rarity = outcomeRarity(next.session);
      cues.push(...(rarity === "loss" ? tryAgainCue(seed) : winCue(rarity, seed)));
    }
    cues.push(...(spec.sound?.(prev, next) ?? []));
    return scaled(cues);
  };

  const running = runGame<CasinoState<TExtra>>(
    canvas,
    {
      actions: { ...COMMON_ACTIONS, ...spec.actions },
      init: () => freshRoundState(env, spec, runtime.round, false),
      resources: spec.resources,
      sound,
      update: (state, input, ctx) => foldRoundTick(env, spec, state, input, ctx),
      view,
    },
    {
      backend: runtime.backend,
      fixedHz: 60,
      freezeAtTick: runtime.freezeAtTick,
      now: runtime.pinnedNowMs === undefined ? undefined : (): number => runtime.pinnedNowMs as number,
      onFrame: (state): void => runtime.onHud(hudOf(spec, runtime.source.kind, state)),
      pointerLock: false,
      script: runtime.script,
      seed: runtime.seed,
    },
  );

  return {
    input: running.input,
    readHud: (): CasinoHud => hudOf(spec, runtime.source.kind, running.getState()),
    stop: running.stop,
  };
};
