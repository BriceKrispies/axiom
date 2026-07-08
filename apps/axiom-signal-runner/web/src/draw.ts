/*
 * Thin drawing helpers over the SDK's immediate-mode `Frame` (draw2d): filled/stroked
 * polygons, diamonds, rounded panels, circles, lines, and monospace text. They exist
 * only to keep the world + UI renderers legible — no game logic lives here. Every
 * verb forwards straight to `frame.*`, which the engine rasterizes through its
 * WebGPU → WebGL2 → Canvas2D cascade.
 */

import type { Frame, Rgba, Vec2 } from "@axiom/game";

const CLEAR: Rgba = [0, 0, 0, 0];

/** A filled (and optionally stroked) polygon. */
export const poly = (
  frame: Frame,
  points: readonly Vec2[],
  fill: Rgba,
  layer: number,
  stroke?: Rgba,
  strokeWidth = 2,
  alpha = 1,
): void => {
  frame.path(points, { alpha, closed: true, fill, layer, stroke, strokeWidth });
};

/** An open, stroked polyline (no fill). */
export const stroke = (frame: Frame, points: readonly Vec2[], color: Rgba, width: number, layer: number, alpha = 1): void => {
  frame.path(points, { alpha, closed: false, fill: CLEAR, layer, stroke: color, strokeWidth: width });
};

/** A filled/stroked axis-aligned rectangle. */
export const box = (
  frame: Frame,
  x: number,
  y: number,
  w: number,
  h: number,
  fill: Rgba,
  layer: number,
  strokeC?: Rgba,
  strokeWidth = 2,
  alpha = 1,
): void => {
  frame.rect({ height: h, width: w, x, y }, { alpha, fill, layer, stroke: strokeC, strokeWidth });
};

/** A filled/stroked circle. */
export const disc = (
  frame: Frame,
  cx: number,
  cy: number,
  r: number,
  fill: Rgba,
  layer: number,
  strokeC?: Rgba,
  strokeWidth = 2,
  alpha = 1,
): void => {
  frame.circle({ x: cx, y: cy }, r, { alpha, fill, layer, stroke: strokeC, strokeWidth });
};

/** A straight line of its own colour + width. */
export const seg = (frame: Frame, a: Vec2, b: Vec2, color: Rgba, width: number, layer: number, alpha = 1): void => {
  frame.line(a, b, { alpha, color, layer, width });
};

/** A diamond (rotated square) — the shard / plate / minimap glyph. */
export const diamond = (
  frame: Frame,
  cx: number,
  cy: number,
  rx: number,
  ry: number,
  fill: Rgba,
  layer: number,
  strokeC?: Rgba,
  strokeWidth = 2,
  alpha = 1,
): void => {
  poly(
    frame,
    [
      { x: cx, y: cy - ry },
      { x: cx + rx, y: cy },
      { x: cx, y: cy + ry },
      { x: cx - rx, y: cy },
    ],
    fill,
    layer,
    strokeC,
    strokeWidth,
    alpha,
  );
};

/** A rounded-rectangle panel (arc-approximated corners), filled + outlined. */
export const panel = (
  frame: Frame,
  x: number,
  y: number,
  w: number,
  h: number,
  radius: number,
  fill: Rgba,
  strokeC: Rgba,
  layer: number,
  strokeWidth = 3,
  alpha = 1,
): void => {
  const r = Math.min(radius, w / 2, h / 2);
  const steps = 4;
  const corners: [number, number, number][] = [
    [x + w - r, y + r, -Math.PI / 2],
    [x + w - r, y + h - r, 0],
    [x + r, y + h - r, Math.PI / 2],
    [x + r, y + r, Math.PI],
  ];
  const pts: Vec2[] = [];
  for (const [cxq, cyq, base] of corners) {
    for (let i = 0; i <= steps; i += 1) {
      const a = base + (i / steps) * (Math.PI / 2);
      pts.push({ x: cxq + Math.cos(a) * r, y: cyq + Math.sin(a) * r });
    }
  }
  poly(frame, pts, fill, layer, strokeC, strokeWidth, alpha);
};

/** A line of monospace text (the engine's built-in font). */
export const label = (
  frame: Frame,
  value: string,
  x: number,
  y: number,
  size: number,
  color: Rgba,
  layer: number,
  align: "left" | "center" | "right" = "left",
  alpha = 1,
): void => {
  frame.text(value, { align, alpha, color, font: { family: "monospace", size }, layer, pos: { x, y } });
};

/** Monospace text width for the built-in font (advance = size * 0.5 per glyph). */
export const textWidth = (value: string, size: number): number => value.length * size * 0.5;
