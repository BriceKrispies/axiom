import assert from "node:assert/strict";
import { test } from "node:test";

import { addLight, createMaterial, createMesh, setCamera3D } from "../src/scene3d.ts";
import { bindNative } from "../src/host-binding.ts";
import { FakeHost } from "./fake-host.ts";

test("createMesh resolves each primitive to its dense native kind index", () => {
  const host = new FakeHost();
  bindNative(host);
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const cylinder = createMesh("cylinder");
  assert.deepEqual(host.meshKinds, [0, 1, 2]);
  assert.deepEqual([box, sphere, cylinder], [1, 2, 3]); // distinct opaque handles
});

test("createMaterial forwards all fields and defaults the optional ones", () => {
  const host = new FakeHost();
  bindNative(host);
  createMaterial({
    baseColor: [1, 0, 0, 1],
    emissive: [0, 0, 1, 1],
    opacity: 0.5,
    roughness: 0.25,
  });
  createMaterial({ baseColor: [0, 1, 0, 1] });
  assert.deepEqual(host.materials, [
    { baseColor: [1, 0, 0, 1], emissive: [0, 0, 1, 1], opacity: 0.5, roughness: 0.25 },
    // defaults: no emissive, matte roughness, fully opaque
    { baseColor: [0, 1, 0, 1], emissive: [0, 0, 0, 0], opacity: 1, roughness: 1 },
  ]);
});

test("setCamera3D marshals the flat perspective record to the scene facade", () => {
  const host = new FakeHost();
  bindNative(host);
  setCamera3D({
    far: 100,
    fovY: 1.2,
    near: 0.1,
    position: { x: 0, y: 1, z: 5 },
    target: { x: 0, y: 0, z: 0 },
  });
  assert.deepEqual(host.cameras, [
    { far: 100, fovY: 1.2, near: 0.1, position: { x: 0, y: 1, z: 5 }, target: { x: 0, y: 0, z: 0 } },
  ]);
});

test("addLight reads the directional/point vector branchlessly and returns its entity", () => {
  const host = new FakeHost();
  bindNative(host);
  const sun = addLight({
    color: [1, 1, 1, 1],
    direction: { x: 0, y: -1, z: 0 },
    intensity: 2,
    kind: "directional",
  });
  const lamp = addLight({
    color: [1, 0.5, 0, 1],
    intensity: 3,
    kind: "point",
    position: { x: 4, y: 5, z: 6 },
  });
  assert.deepEqual(host.lights, [
    { color: [1, 1, 1, 1], intensity: 2, kind: 0, vector: { x: 0, y: -1, z: 0 } },
    { color: [1, 0.5, 0, 1], intensity: 3, kind: 1, vector: { x: 4, y: 5, z: 6 } },
  ]);
  assert.deepEqual([sun, lamp], [1, 2]); // distinct light entities
});
