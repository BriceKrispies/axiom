/*
 * render-quality.test.ts — `node --test` coverage for the rasterization policy in
 * `render-quality.ts`: the defaults, validation and clamping of every field,
 * pixel-ratio mode resolution, backing-store arithmetic, the two ceilings, and
 * the resize guard. Pure values — no canvas, no context, no DOM.
 *
 * The load-bearing assertions are (a) that the default quality reproduces the
 * engine's previous tessellation exactly, so adding this policy re-renders
 * nothing on its own, and (b) that a clamp only ever moves the SCALE, so the
 * aspect ratio survives every limit.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import {
  DEFAULT_MAX_SAMPLES,
  DEFAULT_RENDER_QUALITY,
  LINE_CAPS,
  LINE_JOINS,
  MITER_LIMIT,
  PIXEL_RATIO_MODES,
  RENDER_SCALES,
  type RenderQuality,
  type RenderQualityInput,
  backingSizeMatches,
  clampRenderQuality,
  resolveBackingSize,
  resolvePixelRatio,
} from "./render-quality.ts";

/** A quality with both ceilings lifted, so a test of the SCALE arithmetic is not
 * silently measuring a limit instead. */
const unbounded = (over: RenderQualityInput): RenderQuality =>
  clampRenderQuality({ maxBackingDimension: 8192, maxSamples: 8192 * 8192, ...over });

/** The ratio a mode resolves for a given display, at a fixed 2x cap. */
const at = (pixelRatioMode: string, deviceRatio: number): number =>
  resolvePixelRatio(clampRenderQuality({ maxPixelRatio: 2, pixelRatioMode }), deviceRatio);

test("the default quality leaves the engine's existing rendering alone", () => {
  assert.equal(DEFAULT_RENDER_QUALITY.curveDetail, 1, "1 means 'do not touch the backend's own facet budget'");
  assert.equal(DEFAULT_RENDER_QUALITY.renderScale, 1, "no supersampling on top of the display");
  assert.equal(DEFAULT_RENDER_QUALITY.pixelRatioMode, "capped-device");
  assert.equal(DEFAULT_RENDER_QUALITY.maxPixelRatio, 2);
  assert.equal(DEFAULT_RENDER_QUALITY.lineJoin, "round");
  assert.equal(DEFAULT_RENDER_QUALITY.lineCap, "round");
  assert.equal(DEFAULT_RENDER_QUALITY.maxSamples, DEFAULT_MAX_SAMPLES);
  assert.equal(MITER_LIMIT, 4, "a fixed safety bound, not a look");
});

test("an absent or empty input resolves to exactly the defaults", () => {
  assert.deepEqual(clampRenderQuality(), DEFAULT_RENDER_QUALITY);
  assert.deepEqual(clampRenderQuality({}), DEFAULT_RENDER_QUALITY);
});

test("every numeric field clamps to its own range", () => {
  const low = clampRenderQuality({ curveDetail: 0, maxPixelRatio: 0, maxSamples: 1, renderScale: 0 });
  assert.equal(low.renderScale, 0.5, "render scale floors at the historical software rung");
  assert.equal(low.curveDetail, 0.25);
  assert.equal(low.maxPixelRatio, 1);
  assert.equal(low.maxSamples, 64 * 64);

  const high = clampRenderQuality({ curveDetail: 99, maxBackingDimension: 1e9, maxPixelRatio: 99, renderScale: 99 });
  assert.equal(high.renderScale, 2);
  assert.equal(high.curveDetail, 2);
  assert.equal(high.maxPixelRatio, 4);
  assert.equal(high.maxBackingDimension, 8192);
});

test("a non-finite number is a corrupt setting, not a value to clamp", () => {
  const resolved = clampRenderQuality({ curveDetail: Number.NaN, maxSamples: Number.NaN, renderScale: Number.POSITIVE_INFINITY });
  assert.equal(resolved.renderScale, DEFAULT_RENDER_QUALITY.renderScale, "Infinity falls back rather than clamping to the max");
  assert.equal(resolved.curveDetail, DEFAULT_RENDER_QUALITY.curveDetail);
  assert.equal(resolved.maxSamples, DEFAULT_RENDER_QUALITY.maxSamples);
});

test("an unknown enum from stored JSON degrades to the default", () => {
  const resolved = clampRenderQuality({ lineCap: "chamfered", lineJoin: "swoosh", pixelRatioMode: "retina-plus" });
  assert.equal(resolved.pixelRatioMode, DEFAULT_RENDER_QUALITY.pixelRatioMode);
  assert.equal(resolved.lineJoin, DEFAULT_RENDER_QUALITY.lineJoin);
  assert.equal(resolved.lineCap, DEFAULT_RENDER_QUALITY.lineCap);
});

