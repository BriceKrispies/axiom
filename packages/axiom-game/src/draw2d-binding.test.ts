import assert from "node:assert/strict";
import { test } from "node:test";

import { type EmitterConfig, UNBOUND_DRAW2D } from "./draw2d-binding.ts";
import type { Rgba } from "./vocabulary.ts";

const RED: Rgba = [1, 0, 0, 1];

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

test("the inert UNBOUND_DRAW2D surface makes every draw verb a safe no-op", () => {
  assert.doesNotThrow(() => {
    UNBOUND_DRAW2D.draw2dCamera2d({ x: 0, y: 0 }, 1);
    UNBOUND_DRAW2D.draw2dRect({ height: 1, width: 1, x: 0, y: 0 }, { fill: RED });
    UNBOUND_DRAW2D.draw2dCircle({ x: 0, y: 0 }, 1, { fill: RED });
    UNBOUND_DRAW2D.draw2dEllipse({ x: 0, y: 0 }, { rotation: 0, rx: 2, ry: 1 }, { fill: RED });
    UNBOUND_DRAW2D.draw2dLine({ x: 0, y: 0 }, { x: 1, y: 1 }, { color: RED, width: 1 });
    UNBOUND_DRAW2D.draw2dEmit(0, { x: 0, y: 0 }, { x: 1, y: 0 });
    UNBOUND_DRAW2D.draw2dAdvanceParticles(0.016);
    UNBOUND_DRAW2D.draw2dBeginTarget(0);
    UNBOUND_DRAW2D.draw2dEndTarget();
  });
});

test("the inert UNBOUND_DRAW2D id-returning verbs mint the null handle / empty list", () => {
  assert.equal(UNBOUND_DRAW2D.draw2dCreateEmitter(emitterConfig), 0);
  assert.equal(UNBOUND_DRAW2D.draw2dCreateRenderTarget(64, 32), 0);
  assert.equal(UNBOUND_DRAW2D.draw2dTargetTexture(5), 0);
  assert.deepEqual(UNBOUND_DRAW2D.draw2dFinish(), []);
});

test("the inert UNBOUND_DRAW2D flip-book sampler returns the inert zero-rect", () => {
  const frame = UNBOUND_DRAW2D.draw2dSampleAnimation(
    { fps: 12, frames: [{ height: 1, width: 1, x: 0, y: 0 }] },
    1,
    true,
  );
  assert.deepEqual(frame, { height: 0, width: 0, x: 0, y: 0 });
});

test("the inert UNBOUND_DRAW2D sprite + text verbs are safe no-ops", () => {
  assert.doesNotThrow(() => {
    UNBOUND_DRAW2D.draw2dSprite(1, { pos: { x: 0, y: 0 } });
    UNBOUND_DRAW2D.draw2dText("hi", { color: RED, font: { family: "monospace", size: 16 }, pos: { x: 0, y: 0 } });
  });
});

test("the inert UNBOUND_DRAW2D measureText returns the inert zero extent", () => {
  assert.deepEqual(
    UNBOUND_DRAW2D.draw2dMeasureText("hi", { family: "monospace", size: 16 }),
    { height: 0, width: 0 },
  );
});
