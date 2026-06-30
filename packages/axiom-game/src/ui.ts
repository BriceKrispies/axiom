/*
 * The author-facing screen-space `Ui` surface (SPEC-09 §4.2) — the immediate-mode
 * HUD/overlay the author draws from `onRender`. It is surfaced exactly as `Frame`
 * (`sim.ts`): every verb reads the installed `HostBridge` at call time (the
 * presentation channel `bindNative` installs) and forwards to the Wave-2 `ui*`
 * exports, so `makeUi()` needs no bridge argument and nothing is rasterized or
 * laid out in TS. The surface is presentation-only (SPEC-09 §6): a HUD reflects
 * sim state but no `Ui` value re-enters a fixed update.
 *
 * `beginFrame`/`drawList` are the per-frame bracketing the platform edge drives
 * (install the viewport + pointer; read the accumulated draw bytes back to paint);
 * `rect`/`text`/`sprite`/`button`/`viewport` are the author's immediate-mode draws,
 * and `button` returns its activation this frame (the engine's pure
 * `(bounds, pointer)` truth table). `solveLayout` (the flex placement projection)
 * is re-exported here so the whole SPEC-09 surface lands behind one import.
 */

import type { Rect, Rgba, TextureId, Vec2 } from "./vocabulary.ts";
import type { SpriteOpts, TextOpts } from "./draw2d-binding.ts";
import type { UiStyle, UiViewport } from "./ui-binding.ts";
import { boundHost } from "./host-binding.ts";
import { orElse } from "./control-flow.ts";

/** The screen-space immediate-mode UI surface (SPEC-09 §4.2), legal only from `onRender`. */
export interface Ui {
  /** Install this frame's `viewport` + `pointer` snapshot and clear last frame's draws (the loop drives this). */
  readonly beginFrame: (viewport: UiViewport, pointer: Vec2, pressed: boolean) => void;
  /** Draw a filled/stroked rectangle (SPEC-09 §4.2). */
  readonly rect: (bounds: Rect, style: UiStyle) => void;
  /** Draw a run of text in the SPEC-04 `TextOpts` style — screen-space (SPEC-09 §4.2). */
  readonly text: (value: string, opts: TextOpts) => void;
  /** Draw a textured sprite in the SPEC-04 `SpriteOpts` style — screen-space (SPEC-09 §4.2). */
  readonly sprite: (texture: TextureId, opts: SpriteOpts) => void;
  /** Draw an immediate-mode button; return whether it was activated this frame (SPEC-09 §4.2). */
  readonly button: (bounds: Rect, label: string, style?: UiStyle) => boolean;
  /** This frame's installed viewport (SPEC-09 §4.2) — a per-frame snapshot read property. */
  readonly viewport: UiViewport;
  /** This frame's accumulated screen-space draw bytes — the loop reads these back to paint. */
  readonly drawList: () => Uint8Array;
}

/** Transparent — the fully-omitted colour used as a button's default fill. */
const TRANSPARENT: Rgba = [0, 0, 0, 0];

/** The default button style when the author passes none: a transparent fill, no stroke. */
const DEFAULT_BUTTON_STYLE: UiStyle = { fill: TRANSPARENT };

/*
 * Build the `Ui` surface. Each verb forwards to the installed presentation channel
 * (`boundHost()`), the same late-bound read `Frame`'s 2D verbs and the free
 * `sound`/`scene3d` surfaces use — so the surface is a quiet no-op until `bindNative`.
 */
export const makeUi = (): Ui => ({
  beginFrame: (viewport: UiViewport, pointer: Vec2, pressed: boolean): void => {
    boundHost().uiBeginFrame(viewport, pointer, pressed);
  },
  button: (bounds: Rect, label: string, style?: UiStyle): boolean =>
    boundHost().uiButton(bounds, label, orElse(style, DEFAULT_BUTTON_STYLE)),
  drawList: (): Uint8Array => boundHost().uiDrawList(),
  rect: (bounds: Rect, style: UiStyle): void => {
    boundHost().uiRect(bounds, style);
  },
  sprite: (texture: TextureId, opts: SpriteOpts): void => {
    boundHost().uiSprite(texture, opts);
  },
  text: (value: string, opts: TextOpts): void => {
    boundHost().uiText(value, opts);
  },
  // A readonly property (SPEC-09 §4.2 / contract §14): each read returns this frame's installed viewport — the native `uiViewport` snapshot `beginFrame` set.
  get viewport(): UiViewport {
    return boundHost().uiViewport();
  },
});
