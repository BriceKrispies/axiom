/*
 * render-quality.ts — how many samples the renderer takes, and how strokes and
 * curves are shaped. This is the RASTERIZATION policy of the engine: it decides
 * the size of the drawing surface behind a canvas's CSS box, nothing else. It is
 * pure arithmetic over plain values — no canvas, no context, no DOM — so every
 * rule below is exercisable directly, and the platform edges (`backend-canvas2d`,
 * `canvas-water`, the app's overlay layer) only apply what it resolves.
 *
 * The distinction that makes this correct: a canvas has TWO sizes. Its CSS box is
 * how big it appears; its backing store is how many pixels are actually drawn.
 * Quality changes only the second. The scene's logical coordinate system is
 * untouched, so gameplay, hit testing, layout, camera, and every deterministic
 * result are identical at every setting — the only difference is how finely the
 * same geometry is sampled.
 *
 * Why a SAMPLE BUDGET and not just a dimension cap: the Canvas2D backend is a
 * software scanline rasterizer, so its cost is very close to linear in backing
 * pixels (measured on the treasure-chest scene: 144k px -> 10.7 ms/frame, 576k ->
 * 27.4, 2.30M -> 93.7, 5.18M -> 206.7). Sizing the surface from the CSS box alone
 * would therefore make the frame rate a function of the window size, and a
 * maximized window on a HiDPI display would ask for tens of millions of samples
 * and render a slideshow. `maxSamples` is the ceiling that keeps the software path
 * real-time: quality knobs do exactly what they say up to the budget, and past it
 * the resolved scale falls back and reports `clamped` so the cause is visible
 * rather than mysterious. `maxBackingDimension` is the separate ALLOCATION guard,
 * against a single absurd dimension.
 */

import { orElse, pick, select } from "./branchless.ts";

/** How the display's device-pixel ratio feeds the backing-store scale. */
export type PixelRatioMode = "capped-device" | "device" | "fixed-1x";

/** How stroked corners are shaped (the Canvas2D `lineJoin` vocabulary). */
export type CanvasLineJoin = "bevel" | "miter" | "round";

/** How stroked ends are shaped (the Canvas2D `lineCap` vocabulary). */
export type CanvasLineCap = "butt" | "round" | "square";

/** One resolved rendering-quality configuration. */
export interface RenderQuality {
  /** Whether the device-pixel ratio is followed, capped, or ignored. */
  readonly pixelRatioMode: PixelRatioMode;
  /** The ceiling `"capped-device"` applies to the device ratio. */
  readonly maxPixelRatio: number;
  /** Supersampling factor on top of the resolved pixel ratio. */
  readonly renderScale: number;
  /**
   * Multiplier on the active backend's own facet budget for ROUND primitives
   * (see `tessellation.ts`). Relative, not absolute, so one default value means
   * "unchanged" on every backend even though the software path draws round
   * primitives at half the facet count of the GPU path.
   */
  readonly curveDetail: number;
  readonly lineJoin: CanvasLineJoin;
  readonly lineCap: CanvasLineCap;
  /** Ceiling on backing-store pixels (width x height) — see the file header. */
  readonly maxSamples: number;
  /** Ceiling on either backing-store dimension, guarding the allocation. */
  readonly maxBackingDimension: number;
}

/** Miter limit applied with `lineJoin: "miter"`. Fixed, not configurable: it
 * exists to stop a near-parallel join from firing a spike across the frame, and
 * is a safety bound rather than a look. */
export const MITER_LIMIT = 4;

/** A validated numeric field: the range it must land in, and the value a missing
 * or corrupt input falls back to. One object per field, so the fallback and the
 * bounds cannot drift apart and `DEFAULT_RENDER_QUALITY` can be built FROM them
 * rather than restating them. */
interface NumericRange {
  readonly low: number;
  readonly high: number;
  readonly fallback: number;
}

const MIN_RENDER_SCALE = 0.5;
const MAX_RENDER_SCALE = 2;
const RENDER_SCALE_THREE_QUARTER = 0.75;
const RENDER_SCALE_LOW = 1.25;
const RENDER_SCALE_MID = 1.5;

const MIN_PIXEL_RATIO = 1;
const MAX_PIXEL_RATIO = 4;
const DEFAULT_PIXEL_RATIO_CAP = 2;

/** The smallest surface worth drawing into, and the largest either dimension may
 * reach. 8192 clears the sizes a 4K display at 2x device ratio asks for while
 * staying an order of magnitude inside the browsers' own canvas limits. */
const MIN_BACKING_DIMENSION = 1;
const MAX_BACKING_DIMENSION = 8192;

/** Budget floor: a surface below this is not a rendering setting, it is a bug. */
const SAMPLE_FLOOR_EDGE = 64;

/**
 * The default sample budget: 720p-equivalent (921,600 samples).
 *
 * Chosen from the measured cost curve in the file header, not by taste. It is
 * roughly 6x the engine's previous fixed 480x300 software framebuffer, lands a
 * typical windowed canvas at or near 1:1 with its CSS box, and holds the ceiling
 * well below the multi-million-sample surfaces that make the software rasterizer
 * unusable.
 */
