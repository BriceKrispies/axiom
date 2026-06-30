/*
 * The 2D drawing seam of the installed presentation channel (SPEC-04 §10). It is
 * split out of `host-binding.ts` — the `HostBridge` extends `Draw2dBridge`, and the
 * inert `UNBOUND_HOST` composes `UNBOUND_DRAW2D` — because the 2D surface is a whole
 * subsystem (shapes + particles + render targets) that would otherwise push the
 * host channel past its file budget. Keeping it here mirrors how `host-descriptors`
 * owns the grid/3D parameter records: one focused file per concern.
 *
 * Every verb is presentation-class, immediate-mode, and forwards (through the bound
 * channel) to the native `axiom-draw2d` builder via the Wave-2 `draw2d*` wasm
 * exports — nothing is rasterized in TS. The author calls them through `Frame`
 * (`sim.ts`), only from `onRender`; the surface never feeds sim (SPEC-04 §17.5).
 */

import type { FontSpec, Handle, Rect, Rgba, Seconds, TextureId, Vec2 } from "./vocabulary.ts";
import { pick } from "./control-flow.ts";

/*
 * A scalar-or-`[min, max]` emitter field (SPEC-04 §10.1). The contract's
 * `lifetime` / `speed` / `size` are `[min, max]` ranges from which each particle
 * draws a deterministic in-range value (native-side); a single `number` is the
 * backward-compatible degenerate range `[v, v]`. {@link rangeOf} resolves either
 * form to a `[min, max]` pair branchlessly.
 */
export type RangeOrScalar = number | readonly [number, number];

/*
 * Resolve a {@link RangeOrScalar} to its `[min, max]` pair without a branch:
 * `[value].flat()` is `[v]` for a scalar (so `min = max = v`, the degenerate
 * range) or `[min, max]` for a tuple. Reading index `0` and index `length - 1`
 * yields both endpoints for either shape — the scalar's single element answers
 * both `pick`s, the tuple's two answer one each.
 */
export const rangeOf = (value: RangeOrScalar): readonly [number, number] => {
  const flat = [value].flat();
  return [pick(flat, 0), pick(flat, flat.length - 1)];
};

/*
 * The per-shape 2D fill + stroke + layer/alpha a Wave-2.5 draw carries (SPEC-04
 * §10). `draw2dRect`/`draw2dCircle`/`draw2dEllipse` take a solid `fill` colour, an
 * optional `stroke` colour + `strokeWidth`, and a `layer`/`alpha`. The spec's
 * `shadow` and a gradient (`Paint`) fill still have no draw2d export (see SPEC-04
 * §4.2). `stroke`/`strokeWidth`/`layer`/`alpha` default host-side (the adapter),
 * exactly as the audio option records default — a transparent stroke of width 0,
 * `layer` 0, `alpha` fully opaque.
 */
export interface ShapeStyle {
  /** The solid fill colour. */
  readonly fill: Rgba;
  /** The outline colour (default: none — transparent). */
  readonly stroke?: Rgba;
  /** The outline width (default: 0 — no stroke). */
  readonly strokeWidth?: number;
  /** The explicit z-order (default: 0). */
  readonly layer?: number;
  /** The draw opacity in `[0, 1]` (default: 1). */
  readonly alpha?: number;
}

/*
 * An ellipse's radii + rotation (SPEC-04 §10), bundled into one record so the
 * `ellipse` verb stays within the SDK's ≤3-parameter budget (the contract's flat
 * `rx, ry, rotation` arguments collapse into this geometry record). `rotation`
 * defaults host-side to 0 (an axis-aligned ellipse).
 */
export interface EllipseRadii {
  /** The semi-axis along local x. */
  readonly rx: number;
  /** The semi-axis along local y. */
  readonly ry: number;
  /** The counter-clockwise rotation in radians (default: 0). */
  readonly rotation?: number;
}

/*
 * The per-line style a `draw2dLine` carries (SPEC-04 §10): a line owns its colour
 * and width directly (it has no fill/stroke split), plus the common `layer`/`alpha`
 * that default host-side.
 */
export interface LineStyle {
  /** The line colour. */
  readonly color: Rgba;
  /** The line width. */
  readonly width: number;
  /** The explicit z-order (default: 0). */
  readonly layer?: number;
  /** The draw opacity in `[0, 1]` (default: 1). */
  readonly alpha?: number;
}

/*
 * A particle-emitter recipe (SPEC-04 §10.1). `lifetimeSeconds` / `speed` / `size`
 * are each a {@link RangeOrScalar}: a `[min, max]` range from which every particle
 * draws a deterministic in-range value (native-side, §6), or a single scalar `v`
 * for the degenerate `[v, v]` range (the backward-compatible fixed-value form).
 * `gravity` / `layer` default host-side (the adapter) to no gravity / layer 0.
 */
