/*
 * probe-pattern.test.ts — `node --test` coverage for the probe pattern and its
 * classifiers, on SYNTHETIC buffers. The whole point of splitting these rules
 * out of the browser-only probe files is that the interesting cases — a farbled
 * readback, Tor's blank canvas, a plausible-but-wrong render — can be
 * constructed here exactly, instead of being hoped for in a real browser.
 *
 * The load-bearing assertions:
 *   - the pattern is Y-SYMMETRIC, so bottom-up `readPixels` and top-down
 *     `getImageData` classify identically without any flip;
 *   - a farbled buffer still classifies as a MATCH (the immunity claim);
 *   - a uniform buffer is called `uniform`, not `mismatch` (the blank-canvas
 *     signature the naive `getContext() !== null` check misses).
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import {
  EXPECTED_SIGNATURE,
  PATTERN_HEIGHT,
  PATTERN_WIDTH,
  STRIPE_COLORS,
  STRIPE_COUNT,
  classifyPattern,
  classifyReadbackDelta,
  patternBytes,
  signature,
  stripeBounds,
} from "./probe-pattern.ts";

const CHANNELS = 4;

/** Flip an RGBA buffer top-to-bottom — the `readPixels` vs `getImageData`
 * origin difference, applied by hand. */
const flipRows = (bytes: Uint8ClampedArray): Uint8ClampedArray => {
  const out = new Uint8ClampedArray(bytes.length);
  const rowBytes = PATTERN_WIDTH * CHANNELS;
  for (let y = 0; y < PATTERN_HEIGHT; y++) {
    const src = y * rowBytes;
    const dst = (PATTERN_HEIGHT - 1 - y) * rowBytes;
    out.set(bytes.subarray(src, src + rowBytes), dst);
  }
  return out;
};

/** A deterministic "farbled" buffer: every byte nudged the way Brave and
 * Firefox's resistFingerprinting nudge canvas readback. */
const farble = (bytes: Uint8ClampedArray, amplitude: number): Uint8ClampedArray =>
  Uint8ClampedArray.from(bytes, (value, index) => value + ((index % 7) - 3) * amplitude);

const uniform = (value: number): Uint8ClampedArray =>
  Uint8ClampedArray.from({ length: PATTERN_WIDTH * PATTERN_HEIGHT * CHANNELS }, () => value);

const sigOf = (bytes: ArrayLike<number>): readonly number[] => signature(bytes, PATTERN_WIDTH, PATTERN_HEIGHT);

test("the authored stripes use only 0.0/1.0 channel values", () => {
  assert.equal(STRIPE_COLORS.length, STRIPE_COUNT);
  for (const color of STRIPE_COLORS) {
    for (const channel of color) {
      assert.ok(channel === 0 || channel === 1, `channel ${channel} is not 0.0 or 1.0`);
    }
  }
});

test("the expected signature is four DISTINCT stripe codes", () => {
  assert.deepEqual(EXPECTED_SIGNATURE, [4, 2, 1, 7]);
  assert.equal(new Set(EXPECTED_SIGNATURE).size, STRIPE_COUNT, "distinct codes, so a flat buffer can never match");
});

test("patternBytes renders the expected signature", () => {
  const bytes = patternBytes();
  assert.equal(bytes.length, PATTERN_WIDTH * PATTERN_HEIGHT * CHANNELS);
  assert.deepEqual(sigOf(bytes), EXPECTED_SIGNATURE);
  assert.equal(classifyPattern(sigOf(bytes), EXPECTED_SIGNATURE), "match");
});

test("the pattern is Y-symmetric: a vertically flipped readback classifies identically", () => {
  const bytes = patternBytes();
  const flipped = flipRows(bytes);
  assert.deepEqual(Array.from(flipped), Array.from(bytes), "vertical stripes are unchanged by a row flip");
  assert.deepEqual(sigOf(flipped), EXPECTED_SIGNATURE, "readPixels (bottom-left origin) needs no flip");
});

test("a farbled render still classifies as a match", () => {
  const farbled = farble(patternBytes(), 3);
  assert.notDeepEqual(Array.from(farbled), Array.from(patternBytes()), "the bytes really did change");
  assert.deepEqual(sigOf(farbled), EXPECTED_SIGNATURE, "the CLASSIFICATION is what farbling cannot touch");
  assert.equal(classifyPattern(sigOf(farbled), EXPECTED_SIGNATURE), "match");
});