const DEFAULT_SAMPLE_WIDTH = 1280;
const DEFAULT_SAMPLE_HEIGHT = 720;
export const DEFAULT_MAX_SAMPLES = DEFAULT_SAMPLE_WIDTH * DEFAULT_SAMPLE_HEIGHT;

const RENDER_SCALE_RANGE: NumericRange = { fallback: 1, high: MAX_RENDER_SCALE, low: MIN_RENDER_SCALE };
const CURVE_DETAIL_RANGE: NumericRange = { fallback: 1, high: 2, low: 0.25 };
const PIXEL_RATIO_CAP_RANGE: NumericRange = { fallback: DEFAULT_PIXEL_RATIO_CAP, high: MAX_PIXEL_RATIO, low: MIN_PIXEL_RATIO };
const DEVICE_RATIO_RANGE: NumericRange = { fallback: MIN_PIXEL_RATIO, high: MAX_PIXEL_RATIO, low: MIN_PIXEL_RATIO };
const SAMPLES_RANGE: NumericRange = {
  fallback: DEFAULT_MAX_SAMPLES,
  high: MAX_BACKING_DIMENSION * MAX_BACKING_DIMENSION,
  low: SAMPLE_FLOOR_EDGE * SAMPLE_FLOOR_EDGE,
};
const BACKING_DIMENSION_RANGE: NumericRange = {
  fallback: MAX_BACKING_DIMENSION,
  high: MAX_BACKING_DIMENSION,
  low: MIN_BACKING_DIMENSION,
};

/** Every value a `pixelRatioMode` control may offer. */
export const PIXEL_RATIO_MODES: readonly PixelRatioMode[] = ["capped-device", "device", "fixed-1x"];

/** Every value a `lineJoin` control may offer. */
export const LINE_JOINS: readonly CanvasLineJoin[] = ["bevel", "miter", "round"];

/** Every value a `lineCap` control may offer. */
export const LINE_CAPS: readonly CanvasLineCap[] = ["butt", "round", "square"];

/** The supersampling ladder a setup control offers. `0.5` is the engine's
 * historical software resolution and stays reachable — a low-end machine needs a
 * rung BELOW native, not only rungs above it. */
export const RENDER_SCALES: readonly number[] = [
  MIN_RENDER_SCALE,
  RENDER_SCALE_THREE_QUARTER,
  1,
  RENDER_SCALE_LOW,
  RENDER_SCALE_MID,
  MAX_RENDER_SCALE,
];

/**
 * The engine's default quality: follow the display up to 2x, no supersampling on
 * top, backend-native curve detail, round joins and caps.
 *
 * `curveDetail: 1` means "leave the backend's own facet budget alone", so this
 * default reproduces the engine's previous tessellation exactly.
 */
export const DEFAULT_RENDER_QUALITY: RenderQuality = {
  curveDetail: CURVE_DETAIL_RANGE.fallback,
  lineCap: "round",
  lineJoin: "round",
  maxBackingDimension: BACKING_DIMENSION_RANGE.fallback,
  maxPixelRatio: PIXEL_RATIO_CAP_RANGE.fallback,
  maxSamples: SAMPLES_RANGE.fallback,
  pixelRatioMode: "capped-device",
  renderScale: RENDER_SCALE_RANGE.fallback,
};

/**
 * UNTRUSTED quality input — what a stored config, an imported JSON blob, or a
 * setup control supplies.
 *
 * Deliberately NOT `Partial<RenderQuality>`: the whole job of `clampRenderQuality`
 * is to turn unvalidated data into a `RenderQuality`, so typing its input as the
 * already-validated shape would assume the very thing it exists to establish, and
 * would force every real caller (and every test of a bad value) to lie with a
 * cast. The enum fields are plain `string` here and are narrowed by validation.
 */
export interface RenderQualityInput {
  readonly pixelRatioMode?: string;
  readonly maxPixelRatio?: number;
  readonly renderScale?: number;
  readonly curveDetail?: number;
  readonly lineJoin?: string;
  readonly lineCap?: string;
  readonly maxSamples?: number;
  readonly maxBackingDimension?: number;
}

/** `value` when it is a real number, else `fallback` (NaN and Infinity are not
 * settings; they are a corrupt config or a division that went wrong). */
const finite = (value: number | undefined, fallback: number): number => {
  const candidate = orElse(value, fallback);
  return select(Number.isFinite(candidate), candidate, fallback);
};

/** `value` clamped into `range`, falling back before clamping. */
const clampNumber = (value: number | undefined, range: NumericRange): number =>
  Math.min(range.high, Math.max(range.low, finite(value, range.fallback)));

/** `value` when it is one of `allowed`, else `fallback` — the string analogue of
 * `clampNumber`, so an unknown enum from stored JSON degrades instead of leaking. */
const oneOf = <Value extends string>(allowed: readonly Value[], value: string | undefined, fallback: Value): Value => {
  const found = allowed.filter((candidate): boolean => candidate === value);
  return pick([fallback, ...found], found.length);
};