export interface EmitterConfig {
  /** How many particles a burst spawns. */
  readonly count: number;
  /** Each particle's lifetime in seconds — a `[min, max]` range or a fixed scalar. */
  readonly lifetimeSeconds: RangeOrScalar;
  /** The initial particle speed — a `[min, max]` range or a fixed scalar. */
  readonly speed: RangeOrScalar;
  /** The emission cone half-angle (radians). */
  readonly spread: number;
  /** A constant acceleration applied each step (default: none). */
  readonly gravity?: Vec2;
  /** The particle quad size — a `[min, max]` range or a fixed scalar. */
  readonly size: RangeOrScalar;
  /** The colour at spawn. */
  readonly colorStart: Rgba;
  /** The colour at death (the particle fades between the two). */
  readonly colorEnd: Rgba;
  /** The explicit z-order (default: 0). */
  readonly layer?: number;
}

/*
 * A flip-book sprite animation (SPEC-04 §10.2): an ordered list of atlas sub-rect
 * `frames` played at `fps` frames per second. A pure value recipe the
 * `sampleAnimation` sampler reads; the frame-index math runs NATIVE-side (one
 * deterministic source of truth), never recomputed in TS.
 */
export interface SpriteAnimation {
  /** The ordered atlas sub-rects, one per animation frame. */
  readonly frames: readonly Rect[];
  /** The playback rate, in frames per second. */
  readonly fps: number;
}

/*
 * The per-sprite draw options (SPEC-04 §4.2). Placement is `pos` plus an optional
 * `rotation` (radians) and `scale`; the sprite-local `anchor` (`0..1` pivot),
 * `tint`, per-axis `flipX`/`flipY`, and atlas `source` sub-rect ride on the draw.
 * An omitted `source` means the whole texture. All optionals default host-side
 * (the adapter): unit scale, zero rotation, top-left anchor, white tint, no flip,
 * layer 0, opaque.
 */
export interface SpriteOpts {
  /** The world position the sprite is placed at. */
  readonly pos: Vec2;
  /** The clockwise rotation in radians (default: 0). */
  readonly rotation?: number;
  /** The per-axis scale (default: `{ x: 1, y: 1 }`). */
  readonly scale?: Vec2;
  /** The sprite-local pivot in `0..1` (default: `{ x: 0, y: 0 }`). */
  readonly anchor?: Vec2;
  /** A multiplicative colour tint (default: white). */
  readonly tint?: Rgba;
  /** Mirror horizontally (default: false). */
  readonly flipX?: boolean;
  /** Mirror vertically (default: false). */
  readonly flipY?: boolean;
  /** The atlas / flip-book sub-rect to sample (default: the whole texture). */
  readonly source?: Rect;
  /** The explicit z-order (default: 0). */
  readonly layer?: number;
  /** The draw opacity in `[0, 1]` (default: 1). */
  readonly alpha?: number;
}

/*
 * The per-text draw options (SPEC-04 §4.2): the world `pos`, the `font`, the glyph
 * `color`, and an optional `align`/`layer`/`alpha`. The `font.size` drives the
 * glyph size; alignment defaults to left.
 */
export interface TextOpts {
  /** The world position of the text origin (the left edge of the baseline row). */
  readonly pos: Vec2;
  /** The font to render with (its `size` drives the glyph size). */
  readonly font: FontSpec;
  /** The glyph colour. */
  readonly color: Rgba;
  /** The horizontal alignment (default: "left"). */
  readonly align?: "left" | "center" | "right";
  /** The explicit z-order (default: 0). */
  readonly layer?: number;
  /** The draw opacity in `[0, 1]` (default: 1). */
  readonly alpha?: number;
}

/** The measured extent of a text string (SPEC-04 §4.2). */
export interface TextMetrics {
  readonly width: number;
  readonly height: number;
}

