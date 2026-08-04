/*
 * index.ts — the public surface of @axiom/web-engine. A consumer imports
 * everything it needs from this one entry point: the value contract types, the
 * retained-scene store (create meshes/materials, spawn + pose nodes, lights,
 * camera, clear color, ambient, render), the backend-selecting `initRenderer` facade, the
 * fixed-step loop, input, and the tone/ambience audio.
 *
 * The internal spine (matrix math, mesh + shading generators, the backend
 * contract) and the concrete WebGL2 / Canvas2D backends are deliberately NOT
 * re-exported: they are the engine's private machinery, reachable only through
 * the store + `initRenderer`.
 */

// ── value contract ────────────────────────────────────────────────────────────
export type {
  Camera3D,
  EngineQuat,
  EngineVec3,
  Entity,
  Handle,
  InputFrame,
  Light,
  MaterialSpec,
  MeshData,
  MeshKind,
  PointerSample,
  Rgba,
  TickInput,
  ToneSpec,
  Transform,
} from "./api.ts";

// ── pure-functional game authoring ──────────────────────────────────────────────
// Declare resources + write init/update/view (/sound) as pure functions; `runGame`
// is the imperative shell that drives them. `reconcile`/`emptyMemory` are exposed
// for tests and advanced hosts. See game.ts / run-game.ts.
export type {
  Game,
  GameResources,
  MeshRef,
  ReconcilePlan,
  ReposeOp,
  Scene,
  SceneInstance,
  SceneLabel,
  SceneLight,
  SceneMemory,
  TickContext,
  ViewContext,
} from "./game.ts";
export { emptyMemory, reconcile } from "./game.ts";
export type { RunGameOptions, RunningGame } from "./run-game.ts";
export { runGame } from "./run-game.ts";

// ── retained-scene store ────────────────────────────────────────────────────────
export {
  addLight,
  clearScene,
  createMaterial,
  createMesh,
  createMeshData,
  rendererBackendName,
  rendererNodeCount,
  renderScene,
  resizeRenderer,
  setAmbient,
  setCamera3D,
  setLabels,
  setClearColor,
  setLight,
  setNodeTransform,
  spawnRenderable,
} from "./store.ts";

// ── backend-selecting facade + capability ladder ────────────────────────────────
// `initRenderer(canvas)` runs the probed ladder (webgpu → webgl2 → webgl1 →
// canvas2d → css3d), painting a known pattern on each rung instead of trusting
// a non-null context. `detectTier()` awaits the full ladder (it is the only way
// the async WebGPU rung is probed) and caches its report, so a later
// `initRenderer(canvas, "auto")` reuses it. `rendererDetection()` hands back the
// whole report — every rung's outcome and the readback verdict — for a harness,
// a diagnostics overlay, or a test to assert on.
export type { BackendChoice } from "./renderer.ts";
export { initRenderer, rendererDetection, rendererSurface, rendererTier, rendererTierAtLeast } from "./renderer.ts";

// ── rasterization quality ───────────────────────────────────────────────────────
// How finely the scene is sampled: backing-store resolution vs the canvas's CSS
// box, supersampling, curve tessellation, and stroke shaping. Pass a `quality` to
// `runGame`; use `clampRenderQuality` to validate values coming from a stored
// config or a setup screen, and `resolveBackingSize` to size any SECOND canvas an
// app layers over the scene so it matches the renderer's sampling. See
// render-quality.ts.
export type {
  BackingSize,
  BackingSizeSpec,
  CanvasLineCap,
  CanvasLineJoin,
  PixelRatioMode,
  RenderQuality,
  RenderQualityInput,
} from "./render-quality.ts";
export {
  DEFAULT_MAX_SAMPLES,
  DEFAULT_RENDER_QUALITY,
  LINE_CAPS,
  LINE_JOINS,
  MITER_LIMIT,
  PIXEL_RATIO_MODES,
  RENDER_SCALES,
  backingSizeMatches,
  clampRenderQuality,
  resolveBackingSize,
  resolvePixelRatio,
} from "./render-quality.ts";
export type { DetectionReport, Tier, TierChoice, TierOutcome, TierProbe, TierProbes, TierSource } from "./tier.ts";
export { TIER_ORDER, chooseTier, isTier, ladderFrom, parseTierChoice, rank } from "./tier.ts";
export type { PatternVerdict, ReadbackTrust } from "./probe-pattern.ts";
export { detectTier, detectTierSync, latestDetection, resetDetection } from "./detect.ts";
export type { OverrideSource, TierOverride } from "./override.ts";
export { clearTierOverride, readTierOverride } from "./override.ts";

// ── fixed-step loop ─────────────────────────────────────────────────────────────
export type { LoopConfig } from "./raf-loop.ts";
export { startLoop } from "./raf-loop.ts";
export { FixedStepper } from "./stepper.ts";

// ── input ───────────────────────────────────────────────────────────────────────
export { InputState } from "./input.ts";
export type { DomInputOptions } from "./dom-input.ts";
export { attachDomInput } from "./dom-input.ts";

// ── audio ───────────────────────────────────────────────────────────────────────
export { playTone, setAmbienceLevel, startAmbience, stopAmbience } from "./audio.ts";

// ── text authoring ────────────────────────────────────────────────────────────────
// `text("Hello, world")` (or `axiom.text(...)`) builds an immutable Text value —
// plain/rich spans, style cascade, layout box, placement — and lays it out into
// backend-neutral glyph quads. The pure-TS counterpart of the Rust `axiom-text`.
export { axiom, text } from "./text.ts";
// The engine's built-in 5×7 bitmap font as renderable cell-run geometry — for
// apps that BUILD lettering out of meshes (no texture/text-quad primitive).
export { GLYPH_GAP, GLYPH_H, GLYPH_W, glyphRuns, textColumns, textRuns } from "./glyph-font.ts";
export type { CellRun } from "./glyph-font.ts";
export type {
  Text,
  TextAlign,
  TextBounds,
  TextContent,
  TextGlyph,
  TextLayoutInput,
  TextOptions,
  TextSpace,
  TextSpanInput,
  TextStyleInput,
  TextWrap,
} from "./text.ts";

// ── stylized water surface (Canvas2D) ─────────────────────────────────────────────────
// `drawStylizedWaterSurface(ctx, options)` paints a subtle, broken cellular
// highlight net inside a caller-supplied boundary so a flat blue region reads as
// water — a small, deterministic Canvas2D effect (base fill, hex markings, soft
// shoreline fade). Part of the Canvas2D module. See `canvas-water.ts`.
export { drawStylizedWaterSurface } from "./canvas-water.ts";
export type { StylizedWaterOptions, WaterBounds } from "./canvas-water.ts";
