/*
 * Renderer: the backend-constructing PLATFORM EDGE of the store. This is the
 * one place that turns a chosen capability TIER into a concrete drawing
 * backend, then injects it into the pure `store.ts` singleton via `initStore`.
 * Everything else — meshes, materials, nodes, lights, camera, and
 * `renderScene` — lives branchless and fully covered in `store.ts`.
 *
 * Backend selection is a probed CAPABILITY LADDER, not a null check:
 *
 *     webgpu → webgl2 → webgl1 → canvas2d → css3d
 *
 * "auto" runs that ladder (`detect.ts`), which paints a known pattern on each
 * rung and classifies what comes back, so a context that exists but renders
 * nothing — a real state on locked-down enterprise machines and in
 * remote-desktop sessions — is rejected instead of being trusted. See
 * `tier.ts` for the decision, `probe-pattern.ts` for what "correct pixels"
 * means, and `override.ts` for the `?render=` escape hatch and the crash guard.
 *
 * Tier → backend is a SEPARATE mapping from tier → capability, because the
 * engine does not have a renderer for every rung it can detect:
 *   - `webgpu`   → the WebGL2 backend. There is no WebGPU backend yet; the tier
 *                  is still detected and REPORTED honestly, because "this
 *                  machine has WebGPU" is a fact worth knowing and the ladder
 *                  above it is then already correct when a backend lands.
 *   - `webgl2`   → `backend-webgl2.ts`, the hardware path.
 *   - `webgl1`   → the Canvas2D rasterizer. The GL backend needs WebGL2 (GLSL
 *                  300 es), so a WebGL1-only machine is better served by the
 *                  software path than by a backend that cannot compile.
 *   - `canvas2d` → `backend-canvas2d.ts`, the z-buffered software rasterizer.
 *   - `css3d`    → `backend-css.ts`, the canvas-free DOM renderer. The
 *                  fail-safe: it acquires no drawing context at all.
 *
 * Explicit choices still mean exactly what they always did: `?backend=webgl2`
 * forces the hardware path and throws if it is unavailable, `canvas2d` forces
 * the software rasterizer, and `css` forces the DOM renderer.
 *
 * As a browser-API boundary this file is coverage-exempt (test-exempt.json) and
 * outside the Branchless Law — it keeps ordinary control flow.
 */

import type { RenderBackend } from "./backend.ts";
import { FALLBACK_TIER, type DetectionReport, type Tier, ladderFrom, rank } from "./tier.ts";
import { beginAttempt, confirmFirstFrame } from "./override.ts";
import { createCanvas2dBackend } from "./backend-canvas2d.ts";
import { createCssBackend } from "./backend-css.ts";
import { createWebGl2Backend } from "./backend-webgl2.ts";
import { type RenderQuality, DEFAULT_RENDER_QUALITY, backingSizeMatches, resolveBackingSize } from "./render-quality.ts";
import { detectTierSync, latestDetection } from "./detect.ts";
import { initStore } from "./store.ts";

/** Which drawing backend to use. "auto" (the default) runs the probed
 * capability ladder; a tier name forces that rung. `"css"` is the legacy
 * spelling of `"css3d"` and still works. */
export type BackendChoice = "auto" | Tier | "css";

/** How each rung is actually drawn. Several tiers share a backend — see the
 * file header for why. `null` means the backend could not be constructed. */
const TIER_BACKENDS: Readonly<Record<Tier, (canvas: HTMLCanvasElement, quality: RenderQuality) => RenderBackend | null>> = {
  canvas2d: createCanvas2dBackend,
  css3d: createCssBackend,
  webgl1: createCanvas2dBackend,
  webgl2: createWebGl2Backend,
  webgpu: createWebGl2Backend,
};

const CHOICE_TIERS: Readonly<Record<Exclude<BackendChoice, "auto">, Tier>> = {
  canvas2d: "canvas2d",
  css: "css3d",
  css3d: "css3d",
  webgl1: "webgl1",
  webgl2: "webgl2",
  webgpu: "webgpu",
};

let activeTier: Tier | undefined;
let activeReport: DetectionReport | undefined;