/** The 2D drawing channel (SPEC-04 §10): shapes, sprites, text, particles, render targets, and the flip-book sampler. */
export interface Draw2dBridge {
  /** Set the 2D camera — world `center` + `zoom` (`draw2dCamera2d`). */
  readonly draw2dCamera2d: (center: Vec2, zoom: number) => void;
  /** Draw a filled / stroked rectangle (`draw2dRect`). */
  readonly draw2dRect: (bounds: Rect, style: ShapeStyle) => void;
  /** Draw a filled / stroked circle (`draw2dCircle`). */
  readonly draw2dCircle: (center: Vec2, radius: number, style: ShapeStyle) => void;
  /** Draw a filled / stroked (optionally rotated) ellipse (`draw2dEllipse`). */
  readonly draw2dEllipse: (center: Vec2, radii: EllipseRadii, style: ShapeStyle) => void;
  /** Draw a straight line segment of its own colour + width (`draw2dLine`). */
  readonly draw2dLine: (from: Vec2, to: Vec2, style: LineStyle) => void;
  /** Draw a textured sprite (`draw2dSprite`). */
  readonly draw2dSprite: (texture: TextureId, opts: SpriteOpts) => void;
  /** Draw a line of text in `opts.font` (`draw2dText`). */
  readonly draw2dText: (value: string, opts: TextOpts) => void;
  /** Measure `value` in `font`, returning its extent (`draw2dMeasureText`). */
  readonly draw2dMeasureText: (value: string, font: FontSpec) => TextMetrics;
  /** Register a particle emitter, returning its handle (`draw2dCreateEmitter`, §10.1). */
  readonly draw2dCreateEmitter: (config: EmitterConfig) => Handle;
  /** Spawn a burst from emitter `id` at `at` flying along `direction` (`draw2dEmit`, §10.1). */
  readonly draw2dEmit: (id: Handle, at: Vec2, direction: Vec2) => void;
  /** Step live particles by the presentation delta and append their quads (`draw2dAdvanceParticles`, §10.1). */
  readonly draw2dAdvanceParticles: (dtSeconds: number) => void;
  /** Create an off-screen render target, returning its handle (`draw2dCreateRenderTarget`, §10.3). */
  readonly draw2dCreateRenderTarget: (width: number, height: number) => Handle;
  /** Route subsequent draws into `target` (`draw2dBeginTarget`, §10.3). */
  readonly draw2dBeginTarget: (target: Handle) => void;
  /** Stop routing into a render target (`draw2dEndTarget`, §10.3). */
  readonly draw2dEndTarget: () => void;
  /** The texture handle naming `target`'s off-screen surface (`draw2dTargetTexture`, §10.3). */
  readonly draw2dTargetTexture: (target: Handle) => Handle;
  /** Finish the frame: the layer-sorted neutral command list `[kind, layer, submission, …]` (`draw2dFinish`). */
  readonly draw2dFinish: () => readonly number[];
  /** Sample a flip-book's sub-rect at presentation time `elapsedSeconds`, wrapping when `looping` else clamping to the last frame (`draw2dSampleAnimation`, §10.2). */
  readonly draw2dSampleAnimation: (anim: SpriteAnimation, elapsedSeconds: Seconds, looping: boolean) => Rect;
}

/** The handle returned by the inert handle-minting 2D reads before a host binds (a null handle). */
const UNBOUND_HANDLE = 0;

/** The inert sub-rect the unbound flip-book sampler returns (nothing to draw). */
const INERT_RECT: Rect = { height: 0, width: 0, x: 0, y: 0 };

/** The inert extent the unbound `measureText` returns before a host binds. */
const INERT_METRICS: TextMetrics = { height: 0, width: 0 };

/*
 * The inert 2D surface used before `bindNative`: every draw is a no-op and every
 * id-returning verb mints the null handle / empty list. Composed into `UNBOUND_HOST`
 * so the free surface stays total (no `null` channel to branch on).
 */
export const UNBOUND_DRAW2D: Draw2dBridge = {
  draw2dAdvanceParticles: (): void => {
    // No-op until a host is bound
  },
  draw2dBeginTarget: (): void => {
    // No-op until a host is bound
  },
  draw2dCamera2d: (): void => {
    // No-op until a host is bound
  },
  draw2dCircle: (): void => {
    // No-op until a host is bound
  },
  draw2dCreateEmitter: (): Handle => UNBOUND_HANDLE,
  draw2dCreateRenderTarget: (): Handle => UNBOUND_HANDLE,
  draw2dEllipse: (): void => {
    // No-op until a host is bound
  },
  draw2dEmit: (): void => {
    // No-op until a host is bound
  },
  draw2dEndTarget: (): void => {
    // No-op until a host is bound
  },
  draw2dFinish: (): readonly number[] => [],
  draw2dLine: (): void => {
    // No-op until a host is bound
  },
  draw2dMeasureText: (): TextMetrics => INERT_METRICS,
  draw2dRect: (): void => {
    // No-op until a host is bound
  },
  draw2dSampleAnimation: (): Rect => INERT_RECT,
  draw2dSprite: (): void => {
    // No-op until a host is bound
  },
  draw2dTargetTexture: (): Handle => UNBOUND_HANDLE,
  draw2dText: (): void => {
    // No-op until a host is bound
  },
};
