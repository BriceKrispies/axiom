import assert from "node:assert/strict";
import { test } from "node:test";

import {
  addLight,
  clearScene,
  controlFirstPerson,
  createController,
  createMaterial,
  createMesh,
  createMeshData,
  setCamera3D,
  setNodeBounds,
  setNodeTransform,
  spawnRenderable,
} from "./scene3d.ts";
import { bindNative } from "./host-binding.ts";
import { FakeHost } from "./fake-host.testkit.ts";
import type { Transform, Vec3 } from "./vocabulary.ts";

/** An identity-rotation transform at `position` with uniform `scale`. */
const pose = (position: Vec3, scale: number): Transform => ({
  position,
  rotation: [0, 0, 0, 1],
  scale: { x: scale, y: scale, z: scale },
});

test("createMesh resolves each primitive to its dense native kind index", () => {
  const host = new FakeHost();
  bindNative(host);
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const cylinder = createMesh("cylinder");
  assert.deepEqual(host.meshKinds, [0, 1, 2]);
  assert.deepEqual([box, sphere, cylinder], [1, 2, 3]); // distinct opaque handles
});

test("createMeshData forwards author geometry with explicit UVs and returns a handle", () => {
  const host = new FakeHost();
  bindNative(host);
  const handle = createMeshData({
    indices: [0, 1, 2],
    normals: [
      { x: 0, y: 0, z: 1 },
      { x: 0, y: 0, z: 1 },
      { x: 0, y: 0, z: 1 },
    ],
    positions: [
      { x: 0, y: 0, z: 0 },
      { x: 1, y: 0, z: 0 },
      { x: 0, y: 1, z: 0 },
    ],
    uvs: [
      { x: 0, y: 0 },
      { x: 1, y: 0 },
      { x: 0, y: 1 },
    ],
  });
  assert.equal(handle, 1); // a fresh opaque mesh handle
  assert.deepEqual(host.meshDatas, [
    {
      indices: [0, 1, 2],
      normals: [
        { x: 0, y: 0, z: 1 },
        { x: 0, y: 0, z: 1 },
        { x: 0, y: 0, z: 1 },
      ],
      positions: [
        { x: 0, y: 0, z: 0 },
        { x: 1, y: 0, z: 0 },
        { x: 0, y: 1, z: 0 },
      ],
      uvs: [
        { x: 0, y: 0 },
        { x: 1, y: 0 },
        { x: 0, y: 1 },
      ],
    },
  ]);
});

test("createMeshData defaults omitted UVs to the empty list the engine fills", () => {
  const host = new FakeHost();
  bindNative(host);
  createMeshData({
    indices: [0, 1, 2],
    normals: [
      { x: 0, y: 0, z: 1 },
      { x: 0, y: 0, z: 1 },
      { x: 0, y: 0, z: 1 },
    ],
    positions: [
      { x: 0, y: 0, z: 0 },
      { x: 1, y: 0, z: 0 },
      { x: 0, y: 1, z: 0 },
    ],
  });
  // The optional `uvs` resolves to `[]` (the engine fills the origin per vertex).
  assert.deepEqual(host.meshDatas, [
    {
      indices: [0, 1, 2],
      normals: [
        { x: 0, y: 0, z: 1 },
        { x: 0, y: 0, z: 1 },
        { x: 0, y: 0, z: 1 },
      ],
      positions: [
        { x: 0, y: 0, z: 0 },
        { x: 1, y: 0, z: 0 },
        { x: 0, y: 1, z: 0 },
      ],
      uvs: [],
    },
  ]);
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

test("spawnRenderable places a mesh+material at a transform and returns its entity", () => {
  const host = new FakeHost();
  bindNative(host);
  const mesh = createMesh("box");
  const material = createMaterial({ baseColor: [1, 0, 0, 1] });
  const node = spawnRenderable(mesh, material, pose({ x: 2, y: 0, z: -3 }, 1));
  assert.deepEqual(host.spawns, [{ material, mesh, transform: pose({ x: 2, y: 0, z: -3 }, 1) }]);
  assert.equal(node, 3); // a fresh entity after the mesh (1) and material (2) handles
});

test("setNodeTransform and setNodeBounds forward the node's pose and box", () => {
  const host = new FakeHost();
  bindNative(host);
  setNodeTransform(7, pose({ x: 1, y: 2, z: 3 }, 2));
  setNodeBounds(7, { x: 0.5, y: 1, z: 0.5 });
  assert.deepEqual(host.nodeTransforms, [{ entity: 7, transform: pose({ x: 1, y: 2, z: 3 }, 2) }]);
  assert.deepEqual(host.nodeBounds, [{ entity: 7, halfExtents: { x: 0.5, y: 1, z: 0.5 } }]);
});

test("clearScene forwards the blank-the-scene signal", () => {
  const host = new FakeHost();
  bindNative(host);
  clearScene();
  clearScene();
  assert.equal(host.sceneClears, 2);
});

test("createController forwards the placement + index (defaulting to the root controller)", () => {
  const host = new FakeHost();
  bindNative(host);
  const spec = { far: 100, fovY: 1.2, near: 0.05, position: { x: 1, y: 1, z: 5 } };
  const cam = createController(spec);
  createController(spec, 2);
  assert.deepEqual(host.controllers, [
    { index: 0, spec }, // default root controller
    { index: 2, spec },
  ]);
  assert.equal(cam, 1); // a fresh entity handle
});

test("controlFirstPerson forwards the resolved per-frame input (index defaults to root)", () => {
  const host = new FakeHost();
  bindNative(host);
  controlFirstPerson({ moveLocal: { x: 0, y: 0, z: -1 }, pitchDelta: 0.1, yawDelta: 0.2 });
  controlFirstPerson({ index: 3, moveLocal: { x: 1, y: 0, z: 0 }, pitchDelta: 0, yawDelta: -0.5 });
  assert.deepEqual(host.controls, [
    { index: 0, moveLocal: { x: 0, y: 0, z: -1 }, pitchDelta: 0.1, yawDelta: 0.2 },
    { index: 3, moveLocal: { x: 1, y: 0, z: 0 }, pitchDelta: 0, yawDelta: -0.5 },
  ]);
});
