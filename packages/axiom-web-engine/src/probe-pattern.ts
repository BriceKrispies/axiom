/*
 * probe-pattern.ts — the TEST PATTERN the capability probes paint, and the
 * CLASSIFIERS that judge what came back. Pure: no canvas, no context, no DOM.
 * The platform-edge probe files (`probe-readback.ts`, `probe-canvas2d.ts`,
 * `probe-webgl.ts`) own the drawing; this file owns what "the right pixels" and
 * "a trustworthy readback" mean, so both questions are decidable under
 * `node --test` on synthetic buffers.
 *
 * Two properties make the pattern usable across every backend:
 *
 *   - It uses ONLY 0.0/1.0 channel values. A stripe is red, green, blue, or
 *     white — never an intermediate. Nothing in the pipeline (8-bit rounding, a
 *     colour-space conversion, premultiplied alpha, a software rasterizer) can
 *     move a channel across the 128 classification threshold, so a correct
 *     backend classifies identically everywhere.
 *   - It is Y-SYMMETRIC: the stripes are VERTICAL, so every row is the same.
 *     `gl.readPixels` returns rows bottom-up and `ctx.getImageData` returns them
 *     top-down; with a Y-symmetric pattern the two agree WITHOUT the probe
 *     flipping anything, which removes the single most common source of
 *     false negatives in a cross-backend pixel check.
 *
 * The load-bearing idea is that the probes compare CLASSIFICATIONS, not bytes.
 * Brave and Firefox (resistFingerprinting) deliberately perturb canvas readback
 * — "farbling" — so a byte comparison reports a broken GPU on a perfectly
 * healthy machine. A per-stripe majority vote against a 128 threshold survives
 * that perturbation untouched, while still catching the cases that matter: a
 * blank buffer (Tor Browser returns uniform white) or genuinely wrong pixels.
 */

import { both, either, orElse, pick, select } from "./branchless.ts";

/** Four vertical stripes: red, green, blue, white. */
export const STRIPE_COUNT = 4;

/** The probe surface is deliberately tiny — a few hundred bytes to paint and
 * read back, so the whole ladder costs microseconds and a WebGL probe context
 * allocates almost nothing before it is disposed. */
export const PATTERN_WIDTH = 8;
export const PATTERN_HEIGHT = 8;

/** RGBA. */
const CHANNELS = 4;
const BYTE_MAX = 255;

/** Above this a channel reads as "on". Exactly half of the 0..255 range, which
 * is as far as a 0.0/1.0 authored channel can possibly be from either verdict. */
const CHANNEL_THRESHOLD = 128;

/** One half, named so it is not a magic number. */
const HALF = 0.5;

/** The largest per-byte readback delta that still counts as farbling noise
 * rather than a destroyed signal. Half the classification threshold: a delta
 * this size provably cannot flip any stripe's bit, so the classification — the
 * thing the probes actually consume — survives it intact. */
const NOISE_LIMIT = CHANNEL_THRESHOLD * HALF;

/** A stripe reads as "on" for a channel when at least this fraction of its
 * pixels are above the threshold. A MAJORITY vote, not a mean: a farbling
 * implementation that scrambles a handful of pixels outright (rather than
 * nudging all of them) cannot move the verdict. */
const MAJORITY_FRACTION = HALF;

const RED_INDEX = 0;
const GREEN_INDEX = 1;
const BLUE_INDEX = 2;

/** The 3-bit stripe code: red is the high bit, blue the low one. */
const RED_BIT = 4;
const GREEN_BIT = 2;
const BLUE_BIT = 1;

/** A stripe colour as authored: every channel is exactly 0.0 or 1.0. */
export type StripeColor = readonly [number, number, number];

/** The four stripes, left to right. Distinct 3-bit codes (4, 2, 1, 7) that are
 * pairwise different, so a backend that paints all four the same colour — the
 * blank-canvas signature — is never mistaken for a correct render. */
export const STRIPE_COLORS: readonly StripeColor[] = [
  [1, 0, 0],
  [0, 1, 0],
  [0, 0, 1],
  [1, 1, 1],
];

/** How a rendered pattern compares to the expected one. */
export type PatternVerdict = "match" | "mismatch" | "uniform";

/** How much a `putImageData` → `getImageData` round trip can be believed.
 *   - `exact`      — byte-identical: pixels mean what they say.
 *   - `noisy`      — small per-pixel deltas: Brave/Firefox farbling. Pixels are
 *                    still classifiable, so pixel-based probes remain valid.
 *   - `neutralised`— uniform or wildly wrong: Tor's blank canvas, or a policy
 *                    that blocks readback. NO pixel evidence is admissible; the
 *                    probes must fall back to structural evidence. */
export type ReadbackTrust = "exact" | "noisy" | "neutralised";

/** `[0, 1, …, count - 1]` — the branchless counting-index list. */
const range = (count: number): number[] => Array.from({ length: count }, (value, index) => index);

/** `bytes[index]`, or 0 when the buffer is short — a truncated buffer must
 * classify as wrong pixels, never throw out of a probe. */
const byteAt = (bytes: readonly number[], index: number): number => orElse(bytes[index], 0);

/** Which stripe column `x` belongs to. */
const stripeIndexAt = (x: number, width: number): number =>
  Math.min(STRIPE_COUNT - 1, Math.floor((x * STRIPE_COUNT) / width));