test("every offered control value survives validation unchanged", () => {
  PIXEL_RATIO_MODES.map((mode): void =>
    assert.equal(clampRenderQuality({ pixelRatioMode: mode }).pixelRatioMode, mode, `mode ${mode}`),
  );
  LINE_JOINS.map((join): void => assert.equal(clampRenderQuality({ lineJoin: join }).lineJoin, join, `join ${join}`));
  LINE_CAPS.map((cap): void => assert.equal(clampRenderQuality({ lineCap: cap }).lineCap, cap, `cap ${cap}`));
  RENDER_SCALES.map((scale): void =>
    assert.equal(clampRenderQuality({ renderScale: scale }).renderScale, scale, `scale ${scale}`),
  );
});

test("each pixel-ratio mode resolves the display the way it says", () => {
  assert.equal(at("fixed-1x", 3), 1, "fixed ignores the display entirely");
  assert.equal(at("device", 3), 3, "device follows it");
  assert.equal(at("capped-device", 3), 2, "capped stops at maxPixelRatio");
  assert.equal(at("capped-device", 1.5), 1.5, "capped is a ceiling, not a target");
});

test("a nonsense device ratio cannot poison the backing size", () => {
  assert.equal(resolvePixelRatio(clampRenderQuality({ pixelRatioMode: "device" }), 0), 1, "0 floors at 1");
  assert.equal(resolvePixelRatio(clampRenderQuality({ pixelRatioMode: "device" }), Number.NaN), 1, "NaN falls back to 1");
  assert.equal(resolvePixelRatio(clampRenderQuality({ pixelRatioMode: "device" }), 99), 4, "an absurd ratio is capped");
});

test("the backing store is the CSS box times pixel ratio times render scale", () => {
  const size = resolveBackingSize({
    cssHeight: 730,
    cssWidth: 1500,
    deviceRatio: 2,
    quality: unbounded({ maxPixelRatio: 2, pixelRatioMode: "capped-device", renderScale: 1.5 }),
  });
  assert.equal(size.width, 4500, "1500 * 2.0 * 1.5");
  assert.equal(size.height, 2190, "730 * 2.0 * 1.5");
  assert.equal(size.scale, 3, "one transform of 3x, applied to the context exactly once");
  assert.equal(size.clamped, false);
});

test("render scale is pure supersampling — the CSS box is not consulted twice", () => {
  const spec = { cssHeight: 600, cssWidth: 960, deviceRatio: 1 };
  const plain = resolveBackingSize({ ...spec, quality: unbounded({ pixelRatioMode: "fixed-1x", renderScale: 1 }) });
  const supersampled = resolveBackingSize({ ...spec, quality: unbounded({ pixelRatioMode: "fixed-1x", renderScale: 2 }) });
  assert.equal(plain.width, 960);
  assert.equal(supersampled.width, 1920, "twice the samples across the SAME CSS box");
  assert.equal(supersampled.height / plain.height, 2);
  assert.equal(supersampled.width / supersampled.height, plain.width / plain.height, "aspect is untouched");
});

test("the sample budget pulls the scale down and says so", () => {
  const quality = clampRenderQuality({
    maxSamples: 1280 * 720,
    pixelRatioMode: "device",
    renderScale: 2,
  });
  const size = resolveBackingSize({ cssHeight: 1000, cssWidth: 1600, deviceRatio: 2, quality });
  assert.ok(size.width * size.height <= 1280 * 720 + 1, `budget honoured, got ${size.width}x${size.height}`);
  assert.equal(size.clamped, true, "the caller can see a limit engaged");
  assert.ok(
    Math.abs(size.width / size.height - 1.6) < 0.01,
    `aspect preserved through the clamp, got ${size.width}x${size.height}`,
  );
});

test("the dimension guard bounds a single absurd axis", () => {
  const size = resolveBackingSize({
    cssHeight: 200,
    cssWidth: 20_000,
    deviceRatio: 1,
    quality: clampRenderQuality({ maxBackingDimension: 4096, maxSamples: 8192 * 8192, pixelRatioMode: "fixed-1x" }),
  });
  assert.equal(size.width, 4096, "the long axis lands exactly on the guard");
  assert.ok(size.height < 200, "the short axis came down with it, so the aspect held");
  assert.equal(size.clamped, true);
});

test("a degenerate CSS box still yields a drawable surface", () => {
  const size = resolveBackingSize({
    cssHeight: 0,
    cssWidth: Number.NaN,
    deviceRatio: 1,
    quality: DEFAULT_RENDER_QUALITY,
  });
  assert.ok(size.width >= 1 && size.height >= 1, "never a zero-dimension canvas");
});

test("the resize guard only fires on a real size change", () => {
  const size = resolveBackingSize({
    cssHeight: 600,
    cssWidth: 960,
    deviceRatio: 1,
    quality: unbounded({ pixelRatioMode: "fixed-1x", renderScale: 1 }),
  });
  assert.equal(backingSizeMatches({ height: 600, width: 960 }, size), true, "already correct — do not reallocate");
  assert.equal(backingSizeMatches({ height: 600, width: 959 }, size), false);
  assert.equal(backingSizeMatches({ height: 599, width: 960 }, size), false);
});
