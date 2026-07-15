/*
 * thumbnails.ts — lightweight procedural catalog thumbnails. Each card gets a
 * small Canvas2D painting (gradient sky, ground, one glyph silhouette) drawn
 * exactly once — no live scenes, no per-frame work in the catalog.
 */

import type { ThumbnailSpec } from "../chance-engine/registry/definition.ts";

const glyphPath = (ctx: CanvasRenderingContext2D, glyph: ThumbnailSpec["glyph"], w: number, h: number): void => {
  const cx = w / 2;
  const cy = h * 0.56;
  const s = h * 0.3;
  ctx.beginPath();
  switch (glyph) {
    case "chest":
      ctx.rect(cx - s, cy - s * 0.3, s * 2, s * 1.1);
      ctx.moveTo(cx - s, cy - s * 0.3);
      ctx.arc(cx, cy - s * 0.3, s, Math.PI, 0);
      break;
    case "card":
      ctx.roundRect(cx - s * 0.7, cy - s, s * 1.4, s * 2, s * 0.2);
      break;
    case "wheel":
      ctx.arc(cx, cy, s, 0, Math.PI * 2);
      ctx.moveTo(cx, cy);
      for (let i = 0; i < 6; i += 1) {
        const a = (i / 6) * Math.PI * 2;
        ctx.moveTo(cx, cy);
        ctx.lineTo(cx + Math.cos(a) * s, cy + Math.sin(a) * s);
      }
      break;
    case "dice":
      ctx.roundRect(cx - s, cy - s, s * 2, s * 2, s * 0.3);
      break;
    case "door":
      ctx.roundRect(cx - s * 0.75, cy - s, s * 1.5, s * 2.05, [s * 0.75, s * 0.75, 0, 0]);
      break;
    case "globe":
      ctx.arc(cx, cy - s * 0.15, s, 0, Math.PI * 2);
      ctx.rect(cx - s * 0.5, cy + s * 0.75, s, s * 0.5);
      break;
    case "dial":
      ctx.arc(cx, cy, s, 0, Math.PI * 2);
      ctx.moveTo(cx, cy);
      ctx.lineTo(cx + s * 0.7, cy - s * 0.5);
      break;
    case "ticket":
      ctx.roundRect(cx - s * 1.2, cy - s * 0.65, s * 2.4, s * 1.3, s * 0.2);
      break;
    case "gift":
      ctx.rect(cx - s * 0.9, cy - s * 0.55, s * 1.8, s * 1.4);
      ctx.rect(cx - s * 0.12, cy - s * 0.55, s * 0.24, s * 1.4);
      ctx.rect(cx - s * 1.02, cy - s * 0.8, s * 2.04, s * 0.3);
      break;
    case "rocket":
      ctx.moveTo(cx, cy - s * 1.2);
      ctx.quadraticCurveTo(cx + s * 0.65, cy - s * 0.1, cx + s * 0.4, cy + s * 0.8);
      ctx.lineTo(cx - s * 0.4, cy + s * 0.8);
      ctx.quadraticCurveTo(cx - s * 0.65, cy - s * 0.1, cx, cy - s * 1.2);
      break;
    case "bobber":
      ctx.arc(cx, cy, s * 0.75, 0, Math.PI * 2);
      ctx.moveTo(cx, cy - s * 1.3);
      ctx.lineTo(cx, cy - s * 0.75);
      break;
    case "claw":
      ctx.arc(cx, cy - s * 0.4, s * 0.5, Math.PI, 0);
      ctx.moveTo(cx - s * 0.5, cy - s * 0.4);
      ctx.quadraticCurveTo(cx - s * 0.8, cy + s * 0.5, cx - s * 0.25, cy + s * 0.7);
      ctx.moveTo(cx + s * 0.5, cy - s * 0.4);
      ctx.quadraticCurveTo(cx + s * 0.8, cy + s * 0.5, cx + s * 0.25, cy + s * 0.7);
      ctx.moveTo(cx, cy - s * 0.9);
      ctx.lineTo(cx, cy - s * 1.4);
      break;
    case "elevator":
      ctx.rect(cx - s * 0.8, cy - s * 1.1, s * 1.6, s * 2.2);
      ctx.moveTo(cx, cy - s * 1.1);
      ctx.lineTo(cx, cy + s * 1.1);
      break;
    case "fountain":
      ctx.ellipse(cx, cy + s * 0.6, s * 1.2, s * 0.4, 0, 0, Math.PI * 2);
      ctx.moveTo(cx, cy + s * 0.4);
      ctx.quadraticCurveTo(cx - s * 0.6, cy - s * 0.6, cx, cy - s * 1.1);
      ctx.quadraticCurveTo(cx + s * 0.6, cy - s * 0.6, cx, cy + s * 0.4);
      break;
    case "map":
      ctx.moveTo(cx - s * 1.2, cy - s * 0.7);
      ctx.lineTo(cx - s * 0.4, cy - s * 0.95);
      ctx.lineTo(cx + s * 0.4, cy - s * 0.7);
      ctx.lineTo(cx + s * 1.2, cy - s * 0.95);
      ctx.lineTo(cx + s * 1.2, cy + s * 0.7);
      ctx.lineTo(cx + s * 0.4, cy + s * 0.95);
      ctx.lineTo(cx - s * 0.4, cy + s * 0.7);
      ctx.lineTo(cx - s * 1.2, cy + s * 0.95);
      ctx.closePath();
      break;
    case "portal":
      ctx.ellipse(cx, cy, s * 0.7, s * 1.05, 0, 0, Math.PI * 2);
      break;
    case "capsule":
      ctx.arc(cx, cy - s * 0.12, s * 0.85, Math.PI, 0);
      ctx.arc(cx, cy + 0.12 * s, s * 0.85, 0, Math.PI);
      break;
    case "lantern":
      ctx.ellipse(cx, cy - s * 0.1, s * 0.7, s * 0.95, 0, 0, Math.PI * 2);
      ctx.rect(cx - s * 0.3, cy + s * 0.85, s * 0.6, s * 0.22);
      break;
    case "gem":
      ctx.moveTo(cx, cy + s);
      ctx.lineTo(cx - s, cy - s * 0.25);
      ctx.lineTo(cx - s * 0.5, cy - s * 0.85);
      ctx.lineTo(cx + s * 0.5, cy - s * 0.85);
      ctx.lineTo(cx + s, cy - s * 0.25);
      ctx.closePath();
      break;
    case "token":
      ctx.arc(cx, cy, s, 0, Math.PI * 2);
      ctx.moveTo(cx + s * 0.55, cy);
      ctx.arc(cx, cy, s * 0.55, 0, Math.PI * 2);
      break;
  }
};

