/*
 * streams.test.ts — the independence and determinism invariants of the named
 * random streams: same inputs → same draws; distinct purposes → decorrelated
 * seed spaces; the shuffle is a stable permutation of its input.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { sample01, sampleChance, sampleInt, shuffled, STREAM_PURPOSES, streamSeed } from "./streams.ts";

test("sample01 is a pure function of (seed, purpose, keys)", () => {
  for (let seed = 1; seed < 40; seed += 1) {
    assert.equal(sample01(seed, "gameplay", 3, 7), sample01(seed, "gameplay", 3, 7));
    assert.notEqual(sample01(seed, "gameplay", 3, 7), sample01(seed, "gameplay", 3, 8));
  }
});

test("every draw lands in [0, 1)", () => {
  for (let k = 0; k < 500; k += 1) {
    const u = sample01(k * 2654435761, "particles", k);
    assert.ok(u >= 0 && u < 1, `draw ${u} out of range`);
  }
});

test("stream purposes occupy distinct seed spaces", () => {
  const seeds = STREAM_PURPOSES.map((purpose) => streamSeed(12345, purpose));
  assert.equal(new Set(seeds).size, STREAM_PURPOSES.length);
});

test("gameplay draws are independent of decorative draws", () => {
  // The gameplay value for a given key must not depend on how many ambient /
  // particle draws happen elsewhere — pure keying guarantees it structurally;
  // pin it anyway.
  const before = sample01(999, "gameplay", 1, 1);
  sample01(999, "ambient", 5);
  sample01(999, "particles", 6);
  sample01(999, "camera", 7);
  assert.equal(sample01(999, "gameplay", 1, 1), before);
});

test("sampleInt stays in range and sampleChance respects 0 and 1", () => {
  for (let k = 0; k < 200; k += 1) {
    const n = sampleInt(9, k, "placement", k);
    assert.ok(n >= 0 && n < 9);
    assert.equal(sampleChance(0, k, "gameplay", k), false);
    assert.equal(sampleChance(1, k, "gameplay", k), true);
  }
});

test("shuffled returns a deterministic permutation", () => {
  const items = Array.from({ length: 12 }, (_, i) => i);
  const a = shuffled(items, 77, "placement", 4);
  const b = shuffled(items, 77, "placement", 4);
  assert.deepEqual(a, b);
  assert.deepEqual([...a].sort((x, y) => x - y), items);
  assert.notDeepEqual(shuffled(items, 78, "placement", 4), a);
});