/**
 * Validate and clamp an untrusted quality input into a complete `RenderQuality`.
 *
 * Total by construction: every field either survives validation or falls back to
 * its default, so a truncated, stale, or hand-edited config can never produce an
 * unrenderable surface.
 */
export const clampRenderQuality = (input?: RenderQualityInput): RenderQuality => {
  const given = orElse(input, DEFAULT_RENDER_QUALITY);
  return {
    curveDetail: clampNumber(given.curveDetail, CURVE_DETAIL_RANGE),
    lineCap: oneOf(LINE_CAPS, given.lineCap, DEFAULT_RENDER_QUALITY.lineCap),
    lineJoin: oneOf(LINE_JOINS, given.lineJoin, DEFAULT_RENDER_QUALITY.lineJoin),
    maxBackingDimension: clampNumber(given.maxBackingDimension, BACKING_DIMENSION_RANGE),
    maxPixelRatio: clampNumber(given.maxPixelRatio, PIXEL_RATIO_CAP_RANGE),
    maxSamples: clampNumber(given.maxSamples, SAMPLES_RANGE),
    pixelRatioMode: oneOf(PIXEL_RATIO_MODES, given.pixelRatioMode, DEFAULT_RENDER_QUALITY.pixelRatioMode),
    renderScale: clampNumber(given.renderScale, RENDER_SCALE_RANGE),
  };
};

/** One resolver per mode, dispatched by the discriminant rather than branched. */
const PIXEL_RATIO_RESOLVERS: Readonly<Record<PixelRatioMode, (deviceRatio: number, maxRatio: number) => number>> = {
  "capped-device": (deviceRatio, maxRatio): number => Math.min(deviceRatio, maxRatio),
  device: (deviceRatio): number => deviceRatio,
  "fixed-1x": (): number => 1,
};

/**
 * The device-pixel ratio this quality actually applies. `deviceRatio` is the
 * display's ratio (`window.devicePixelRatio`, or a fixed override in tests and
 * captures so a result never depends on the machine that ran it).
 */
export const resolvePixelRatio = (quality: RenderQuality, deviceRatio: number): number =>
  PIXEL_RATIO_RESOLVERS[quality.pixelRatioMode](clampNumber(deviceRatio, DEVICE_RATIO_RANGE), quality.maxPixelRatio);

/** The backing-store size a canvas should be given, and the scale that produced
 * it. `scale` is applied to the drawing context exactly once, so all drawing
 * stays in logical coordinates. */
export interface BackingSize {
  readonly width: number;
  readonly height: number;
  /** Backing pixels per logical pixel — the one transform the context takes. */
  readonly scale: number;
  /** True when a limit reduced the scale below what the quality asked for. */
  readonly clamped: boolean;
}

/** What `resolveBackingSize` needs: the canvas's CSS box, the display, the quality. */
export interface BackingSizeSpec {
  readonly cssWidth: number;
  readonly cssHeight: number;
  readonly deviceRatio: number;
  readonly quality: RenderQuality;
}

/**
 * Resolve a canvas's backing-store size from its CSS box.
 *
 * The requested scale is `resolvedPixelRatio * renderScale`; a 1500x730 CSS box
 * at ratio 2.0 and render scale 1.5 asks for 4500x2190. Two limits may then pull
 * the scale down — the per-dimension allocation guard and the sample budget —
 * and the smaller wins. Both act on the SCALE, never on one axis alone, so the
 * aspect ratio is preserved and the projection never stretches.
 */
export const resolveBackingSize = (spec: BackingSizeSpec): BackingSize => {
  const cssWidth = Math.max(MIN_BACKING_DIMENSION, finite(spec.cssWidth, MIN_BACKING_DIMENSION));
  const cssHeight = Math.max(MIN_BACKING_DIMENSION, finite(spec.cssHeight, MIN_BACKING_DIMENSION));
  const requested = resolvePixelRatio(spec.quality, spec.deviceRatio) * spec.quality.renderScale;
  const longest = Math.max(cssWidth, cssHeight) * requested;
  const dimensionLimit = Math.min(1, spec.quality.maxBackingDimension / longest);
  const sampleLimit = Math.min(1, Math.sqrt(spec.quality.maxSamples / (cssWidth * cssHeight * requested * requested)));
  const scale = requested * Math.min(dimensionLimit, sampleLimit);
  return {
    clamped: scale < requested,
    height: Math.max(MIN_BACKING_DIMENSION, Math.round(cssHeight * scale)),
    scale,
    width: Math.max(MIN_BACKING_DIMENSION, Math.round(cssWidth * scale)),
  };
};

/** Whether a canvas already has this backing size — the guard that keeps a
 * per-frame resize call from reallocating the surface every frame. */
export const backingSizeMatches = (canvas: { readonly width: number; readonly height: number }, size: BackingSize): boolean =>
  Boolean(Number(canvas.width === size.width) * Number(canvas.height === size.height));
