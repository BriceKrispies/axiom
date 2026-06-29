import assert from "node:assert/strict";
import { test } from "node:test";

import type { EmitterConfig } from "../src/draw2d-binding.ts";
import { bindNative, boundHost } from "../src/host-binding.ts";
import { makeFrame } from "../src/sim.ts";
import type { Rgba } from "../src/vocabulary.ts";
import { FakeHost } from "./fake-host.ts";

const RED: Rgba = [1, 0, 0, 1];
const GREEN: Rgba = [0, 1, 0, 1];

const emitterConfig: EmitterConfig = {
  colorEnd: [0, 0, 0, 0],
  colorStart: [1, 1, 1, 1],
  count: 8,
  gravity: { x: 0, y: -4 },
  layer: 5,
  lifetimeSeconds: 2,
  size: 0.5,
  speed: 10,
  spread: 0.25,
};

// Runs FIRST, before any bindNative in this file, so boundHost() is the inert
// UNBOUND_HOST: every 2D draw is a safe no-op and every id-returning verb mints
// the null handle / empty list. (node:test isolates each file in its own process.)
test("the unbound 2D surface is inert until a host is bound", () => {
  const inert = boundHost();
  assert.doesNotThrow(() => {
    inert.draw2dRect({ height: 1, width: 1, x: 0, y: 0 }, { fill: RED });
    inert.draw2dCircle({ x: 0, y: 0 }, 1, { fill: RED });
    inert.draw2dEmit(0, { x: 0, y: 0 }, { x: 1, y: 0 });
    inert.draw2dAdvanceParticles(0.016);
    inert.draw2dBeginTarget(0);
    inert.draw2dEndTarget();
  });
  assert.equal(inert.draw2dCreateEmitter(emitterConfig), 0);
  assert.equal(inert.draw2dCreateRenderTarget(64, 32), 0);
  assert.equal(inert.draw2dTargetTexture(0), 0);
  assert.deepEqual(inert.draw2dFinish(), []);
});

test("frame.rect and frame.circle forward the geometry and style to the bridge", () => {
  const host = new FakeHost();
  bindNative(host);
  const frame = makeFrame(7);
  frame.rect({ height: 4, width: 3, x: 1, y: 2 }, { alpha: 0.5, fill: RED, layer: 2 });
  frame.circle({ x: 5, y: 6 }, 7, { fill: GREEN });
  assert.deepEqual(host.draw2dRects, [
    { bounds: { height: 4, width: 3, x: 1, y: 2 }, style: { alpha: 0.5, fill: RED, layer: 2 } },
  ]);
  assert.deepEqual(host.draw2dCircles, [{ center: { x: 5, y: 6 }, radius: 7, style: { fill: GREEN } }]);
});

test("frame particle verbs forward to the emitter bridge", () => {
  const host = new FakeHost();
  bindNative(host);
  const frame = makeFrame(0);
  const id = frame.createEmitter(emitterConfig);
  assert.equal(id, 1); // distinct minted handle
  assert.deepEqual(host.draw2dEmitters, [emitterConfig]);
  frame.emit(id, { x: 0, y: 0 }, { x: 1, y: 0 });
  assert.deepEqual(host.draw2dEmits, [{ at: { x: 0, y: 0 }, direction: { x: 1, y: 0 }, id: 1 }]);
  frame.advanceParticles(0.016);
  assert.deepEqual(host.draw2dAdvances, [0.016]);
});

test("frame render-target verbs route the inner draws and drain the command list", () => {
  const host = new FakeHost();
  bindNative(host);
  host.draw2dFinishReturn = [2, 1, 0];
  const frame = makeFrame(3);
  const target = frame.createRenderTarget(64, 32);
  assert.equal(target, 1);
  assert.deepEqual(host.draw2dTargets, [{ height: 32, width: 64 }]);
  assert.equal(frame.targetTexture(target), target);

  let routedTick = -1;
  frame.drawTo(target, (inner) => {
    routedTick = inner.tick; // the routed frame is a real Frame carrying the same tick
    inner.rect({ height: 1, width: 1, x: 0, y: 0 }, { fill: GREEN });
  });
  assert.equal(routedTick, 3);
  assert.deepEqual(host.draw2dBegins, [target]); // begin before the inner draws
  assert.equal(host.draw2dEnds, 1); // end after them
  assert.deepEqual(host.draw2dRects, [{ bounds: { height: 1, width: 1, x: 0, y: 0 }, style: { fill: GREEN } }]);

  assert.deepEqual(frame.finish(), [2, 1, 0]);
});
