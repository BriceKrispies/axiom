/*
 * probe-canvas2d.ts — does the Canvas2D rung actually RASTERIZE? Unlike the
 * control probe (`probe-readback.ts`, which only moves bytes), this one paints
 * four `fillRect` stripes and reads them back, so it exercises the software
 * rasterizer the `backend-canvas2d.ts` renderer depends on.
 *
 * The distinction matters because Canvas2D is the last rung before the
 * context-free CSS3D fallback: if the engine is going to commit to a software
 * rasterizer it should have seen that rasterizer draw something, not merely
 * seen `getContext("2d")` return an object.
 *
 * Platform edge: browser-API boundary — ordinary control flow, coverage-exempt.
 */

import {
  EXPECTED_SIGNATURE,
  PATTERN_HEIGHT,
  PATTERN_WIDTH,
  STRIPE_COLORS,
  STRIPE_COUNT,
  type ReadbackTrust,
  classifyPattern,
  signature,
  stripeBounds,
} from "./probe-pattern.ts";
import type { TierProbe } from "./tier.ts";

const BYTE_MAX = 255;

const cssColor = (color: readonly [number, number, number]): string =>
  `rgb(${color.map((channel) => channel * BYTE_MAX).join(",")})`;

/** Paint the stripe pattern with real `fillRect` rasterization. */
const paint = (ctx: CanvasRenderingContext2D): void => {
  for (let stripe = 0; stripe < STRIPE_COUNT; stripe += 1) {
    const bounds = stripeBounds(stripe, PATTERN_WIDTH);
    ctx.fillStyle = cssColor(STRIPE_COLORS[stripe]!);
    ctx.fillRect(bounds.start, 0, bounds.span, PATTERN_HEIGHT);
  }
};

/**
 * Probe the Canvas2D rung. `trust` is the control probe's verdict: under
 * `neutralised` the pixels are unusable, so a context that took every call
 * without throwing is accepted as `degraded` rather than failed.
 */
export const probeCanvas2d = (trust: ReadbackTrust): TierProbe => {
  try {
    const canvas = document.createElement("canvas");
    canvas.width = PATTERN_WIDTH;
    canvas.height = PATTERN_HEIGHT;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) {
      return { accelerated: false, detail: "no 2d context", outcome: "fail" };
    }
    paint(ctx);
    if (trust === "neutralised") {
      return { accelerated: false, detail: "rasterized; pixels not readable (readback neutralised)", outcome: "degraded" };
    }
    const read = ctx.getImageData(0, 0, PATTERN_WIDTH, PATTERN_HEIGHT).data;
    const verdict = classifyPattern(signature(read, PATTERN_WIDTH, PATTERN_HEIGHT), EXPECTED_SIGNATURE);
    if (verdict === "match") {
      return { accelerated: false, detail: "fillRect pattern verified", outcome: "pass" };
    }
    return { accelerated: false, detail: `fillRect pattern ${verdict}`, outcome: "fail" };
  } catch (error) {
    return { accelerated: false, detail: `canvas2d probe threw: ${String(error)}`, outcome: "fail" };
  }
};