/** The tier the running renderer was built for, once `initRenderer` has run. */
export const rendererTier = (): Tier | undefined => activeTier;

/** The full probe report behind the current choice — what each rung answered,
 * how the control probe classified readback, and where the choice came from.
 * `undefined` when the backend was chosen explicitly rather than detected. */
export const rendererDetection = (): DetectionReport | undefined => activeReport;

/**
 * The other half of the crash guard, and the reason it needs nothing from the
 * app: the chosen backend is wrapped so that the FIRST frame it actually draws
 * clears the sentinel written before init. Every path into the renderer ends in
 * `RenderBackend.render` (the store's `renderScene` calls nothing else), so
 * "a frame was drawn" is observable here without the store knowing the crash
 * guard exists and without an app being trusted to report it.
 */
const confirmingBackend = (backend: RenderBackend, tier: Tier): RenderBackend => {
  let confirmed = false;
  return {
    dropMeshes: backend.dropMeshes,
    detailScale: backend.detailScale,
    name: backend.name,
    render: (frame): void => {
      backend.render(frame);
      if (!confirmed) {
        confirmed = true;
        confirmFirstFrame(tier);
      }
    },
    resize: backend.resize,
    surface: backend.surface,
    uploadMesh: backend.uploadMesh,
  };
};

/** Build the backend for `tier`, walking DOWN the ladder when construction
 * fails. The walk always terminates: `css3d` acquires no context and cannot
 * fail to be constructed. */
const buildFrom = (canvas: HTMLCanvasElement, tier: Tier, quality: RenderQuality): { backend: RenderBackend; tier: Tier } => {
  for (const candidate of ladderFrom(tier)) {
    const backend = TIER_BACKENDS[candidate](canvas, quality);
    if (backend) {
      return { backend, tier: candidate };
    }
  }
  return { backend: createCssBackend(canvas, quality), tier: FALLBACK_TIER };
};

const resolveAuto = (canvas: HTMLCanvasElement, quality: RenderQuality): { backend: RenderBackend; tier: Tier } => {
  const detection = latestDetection() ?? detectTierSync();
  activeReport = detection;
  beginAttempt(detection.tier);
  console.log(
    `axiom-engine: tier = ${detection.tier} via ${detection.source} (readback ${detection.readback}, ceiling ${detection.ceiling}, ${Math.round(detection.elapsedMs)}ms)`,
  );
  return buildFrom(canvas, detection.tier, quality);
};

const resolveExplicit = (
  canvas: HTMLCanvasElement,
  choice: Exclude<BackendChoice, "auto">,
  quality: RenderQuality,
): { backend: RenderBackend; tier: Tier } => {
  const tier = CHOICE_TIERS[choice];
  activeReport = undefined;
  beginAttempt(tier);
  const backend = TIER_BACKENDS[tier](canvas, quality);
  if (!backend) {
    throw new Error(`renderer: ${choice} was forced but is not available in this browser/canvas`);
  }
  return { backend, tier };
};

/**
 * The canvas's BACKING STORE, kept in step with its CSS box.
 *
 * This lives here, not in a backend, because it is a property of the CANVAS —
 * every pixel backend wants the same answer, and the canvas is this file's to
 * own (it already parks and restores it for the DOM renderer). Wiring it into
 * one backend only is how the engine ended up with a hardware path still drawing
 * at whatever `width`/`height` the app's HTML happened to hard-code, stretched to
 * fit, no matter how large the element or how dense the display.
 *
 * Only the backing store moves; the CSS box belongs to the page and is never
 * written here, so the app's logical coordinate space is untouched.
 *
 * A ResizeObserver rather than a per-frame measurement: `getBoundingClientRect`
 * forces layout, and paying that every frame to learn a number that changes when
 * the window changes would be its own performance bug. A zero-sized rect (not laid
 * out yet, `display:none`, or parked by the DOM renderer) is ignored rather than
 * acted on — shrinking the surface to 1x1 because the page was hidden is not a
 * resize, it is data loss.
 */
let backingObserver: ResizeObserver | undefined;
let clampReported = false;

