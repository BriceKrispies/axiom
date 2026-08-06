/*
 * casino-mount.ts — the thin IMPURE shell that runs the shared round fold
 * (`round-state.ts`) inside @axiom/web-engine's `runGame`. This file owns the
 * only engine effects a game mount needs: the render loop (pointer lock off —
 * these are cursor games), volume-scaled tone playback, the celebration
 * camera shake wrap, and the per-frame HUD report to the DOM shell. Every
 * rule of the fairness contract lives in the pure fold, not here.
 */

import type { BackendChoice, RenderQuality, Scene, Tier, ToneSpec, ViewContext } from "@axiom/web-engine";
import {
  MITER_LIMIT,
  clampRenderQuality,
  detectTierSync,
  latestDetection,
  rank,
  rendererBackendName,
  resolveBackingSize,
  runGame,
} from "@axiom/web-engine";
import type { CasinoHud, GameRuntime, RunningCasinoGame } from "../chance-engine/registry/definition.ts";
import { cameraShakeOffset } from "../presentation/cameras/presets.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH } from "../presentation/cameras/picking.ts";
import { commitCue, tryAgainCue, winCue } from "../presentation/audio/cues.ts";
import type { CasinoMountSpec, CasinoState } from "./round-state.ts";
import { celebrationFor, COMMON_ACTIONS, foldRoundTick, freshRoundState, hudOf, outcomeRarity } from "./round-state.ts";

/** A transparent Canvas2D layer that exactly covers the game canvas, for a game's
 * optional flat 2D overlay (the stylized water surface). Absent unless the game
 * provides an `overlay`.
 *
 * A game DRAWS on it in the shared logical 960×600 space, so its coordinates are
 * exactly what `worldToCanvas` returns. Its BACKING STORE, though, is resolved
 * from the same quality as the 3D canvas: this layer carries the pond rim, the
 * ripple net and the shoreline — long, shallow curves, the most aliasing-prone
 * marks on the screen — and pinning it to 960×600 while the scene behind it drew
 * at the display's real resolution would leave the water the one visibly jagged
 * thing in the frame. The logical-to-backing scale is applied to the context as
 * a whole transform each frame with `setTransform`, which REPLACES rather than
 * multiplies, so it cannot compound however many times the layer is resized. */
const attachOverlay = (
  canvas: HTMLCanvasElement,
  quality: RenderQuality,
): { readonly ctx: CanvasRenderingContext2D; readonly remove: () => void } | null => {
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
  const syncBacking = (): void => {
    const rect = canvas.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) {
      return;
    }
    const size = resolveBackingSize({
      cssHeight: rect.height,
      cssWidth: rect.width,
      deviceRatio: window.devicePixelRatio,
      quality,
    });
    if (layer.width === size.width && layer.height === size.height) {
      return;
    }
    layer.width = size.width;
    layer.height = size.height;
  };
  const observer = new ResizeObserver(syncBacking);
  observer.observe(canvas);
  parent.append(layer);
  syncBacking();
  return {
    ctx,
    remove: (): void => {
      observer.disconnect();
      layer.remove();
    },
  };
};

export type { CasinoMountSpec, CasinoState } from "./round-state.ts";
export { celebrationFor, COMMON_ACTIONS, outcomeRarity, speedTicks } from "./round-state.ts";

/*
 * ── rasterization quality is resolved PER TIER ────────────────────────────────
 *
 * A game's configured `renderQuality` is its SOFTWARE baseline. It has to be:
 * the Canvas2D rasterizer costs very nearly one unit of time per backing pixel,
 * so a game that wants to stay playable on the software path picks a
 * conservative `renderScale` (the chest game pins 0.5 with `fixed-1x`, which is
 * the resolution the engine drew at back when that was hard-coded).
 *
 * The HARDWARE path has no reason to wear that number. On WebGL2 the cost of a
 * backing pixel is a rounding error next to the cost of a draw call, and drawing
 * at half the canvas and letting the browser stretch the result is exactly what
 * makes every diagonal in the scene stair-step. The game's own config comment
 * says as much — "the rung worth reaching for is 1.0, which removes the upscale
 * entirely" — and then hands 0.5 to both backends anyway, because there was
 * nowhere to say "0.5 on software, native on hardware."
 *
 * This is that place. It is the right one: `casino-mount.ts` is the app's impure
 * mount shell and already the single site where quality is validated, and the
 * tier is a property of the MACHINE, not of the game's configuration — so it
 * must not become a twentieth field in the config schema that every operator
 * panel and stored config has to carry.
 *
 * The split is strictly one-directional: the software tiers get exactly what the
 * config asked for, byte for byte. Nothing here can make the Canvas2D path draw
 * a single extra sample, which is what keeps
 * `treasure-chest-pick/frame-rate.test.ts` (398 nodes/frame, 137k samples) green
 * by construction rather than by luck.
 */

