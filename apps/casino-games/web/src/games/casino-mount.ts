/*
 * casino-mount.ts — the thin IMPURE shell that runs the shared round fold
 * (`round-state.ts`) inside @axiom/web-engine's `runGame`. This file owns the
 * only engine effects a game mount needs: the render loop (pointer lock off —
 * these are cursor games), volume-scaled tone playback, the celebration
 * camera shake wrap, and the per-frame HUD report to the DOM shell. Every
 * rule of the fairness contract lives in the pure fold, not here.
 */

import type { Scene, ToneSpec, ViewContext } from "@axiom/web-engine";
import { rendererBackendName, runGame } from "@axiom/web-engine";
import type { CasinoHud, GameRuntime, RunningCasinoGame } from "../chance-engine/registry/definition.ts";
import { cameraShakeOffset } from "../presentation/cameras/presets.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH } from "../presentation/cameras/picking.ts";
import { commitCue, tryAgainCue, winCue } from "../presentation/audio/cues.ts";
import type { CasinoMountSpec, CasinoState } from "./round-state.ts";
import { celebrationFor, COMMON_ACTIONS, foldRoundTick, freshRoundState, hudOf, outcomeRarity } from "./round-state.ts";

/** A transparent Canvas2D layer that exactly covers the game canvas, for a game's
 * optional flat 2D overlay (the stylized water surface). Its backing store is the
 * shared logical 960×600 space, so a game draws in the SAME coordinates its
 * `worldToCanvas` projection returns; CSS stretches it over the 3D canvas. Absent
 * unless the game provides an `overlay`. */
const attachOverlay = (canvas: HTMLCanvasElement): { readonly ctx: CanvasRenderingContext2D; readonly remove: () => void } | null => {
  const layer = document.createElement("canvas");
  layer.width = CANVAS_WIDTH;
  layer.height = CANVAS_HEIGHT;
  layer.setAttribute("aria-hidden", "true");
  layer.style.cssText = "position:absolute;left:0;top:0;width:100%;height:100%;pointer-events:none;display:block;";
  const ctx = layer.getContext("2d");
  const parent = canvas.parentElement;
  if (ctx === null || parent === null) {
    return null;
  }
  parent.append(layer);
  return { ctx, remove: (): void => layer.remove() };
};

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

  // The optional per-frame 2D overlay layer (only games that declare `overlay`).
  //
  // Resolved LAZILY, on the first rendered frame, because it depends on which
  // backend `runGame` resolved and that has not happened yet at this point. The
  // DOM backend gets none at all: it presents into elements and keeps no canvas
  // in the page, so hanging a Canvas2D layer over it would put back the very
  // thing that renderer exists to do without.
  let overlay: { readonly ctx: CanvasRenderingContext2D; readonly remove: () => void } | null = null;
  let overlayResolved = false;
  const drawOverlay = spec.overlay;
  const resolveOverlay = (): void => {
    overlayResolved = true;
    overlay = rendererBackendName() === "CSS3D" ? null : attachOverlay(canvas);
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
      onFrame: (state, viewCtx): void => {
        runtime.onHud(hudOf(spec, runtime.source.kind, state));
        if (drawOverlay === undefined) {
          return;
        }
        if (!overlayResolved) {
          resolveOverlay();
        }
        if (overlay !== null) {
          overlay.ctx.clearRect(0, 0, CANVAS_WIDTH, CANVAS_HEIGHT);
          drawOverlay(state, overlay.ctx, viewCtx);
        }
      },
      pointerLock: false,
      script: runtime.script,
      seed: runtime.seed,
    },
  );

  return {
    input: running.input,
    readHud: (): CasinoHud => hudOf(spec, runtime.source.kind, running.getState()),
    stop: (): void => {
      running.stop();
      overlay?.remove();
    },
  };
};
