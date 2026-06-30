import assert from "node:assert/strict";
import { test } from "node:test";

import type { Rect } from "./vocabulary.ts";
import type { SpriteAnimation } from "./draw2d-binding.ts";
import { sampleAnimation } from "./draw2d.ts";
import { bindNative } from "./host-binding.ts";
import { FakeHost } from "./fake-host.testkit.ts";
import { pick } from "./control-flow.ts";

// Three distinct sub-rects so a test can read back which frame was sampled, at
// 2 fps so a frame boundary falls every 0.5s.
const FRAMES: readonly Rect[] = [
  { height: 1, width: 1, x: 0, y: 0 },
  { height: 1, width: 1, x: 10, y: 0 },
  { height: 1, width: 1, x: 20, y: 0 },
];
const ANIM: SpriteAnimation = { fps: 2, frames: FRAMES };

const bound = (): FakeHost => {
  const host = new FakeHost();
  bindNative(host);
  return host;
};

test("sampleAnimation indexes endpoints and the mid-frame via the native sampler", () => {
  bound();
  // index = floor(elapsed * fps): the first frame at 0s, still the first at the
  // mid-frame 0.25s (floor(0.5) = 0), then the frame boundaries at 0.5s and 1.0s.
  assert.deepEqual(sampleAnimation(ANIM, 0), FRAMES[0]);
  assert.deepEqual(sampleAnimation(ANIM, 0.25), FRAMES[0]);
  assert.deepEqual(sampleAnimation(ANIM, 0.5), FRAMES[1]);
  assert.deepEqual(sampleAnimation(ANIM, 1), FRAMES[2]);
});

test("sampleAnimation loops past the end by default, but clamps when loop is false", () => {
  const host = bound();
  // elapsed 2.0s ⇒ index floor(4.0) = 4, past the 3 frames. The omitted `loop`
  // defaults to true (wrap: 4 mod 3 = 1); an explicit false clamps to the last.
  assert.deepEqual(sampleAnimation(ANIM, 2), FRAMES[1]);
  assert.deepEqual(sampleAnimation(ANIM, 2, true), FRAMES[1]);
  assert.deepEqual(sampleAnimation(ANIM, 2, false), FRAMES[2]);
  // The facade resolves the optional `loop` to a concrete boolean before crossing
  // the bridge: omitted ⇒ true, explicit values pass through unchanged.
  assert.deepEqual(
    host.draw2dSamples.map((sample) => sample.looping),
    [true, true, false],
  );
});

test("sampleAnimation forwards the animation + elapsed to the native sampler (no TS re-derivation)", () => {
  const host = bound();
  sampleAnimation(ANIM, 0.5);
  assert.equal(host.draw2dSamples.length, 1);
  const sample = pick(host.draw2dSamples, 0);
  assert.deepEqual(sample.anim.frames, FRAMES);
  assert.equal(sample.anim.fps, 2);
  assert.equal(sample.elapsedSeconds, 0.5);
});

test("sampleAnimation of an empty book is the inert zero-rect", () => {
  bound();
  const empty: SpriteAnimation = { fps: 24, frames: [] };
  assert.deepEqual(sampleAnimation(empty, 1), { height: 0, width: 0, x: 0, y: 0 });
  assert.deepEqual(sampleAnimation(empty, 1, false), { height: 0, width: 0, x: 0, y: 0 });
});

test("sampleAnimation is chunk-stable: the same elapsed yields the same frame however reached (§7)", () => {
  bound();
  // A pure function of TOTAL elapsed: sampling a list of absolute times directly
  // equals reaching those same times incrementally (cumulative deltas), so the
  // sampled Rect sequence is identical under any partition of the same elapsed.
  const deltas = [0.5, 0.5];
  const times: number[] = [];
  let elapsed = 0;
  for (const delta of deltas) {
    elapsed += delta;
    times.push(elapsed);
  }
  const incremental = times.map((time): Rect => sampleAnimation(ANIM, time));
  const direct = [0.5, 1].map((time): Rect => sampleAnimation(ANIM, time));
  assert.deepEqual(incremental, direct);
  assert.deepEqual(direct, [FRAMES[1], FRAMES[2]]);
});