/** The best tier that still rasterizes in SOFTWARE. `webgl1` maps to the Canvas2D
 * backend (the GL backend needs GLSL 300 es), so the hardware split has to key on
 * webgl2-or-better — not on "anything that isn't canvas2d". */
const SOFTWARE_CEILING: Tier = "webgl1";

/** Backing resolution the hardware path is allowed to reach: native 1:1 with the
 * canvas's CSS box, so the blit stops being an upscale. */
const HARDWARE_RENDER_SCALE = 1;

/** Which tier this mount is about to land on, resolved BEFORE `runGame` builds the
 * backend (the quality is read once, at construction).
 *
 * An explicit `?backend=` choice is taken at its word — that is what forcing a
 * rung means. Otherwise the probed ladder decides, and `latestDetection()` is
 * preferred so this shares the one probe `initRenderer("auto")` will itself reuse
 * rather than painting the ladder's test patterns twice per mount. */
const mountTier = (choice: BackendChoice | undefined): Tier =>
  choice === undefined || choice === "auto"
    ? (latestDetection() ?? detectTierSync()).tier
    : choice === "css"
      ? "css3d"
      : choice;

/** The configured quality on a software tier; the same quality lifted to native
 * resolution on a hardware one. `rank` is ascending-by-capability-loss (webgpu 0 …
 * css3d 4), so "at least as good as webgl2" is `rank(tier) < rank("webgl1")`. */
const qualityForTier = (configured: RenderQuality, tier: Tier): RenderQuality =>
  rank(tier) < rank(SOFTWARE_CEILING)
    ? clampRenderQuality({
        ...configured,
        // Follow the display, bounded by the config's own `maxPixelRatio` — a
        // HiDPI player gets the sharpness their panel can show, and the engine's
        // `maxSamples` still bounds the maximised-window case.
        pixelRatioMode: "capped-device",
        renderScale: Math.max(configured.renderScale, HARDWARE_RENDER_SCALE),
      })
    : configured;

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

  // Rasterization quality: validated once here, then read by the renderer and by
  // the 2D overlay layer so both surfaces sample at the same rate. A game that
  // sets nothing gets the engine default. Nothing below this line feeds the fold,
  // the seed, or the result source — quality cannot reach an outcome.
  //
  // The configured value is the SOFTWARE baseline; a hardware tier lifts it to
  // native resolution. See the `qualityForTier` note above — the software path is
  // guaranteed to get exactly what the config asked for.
  const quality = qualityForTier(clampRenderQuality(runtime.config.renderQuality), mountTier(runtime.backend));

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
    overlay = rendererBackendName() === "CSS3D" ? null : attachOverlay(canvas, quality);
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
          // One whole transform, logical 960×600 → this layer's backing store.
          // `setTransform` REPLACES the matrix, so re-applying it every frame is
          // idempotent by construction — there is no state to accumulate.
          const layer = overlay.ctx.canvas;
          overlay.ctx.setTransform(layer.width / CANVAS_WIDTH, 0, 0, layer.height / CANVAS_HEIGHT, 0, 0);
          overlay.ctx.lineJoin = quality.lineJoin;
          overlay.ctx.lineCap = quality.lineCap;
          overlay.ctx.miterLimit = MITER_LIMIT;
          overlay.ctx.clearRect(0, 0, CANVAS_WIDTH, CANVAS_HEIGHT);
          drawOverlay(state, overlay.ctx, viewCtx);
        }
      },
      pointerLock: false,
      quality,
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
