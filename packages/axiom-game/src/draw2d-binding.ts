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

import type { Handle, Rect, Rgba, Vec2 } from "./vocabulary.ts";

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
 * A particle-emitter recipe (SPEC-04 §10.1). Wave-2's `draw2dCreateEmitter` takes a
 * single `lifetimeSeconds`/`speed`/`size` scalar (not the spec's `[min, max]`
 * ranges — the ranged form awaits a richer export), and `gravity`/`layer` default
 * host-side (the adapter) to no gravity / layer 0.
 */
export interface EmitterConfig {
  /** How many particles a burst spawns. */
  readonly count: number;
  /** Each particle's lifetime in seconds. */
  readonly lifetimeSeconds: number;
  /** The initial particle speed. */
  readonly speed: number;
  /** The emission cone half-angle (radians). */
  readonly spread: number;
  /** A constant acceleration applied each step (default: none). */
  readonly gravity?: Vec2;
  /** The particle quad size. */
  readonly size: number;
  /** The colour at spawn. */
  readonly colorStart: Rgba;
  /** The colour at death (the particle fades between the two). */
  readonly colorEnd: Rgba;
  /** The explicit z-order (default: 0). */
  readonly layer?: number;
}

/** The 2D drawing channel (SPEC-04 §10): shapes, particles, and render targets. */
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
}

/** The handle returned by the inert handle-minting 2D reads before a host binds (a null handle). */
const UNBOUND_HANDLE = 0;

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
  draw2dRect: (): void => {
    // No-op until a host is bound
  },
  draw2dTargetTexture: (): Handle => UNBOUND_HANDLE,
};
