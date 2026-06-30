/*
 * The screen-space UI/HUD seam of the installed presentation channel (SPEC-09 §4.2).
 * It is split out of `host-binding.ts` — `HostBridge` extends `UiBridge`, and the
 * inert `UNBOUND_HOST` composes `UNBOUND_UI` — for the same reason `draw2d-binding.ts`
 * is: the UI surface is a whole subsystem (immediate-mode rect/text/sprite/button,
 * the per-frame viewport + pointer, the draw-list readback, and the flex `solveLayout`)
 * that would otherwise push the host channel past its file budget.
 *
 * Every verb forwards (through the bound channel) to the Wave-2 `ui*` wasm exports
 * (`apps/axiom-game-runtime/src/ui.rs`: `uiBeginFrame`/`uiRect`/`uiText`/`uiSprite`/
 * `uiButton`/`uiViewport`/`uiDrawList`/`uiSolveLayout`) — the engine's
 * `axiom_interface::UiSurface` owns the draw list + button hit-testing and
 * `axiom_layout::solve` owns the flex math; nothing is computed in TS. The author
 * reaches these through the `Ui` facade (`ui.ts`), only from `onRender` (SPEC-09 §6
 * presentation; no UI value re-enters sim).
 *
 * The style records REUSE SPEC-04's `ShapeStyle` (`draw2d-binding.ts`) for the
 * packed `fill` rather than minting a parallel fill type — `UiStyle` only adds the
 * `stroke`/`strokeWidth` the `uiRect`/`uiButton` exports carry beyond it. `ShapeStyle`'s
 * `layer`/`alpha` model SPEC-09's `Common`, but the Wave-2 `uiRect`/`uiButton` exports
 * carry only `fill`/`stroke`/`strokeWidth`, so `layer`/`alpha` are dropped at the edge
 * (a documented partial of the kind `wasm-host.ts` records for each host group).
 */

import type { Handle, Rect, Rgba, Vec2 } from "./vocabulary.ts";
import type { ShapeStyle } from "./draw2d-binding.ts";

/** The logical screen-space viewport (SPEC-09 §5 `UiViewport`), fed per frame. */
export interface UiViewport {
  /** The logical screen width. */
  readonly width: number;
  /** The logical screen height. */
  readonly height: number;
}

/*
 * A UI fill+stroke style (SPEC-09 §4.2 `FillStroke & Common`). Extends SPEC-04's
 * `ShapeStyle` — reusing its packed `fill` (and `layer`/`alpha` `Common`) — and adds
 * the optional `stroke`/`strokeWidth` the `uiRect`/`uiButton` exports carry. `stroke`
 * defaults transparent and `strokeWidth` to 0 at the edge when omitted (the audio-style
 * host-side defaulting).
 */
export interface UiStyle extends ShapeStyle {
  /** The outline colour (default: transparent — no stroke). */
  readonly stroke?: Rgba;
  /** The outline width (default: 0). */
  readonly strokeWidth?: number;
}

/** Text-run options (SPEC-09 §4.2 `TextOpts`): screen position, fill colour, and pixel size. */
export interface UiTextOpts {
  /** The text origin x (screen space, top-left origin, +y down). */
  readonly x: number;
  /** The text origin y. */
  readonly y: number;
  /** The fill colour. */
  readonly color: Rgba;
  /** The text size in logical pixels. */
  readonly size: number;
}

/** The screen-space UI channel (SPEC-09 §4.2): immediate-mode draws + the flex solver. */
export interface UiBridge {
  /** Install this frame's `viewport` + `pointer` snapshot and clear the draw log (`uiBeginFrame`). */
  readonly uiBeginFrame: (viewport: UiViewport, pointer: Vec2, pressed: boolean) => void;
  /** Draw a filled/stroked rectangle (`uiRect`). */
  readonly uiRect: (bounds: Rect, style: UiStyle) => void;
  /** Draw a run of text (`uiText`). */
  readonly uiText: (value: string, opts: UiTextOpts) => void;
  /** Draw a textured sprite over `bounds` (`uiSprite`). */
  readonly uiSprite: (texture: Handle, bounds: Rect) => void;
  /** Draw an immediate-mode button; return whether it activated this frame (`uiButton`). */
  readonly uiButton: (bounds: Rect, label: string, style: UiStyle) => boolean;
  /** This frame's installed viewport (`uiViewport`). */
  readonly uiViewport: () => UiViewport;
  /** This frame's accumulated screen-space draw log as bytes (`uiDrawList`). */
  readonly uiDrawList: () => Uint8Array;
  /** Solve a flex layout: the flat node table → each node's `[x, y, w, h]` rect, in input order (`uiSolveLayout`). */
  readonly uiSolveLayout: (viewport: UiViewport, nodes: readonly number[]) => readonly number[];
}

/** The inert viewport an unbound `uiViewport` read returns before a host binds. */
const ZERO_VIEWPORT: UiViewport = { height: 0, width: 0 };

/** The activation an unbound `uiButton` reports — never activated before a host binds. */
const NOT_ACTIVATED = false;

/*
 * The inert UI surface used before `bindNative`: every draw is a no-op, every read
 * a neutral total value. Composed onto `UNBOUND_HOST` so the `Ui` facade stays total
 * (no `null` channel to branch on) and is a quiet no-op until the app binds it.
 */
export const UNBOUND_UI: UiBridge = {
  uiBeginFrame: (): void => {
    // No-op until a host is bound
  },
  uiButton: (): boolean => NOT_ACTIVATED,
  uiDrawList: (): Uint8Array => new Uint8Array(),
  uiRect: (): void => {
    // No-op until a host is bound
  },
  uiSolveLayout: (): readonly number[] => [],
  uiSprite: (): void => {
    // No-op until a host is bound
  },
  uiText: (): void => {
    // No-op until a host is bound
  },
  uiViewport: (): UiViewport => ZERO_VIEWPORT,
};
