/*
 * draw.ts — the immediate-mode 2D drawing toolkit for Arena Forge's single
 * gameplay canvas. Everything the game shows (cards as stamped forge schematics,
 * unit plaques, the HUD, buttons) is composed from these primitives each frame.
 * It is app-tier presentation code: the canvas 2D context is the required
 * Canvas2D baseline (no WebGL/WebGPU needed to play), and it is the ONLY surface
 * that touches pixels — the simulation never does.
 */

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const inRect = (r: Rect, px: number, py: number): boolean => px >= r.x && px <= r.x + r.w && py >= r.y && py <= r.y + r.h;

export const rrPath = (ctx: CanvasRenderingContext2D, r: Rect, radius: number): void => {
  const rad = Math.min(radius, r.w / 2, r.h / 2);
  ctx.beginPath();
  ctx.moveTo(r.x + rad, r.y);
  ctx.arcTo(r.x + r.w, r.y, r.x + r.w, r.y + r.h, rad);
  ctx.arcTo(r.x + r.w, r.y + r.h, r.x, r.y + r.h, rad);
  ctx.arcTo(r.x, r.y + r.h, r.x, r.y, rad);
  ctx.arcTo(r.x, r.y, r.x + r.w, r.y, rad);
  ctx.closePath();
};

export const panel = (ctx: CanvasRenderingContext2D, r: Rect, fill: string, edge: string, radius = 6): void => {
  rrPath(ctx, r, radius);
  ctx.fillStyle = fill;
  ctx.fill();
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = edge;
  ctx.stroke();
};

export type TextAlign = "left" | "center" | "right";

export const text = (
  ctx: CanvasRenderingContext2D,
  str: string,
  x: number,
  y: number,
  opts: { size?: number; color?: string; align?: TextAlign; weight?: number; family?: string; max?: number } = {},
): void => {
  ctx.fillStyle = opts.color ?? "#e8e0d4";
  ctx.font = `${opts.weight ?? 700} ${opts.size ?? 12}px ${opts.family ?? "ui-monospace, monospace"}`;
  ctx.textAlign = opts.align ?? "left";
  ctx.textBaseline = "middle";
  ctx.fillText(opts.max === undefined ? str : ellipsize(ctx, str, opts.max), x, y);
};

/** Shorten `str` with a trailing ellipsis until it fits `max` px in the CURRENT
 * font. Callers set the font via `text`, so this is only correct from there. */
const ellipsize = (ctx: CanvasRenderingContext2D, str: string, max: number): string => {
  if (ctx.measureText(str).width <= max) {
    return str;
  }
  let cut = str.length - 1;
  while (cut > 0 && ctx.measureText(`${str.slice(0, cut)}…`).width > max) {
    cut -= 1;
  }
  return `${str.slice(0, cut)}…`;
};

/** A rivet/bolt accent, part of the forge-plate look. */
export const rivet = (ctx: CanvasRenderingContext2D, x: number, y: number, color: string): void => {
  ctx.beginPath();
  ctx.arc(x, y, 1.8, 0, Math.PI * 2);
  ctx.fillStyle = color;
  ctx.fill();
};

/** A tap-target button. Height is always >= 44 in layout, so touch targets pass. */
export const button = (
  ctx: CanvasRenderingContext2D,
  r: Rect,
  label: string,
  opts: { fill: string; edge: string; text: string; enabled?: boolean; pressed?: boolean; sub?: string } ,
): void => {
  const enabled = opts.enabled ?? true;
  panel(ctx, opts.pressed ? { ...r, y: r.y + 1 } : r, enabled ? opts.fill : "#241f1a", opts.edge, 7);
  rivet(ctx, r.x + 5, r.y + 5, opts.edge);
  rivet(ctx, r.x + r.w - 5, r.y + 5, opts.edge);
  text(ctx, label, r.x + r.w / 2, r.y + (opts.sub ? r.h / 2 - 6 : r.h / 2), { size: 12, weight: 800, align: "center", color: enabled ? opts.text : "#6b6157" });
  if (opts.sub !== undefined) {
    text(ctx, opts.sub, r.x + r.w / 2, r.y + r.h / 2 + 9, { size: 10, weight: 700, align: "center", color: enabled ? opts.text : "#6b6157" });
  }
};

/** Clamp an rgba-ish color's alpha into a translucent overlay. */
export const shade = (ctx: CanvasRenderingContext2D, r: Rect, color: string, alpha: number): void => {
  ctx.save();
  ctx.globalAlpha = alpha;
  rrPath(ctx, r, 6);
  ctx.fillStyle = color;
  ctx.fill();
  ctx.restore();
};