test("a uniform buffer classifies as uniform, not mismatch", () => {
  assert.equal(classifyPattern(sigOf(uniform(255)), EXPECTED_SIGNATURE), "uniform", "Tor's blank white canvas");
  assert.equal(classifyPattern(sigOf(uniform(0)), EXPECTED_SIGNATURE), "uniform", "a cleared-but-never-drawn buffer");
});

test("a wrong-but-varied render classifies as a mismatch", () => {
  // The stripes present, but rotated: varied, so not uniform — just wrong.
  assert.equal(classifyPattern([2, 1, 7, 4], EXPECTED_SIGNATURE), "mismatch");
  assert.equal(classifyPattern([4, 2, 1], EXPECTED_SIGNATURE), "mismatch", "a truncated signature is a mismatch");
});

test("a truncated or empty buffer never throws, it just classifies wrong", () => {
  assert.deepEqual(sigOf(new Uint8ClampedArray(0)), [0, 0, 0, 0]);
  assert.equal(classifyPattern(sigOf(new Uint8ClampedArray(0)), EXPECTED_SIGNATURE), "uniform");
  const half = patternBytes().subarray(0, PATTERN_WIDTH * CHANNELS);
  assert.equal(classifyPattern(sigOf(half), EXPECTED_SIGNATURE), "uniform", "one row of eight: the buffer reads flat");
});

test("a stripe's majority vote survives individually scrambled pixels", () => {
  const bytes = patternBytes();
  // Blow away the first two pixels of the red stripe entirely.
  bytes.set([0, 255, 0, 255, 0, 255, 0, 255], 0);
  assert.deepEqual(sigOf(bytes), EXPECTED_SIGNATURE, "a minority of scrambled pixels cannot flip the vote");
});

test("stripeBounds tiles the width exactly, with no gap and no overlap", () => {
  const spans = Array.from({ length: STRIPE_COUNT }, (unused, stripe) => stripeBounds(stripe, PATTERN_WIDTH));
  assert.deepEqual(spans[0], { span: 2, start: 0 });
  assert.deepEqual(spans[3], { span: 2, start: 6 });
  let covered = 0;
  for (let stripe = 0; stripe < STRIPE_COUNT; stripe++) {
    assert.equal(spans[stripe]!.start, covered, "stripes tile with no gap and no overlap");
    covered += spans[stripe]!.span;
  }
  assert.equal(covered, PATTERN_WIDTH);
});

test("classifyReadbackDelta: an identical round trip is exact", () => {
  const written = patternBytes();
  assert.equal(classifyReadbackDelta(written, Uint8ClampedArray.from(written)), "exact");
});

test("classifyReadbackDelta: a small per-pixel perturbation is noise, not damage", () => {
  const written = patternBytes();
  assert.equal(classifyReadbackDelta(written, farble(written, 1)), "noisy", "Brave-style farbling");
  assert.equal(classifyReadbackDelta(written, farble(written, 20)), "noisy", "still below the classification threshold");
});

test("classifyReadbackDelta: a blank or wildly wrong readback is neutralised", () => {
  const written = patternBytes();
  assert.equal(classifyReadbackDelta(written, uniform(255)), "neutralised", "Tor's blank canvas");
  assert.equal(classifyReadbackDelta(written, uniform(0)), "neutralised", "a zeroed buffer");
  assert.equal(classifyReadbackDelta(written, farble(written, 40)), "neutralised", "past the noise limit");
});

test("classifyReadbackDelta: a missing or short readback is neutralised, never exact", () => {
  const written = patternBytes();
  assert.equal(classifyReadbackDelta(written, new Uint8ClampedArray(0)), "neutralised");
  assert.equal(classifyReadbackDelta(new Uint8ClampedArray(0), new Uint8ClampedArray(0)), "neutralised");
  assert.equal(classifyReadbackDelta(written, written.subarray(0, 16)), "neutralised", "a truncated readback");
});

test("classifyReadbackDelta: an exact round trip of a uniform buffer is still exact", () => {
  const flat = uniform(128);
  assert.equal(classifyReadbackDelta(flat, Uint8ClampedArray.from(flat)), "exact", "exactness outranks flatness");
});
