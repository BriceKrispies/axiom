/*
 * index.test.ts — pins the public surface of the package. If a re-export is
 * dropped or renamed in index.ts, this stops compiling or fails; it also places
 * the barrel in the co-location graph. (Re-exports carry no runtime regions, so
 * this is a surface contract test, not behavior coverage — the behavior lives in
 * each module's own co-located test.)
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import * as engine from "./index.ts";

test("the retained-scene store + facade are exported as functions", () => {
  const names = [
    "initRenderer",
    "renderScene",
    "createMesh",
    "createMeshData",
    "createMaterial",
    "spawnRenderable",
    "setNodeTransform",
    "setCamera3D",
    "setClearColor",
    "addLight",
    "setLight",
    "clearScene",
    "resizeRenderer",
    "rendererBackendName",
    "rendererNodeCount",
  ];
  for (const name of names) {
    assert.equal(typeof (engine as Record<string, unknown>)[name], "function", `${name} is exported`);
  }
});

test("loop, input, and audio are exported", () => {
  assert.equal(typeof engine.startLoop, "function");
  assert.equal(typeof engine.FixedStepper, "function");
  assert.equal(typeof engine.InputState, "function");
  assert.equal(typeof engine.attachDomInput, "function");
  assert.equal(typeof engine.playTone, "function");
  assert.equal(typeof engine.startAmbience, "function");
  assert.equal(typeof engine.setAmbienceLevel, "function");
  assert.equal(typeof engine.stopAmbience, "function");
});