/** Paint one card thumbnail (called once per card at catalog build time). */
export const paintThumbnail = (canvas: HTMLCanvasElement, spec: ThumbnailSpec): void => {
  const w = (canvas.width = 300);
  const h = (canvas.height = 145);
  const ctx = canvas.getContext("2d");
  if (ctx === null) {
    return;
  }
  const sky = ctx.createLinearGradient(0, 0, 0, h);
  sky.addColorStop(0, spec.top);
  sky.addColorStop(1, spec.bottom);
  ctx.fillStyle = sky;
  ctx.fillRect(0, 0, w, h);

  // Soft ground ellipse + sparkles.
  ctx.fillStyle = "rgba(255,255,255,0.22)";
  ctx.beginPath();
  ctx.ellipse(w / 2, h * 0.92, w * 0.42, h * 0.16, 0, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = "rgba(255,255,255,0.75)";
  for (let i = 0; i < 9; i += 1) {
    const x = ((i * 97 + 31) % 300);
    const y = ((i * 53 + 17) % 90) + 8;
    const r = (i % 3) * 0.7 + 0.8;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fill();
  }

  ctx.strokeStyle = "rgba(30,40,60,0.5)";
  ctx.lineWidth = 4;
  ctx.fillStyle = spec.accent;
  glyphPath(ctx, spec.glyph, w, h);
  ctx.fill();
  ctx.stroke();
};