/** The pixel span of one stripe, for a probe that paints it (a scissored
 * `gl.clear`, a `fillRect`, a DOM element). */
export interface StripeBounds {
  /** Width in pixels. */
  readonly span: number;
  /** Leftmost pixel column. */
  readonly start: number;
}

export const stripeBounds = (stripe: number, width: number): StripeBounds => {
  const start = Math.round((stripe * width) / STRIPE_COUNT);
  const end = Math.round(((stripe + 1) * width) / STRIPE_COUNT);
  return { span: end - start, start };
};

/** The 3-bit code of an authored stripe colour. */
const colorCode = ([red, green, blue]: StripeColor): number => red * RED_BIT + green * GREEN_BIT + blue * BLUE_BIT;

/** What `signature` must return for a correctly rendered pattern. */
export const EXPECTED_SIGNATURE: readonly number[] = STRIPE_COLORS.map((color) => colorCode(color));

const pixelBytes = (x: number): number[] => {
  const [red, green, blue] = pick(STRIPE_COLORS, stripeIndexAt(x, PATTERN_WIDTH));
  return [red * BYTE_MAX, green * BYTE_MAX, blue * BYTE_MAX, BYTE_MAX];
};

/** The expected pattern as RGBA bytes — what a probe writes, and the reference
 * a readback is compared against. */
export const patternBytes = (): Uint8ClampedArray =>
  Uint8ClampedArray.from(range(PATTERN_HEIGHT).flatMap(() => range(PATTERN_WIDTH).flatMap((x) => pixelBytes(x))));

/** An RGBA buffer with its dimensions — everything `signature` reads. */
interface PatternGrid {
  readonly bytes: readonly number[];
  readonly height: number;
  readonly width: number;
}

/** Every value of one channel inside one stripe. */
const stripeSamples = (grid: PatternGrid, stripe: number, channel: number): number[] => {
  const columns = range(grid.width).filter((x) => stripeIndexAt(x, grid.width) === stripe);
  return range(grid.height).flatMap((y) => columns.map((x) => byteAt(grid.bytes, (y * grid.width + x) * CHANNELS + channel)));
};

/** The majority verdict for one channel of one stripe: 1 when most of its
 * pixels are above the threshold. An empty stripe votes 0. */
const channelBit = (grid: PatternGrid, stripe: number, channel: number): number => {
  const samples = stripeSamples(grid, stripe, channel);
  const bright = samples.filter((value) => value >= CHANNEL_THRESHOLD).length;
  return Number(both(samples.length > 0, bright >= samples.length * MAJORITY_FRACTION));
};

const stripeCode = (grid: PatternGrid, stripe: number): number =>
  colorCode([channelBit(grid, stripe, RED_INDEX), channelBit(grid, stripe, GREEN_INDEX), channelBit(grid, stripe, BLUE_INDEX)]);

/**
 * Classify an RGBA buffer into one 3-bit code per stripe. This is the lossy
 * projection that makes the probes farbling-immune: it throws away every bit of
 * precision the browser is entitled to perturb and keeps only the question the
 * probe is actually asking — did each stripe come back the colour it was
 * painted?
 */
export const signature = (bytes: ArrayLike<number>, width: number, height: number): readonly number[] => {
  const grid: PatternGrid = { bytes: Array.from(bytes, (value) => value), height, width };
  return range(STRIPE_COUNT).map((stripe) => stripeCode(grid, stripe));
};

/**
 * Compare a rendered signature against the expected one.
 *
 * `uniform` is called out separately from `mismatch` because it is a specific,
 * recognisable failure: every stripe classified the same means the buffer came
 * back FLAT. That is the Tor Browser blank-canvas signature and equally the
 * signature of a GPU that accepted every command and drew nothing — a state a
 * naive `getContext() !== null` check reports as a working backend.
 */
export const classifyPattern = (actual: readonly number[], expected: readonly number[]): PatternVerdict => {
  const matched = both(
    actual.length === expected.length,
    expected.every((code, index) => code === byteAt(actual, index)),
  );
  const flat = actual.every((code) => code === byteAt(actual, 0));
  return select(matched, "match", select(flat, "uniform", "mismatch"));
};

/**
 * The CONTROL PROBE's classifier: how far a `putImageData` → `getImageData`
 * round trip drifted, and therefore how much any later pixel evidence can be
 * believed. No rasterization is involved — the browser is only asked to hand
 * back bytes it was just given — so a delta here is the browser's privacy
 * policy talking, never a driver or a shader.
 *
 * This runs FIRST and every other probe reads its verdict: under `neutralised`
 * a tier must prove itself structurally (context created, no API error, context
 * not lost, shaders linked) because its pixels have been taken away.
 */
export const classifyReadbackDelta = (written: ArrayLike<number>, read: ArrayLike<number>): ReadbackTrust => {
  const source = Array.from(written, (value) => value);
  const result = Array.from(read, (value) => value);
  const usable = both(source.length > 0, source.length === result.length);
  const deltas = source.slice(0, result.length).map((value, index) => Math.abs(value - byteAt(result, index)));
  const worst = Math.max(0, ...deltas);
  const flat = result.every((value, index) => value === byteAt(result, index % CHANNELS));
  const destroyed = either(either(!usable, flat), worst > NOISE_LIMIT);
  return select(both(usable, worst === 0), "exact", select(destroyed, "neutralised", "noisy"));
};