const observeBackingSize = (canvas: HTMLCanvasElement, backend: RenderBackend, quality: RenderQuality): void => {
  backingObserver?.disconnect();
  backingObserver = undefined;
  // A backend that presents into its own element draws no pixels into the canvas.
  if (backend.surface !== undefined) {
    return;
  }
  const sync = (): void => {
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
    if (backingSizeMatches(canvas, size)) {
      return;
    }
    canvas.width = size.width;
    canvas.height = size.height;
    backend.resize(size.width, size.height);
    if (size.clamped && !clampReported) {
      clampReported = true;
      console.log(
        `axiom-engine: backing store clamped to ${size.width}x${size.height} (scale ${size.scale.toFixed(3)}) by the quality's sample/dimension budget`,
      );
    }
  };
  backingObserver = new ResizeObserver(sync);
  backingObserver.observe(canvas);
  sync();
};

/** Where the canvas was, while a DOM renderer has it out of the document. */
let parkedCanvas: { readonly parent: Node; readonly before: Node | null } | undefined;

const detachCanvas = (canvas: HTMLCanvasElement, presentsElsewhere: boolean): void => {
  if (!presentsElsewhere || canvas.parentNode === null) {
    return;
  }
  parkedCanvas = { before: canvas.nextSibling, parent: canvas.parentNode };
  canvas.remove();
};

const restoreCanvas = (canvas: HTMLCanvasElement): void => {
  if (parkedCanvas === undefined) {
    return;
  }
  parkedCanvas.parent.insertBefore(canvas, parkedCanvas.before);
  parkedCanvas = undefined;
};

let activeSurface: HTMLElement | undefined;

const demandSurface = (): HTMLElement => {
  if (activeSurface === undefined) {
    throw new Error("renderer: rendererSurface() called before initRenderer()");
  }
  return activeSurface;
};

/**
 * The element the active renderer presents into — the canvas for the pixel
 * backends, its own DOM layer for the CSS3D one.
 *
 * This is what input should be bound to. Binding to the canvas only ever worked
 * because every backend happened to use one; a DOM renderer has none, and its
 * scene is made of the very elements the pointer lands on.
 */
export const rendererSurface = (): HTMLElement => demandSurface();

/**
 * Initialize the singleton renderer on `canvas`. `choice` defaults to "auto",
 * which runs the probed capability ladder and falls through — all the way to
 * the context-free CSS3D renderer if it must. An explicit tier forces that rung
 * and throws when it is unavailable.
 *
 * The synchronous ladder never probes WebGPU (it cannot: the API is async). An
 * app that wants the webgpu rung reported awaits `detectTier()` first; this
 * function then reuses that report instead of detecting again.
 */
export const initRenderer = (
  canvas: HTMLCanvasElement,
  choice: BackendChoice = "auto",
  quality: RenderQuality = DEFAULT_RENDER_QUALITY,
): void => {
  // A previous mount may have taken the canvas out of the document (the DOM
  // renderer does — see below). Put it back before resolving, so re-mounting on a
  // pixel backend finds the page exactly as it was.
  restoreCanvas(canvas);
  const resolved = choice === "auto" ? resolveAuto(canvas, quality) : resolveExplicit(canvas, choice, quality);
  activeTier = resolved.tier;
  console.log(`axiom-engine: render backend = ${resolved.backend.name} (tier ${resolved.tier})`);
  initStore(confirmingBackend(resolved.backend, resolved.tier), canvas);
  activeSurface = resolved.backend.surface ?? canvas;
  // Size the canvas to its CSS box at this quality, and keep it there.
  observeBackingSize(canvas, resolved.backend, quality);
  // A backend that presents into its OWN element does not want a canvas in the
  // page at all. Leaving it would be harmless to look at and wrong in substance:
  // "no canvas" is the entire proposition of the DOM renderer, and a stray
  // 960x600 canvas element sitting under the scene quietly contradicts it. Its
  // position is remembered so the next pixel-backend mount can put it back.
  detachCanvas(canvas, resolved.backend.surface !== undefined);
};



/** True when the running tier is at or above `tier` on the ladder — for an app
 * that scales its scene to the machine it landed on. */
export const rendererTierAtLeast = (tier: Tier): boolean => activeTier !== undefined && rank(activeTier) <= rank(tier);
