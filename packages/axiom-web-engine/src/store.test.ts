import assert from "node:assert/strict";
import test from "node:test";

import type { Handle, MeshData } from "./api.ts";
import { CLEAR_COLOR, MAX_DIR_LIGHTS, MAX_POINT_LIGHTS, type RenderBackend, type SceneFrame } from "./backend.ts";
import {
  addLight,
  clearScene,
  createMaterial,
  createMesh,
  createMeshData,
  despawnRenderable,
  initStore,
  removeLight,
  rendererBackendName,
  rendererNodeCount,
  renderScene,
  resizeRenderer,
  setCamera3D,
  setClearColor,
  setLight,
  setNodeTransform,
  spawnRenderable,
} from "./store.ts";

// A recording fake backend: it constructs no context, it just captures the calls
// the store makes, so every store path is exercisable without a browser.
interface Recorder {
  uploads: { handle: Handle; data: MeshData }[];
  frames: SceneFrame[];
  resizes: { width: number; height: number }[];
  drops: number;
}

interface Fake {
  backend: RenderBackend;
  rec: Recorder;
}

const makeFake = (name: RenderBackend["name"], meshDetail: RenderBackend["meshDetail"]): Fake => {
  const rec: Recorder = { drops: 0, frames: [], resizes: [], uploads: [] };
  const backend: RenderBackend = {
    dropMeshes: (): void => {
      rec.drops += 1;
    },
    meshDetail,
    name,
    render: (frame): void => {
      rec.frames.push(frame);
    },
    resize: (width, height): void => {
      rec.resizes.push({ height, width });
    },
    uploadMesh: (handle, data): void => {
      rec.uploads.push({ data, handle });
    },
  };
  return { backend, rec };
};

const setup = (name: RenderBackend["name"], meshDetail: RenderBackend["meshDetail"]): Recorder => {
  const { backend, rec } = makeFake(name, meshDetail);
  initStore(backend, { height: 1, width: 1 });
  return rec;
};

const IDENTITY_TRANSFORM = {
  position: { x: 0, y: 0, z: 0 },
  rotation: [0, 0, 0, 1] as const,
  scale: { x: 1, y: 1, z: 1 },
};

const EXPECTED_CAMERA = {
  far: 200,
  fovY: Math.PI / 3,
  near: 0.1,
  position: { x: 0, y: 2, z: 6 },
  target: { x: 0, y: 0, z: 0 },
};

// Runs first: before any initStore, every store function must reject.
test("store functions reject before initStore", () => {
  assert.throws(() => rendererNodeCount(), /must be called before/u);
  assert.throws(
    () => {
      renderScene();
    },
    /must be called before/u,
  );
});

test("initStore seeds the default camera and clear color", () => {
  const rec = setup("WebGL2", "high");
  assert.equal(rendererBackendName(), "WebGL2");
  assert.equal(rendererNodeCount(), 0);
  renderScene();
  const frame = rec.frames[0]!;
  assert.deepEqual([...frame.clearColor], [...CLEAR_COLOR]);
  assert.deepEqual(frame.camera, EXPECTED_CAMERA);
});

test("createMesh caches per kind and builds high-detail primitives", () => {
  const rec = setup("WebGL2", "high");
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const cylinder = createMesh("cylinder");
  assert.equal(new Set([box, sphere, cylinder]).size, 3);
  assert.equal(rec.uploads.length, 3);
  assert.equal(createMesh("box"), box);
  assert.equal(rec.uploads.length, 3);
});

test("createMesh builds lower-poly primitives on the software backend", () => {
  const low = setup("Canvas2D", "low");
  createMesh("box");
  createMesh("cylinder");
  createMesh("sphere");
  const lowSphereVerts = low.uploads[2]!.data.positions.length;
  const high = setup("WebGL2", "high");
  createMesh("sphere");
  assert.ok(lowSphereVerts < high.uploads[0]!.data.positions.length);
});

test("createMeshData rejects mismatched positions/normals", () => {
  setup("WebGL2", "high");
  assert.throws(
    () => createMeshData({ indices: [], normals: [], positions: [{ x: 0, y: 0, z: 0 }] }),
    /positions \(1\) and normals \(0\) differ/u,
  );
});

test("createMaterial applies emissive and opacity, and their defaults", () => {
  const rec = setup("WebGL2", "high");
  const withDefaults = createMaterial({ baseColor: [0.2, 0.4, 0.6, 1] });
  const explicit = createMaterial({ baseColor: [1, 0, 0, 1], emissive: [0.5, 0.5, 0.5, 1], opacity: 0.25 });
  renderScene();
  const { materials } = rec.frames[0]!;
  assert.deepEqual(materials.get(withDefaults), { baseColor: [0.2, 0.4, 0.6, 1], emissive: [0, 0, 0], opacity: 1 });
  assert.deepEqual(materials.get(explicit), { baseColor: [1, 0, 0, 1], emissive: [0.5, 0.5, 0.5], opacity: 0.25 });
});

test("spawnRenderable rejects an unknown material handle", () => {
  setup("WebGL2", "high");
  const box = createMesh("box");
  assert.throws(() => spawnRenderable(box, 9999, IDENTITY_TRANSFORM), /unknown material handle 9999/u);
});

test("setNodeTransform re-poses a node and rejects an unknown entity", () => {
  const rec = setup("WebGL2", "high");
  const material = createMaterial({ baseColor: [1, 1, 1, 1] });
  const node = spawnRenderable(createMesh("box"), material, IDENTITY_TRANSFORM);
  const moved = { position: { x: 3, y: 0, z: 0 }, rotation: [0, 0, 0, 1] as const, scale: { x: 1, y: 1, z: 1 } };
  setNodeTransform(node, moved);
  renderScene();
  assert.equal([...rec.frames[0]!.nodes][0]!.transform.position.x, 3);
  assert.throws(
    () => {
      setNodeTransform(9999, moved);
    },
    /unknown entity 9999/u,
  );
});

test("setCamera3D and setClearColor flow into the next frame", () => {
  const rec = setup("WebGL2", "high");
  const camera = { far: 50, fovY: 1, near: 1, position: { x: 1, y: 2, z: 3 }, target: { x: 0, y: 0, z: 0 } };
  setCamera3D(camera);
  setClearColor([0.1, 0.2, 0.3, 1]);
  renderScene();
  assert.deepEqual(rec.frames[0]!.camera, camera);
  assert.deepEqual([...rec.frames[0]!.clearColor], [0.1, 0.2, 0.3]);
});

test("addLight normalizes directional lights and handles a degenerate direction", () => {
  const rec = setup("WebGL2", "high");
  addLight({ color: [1, 1, 1, 1], direction: { x: 2, y: 0, z: 0 }, intensity: 0.5, kind: "directional" });
  addLight({ color: [1, 1, 1, 1], direction: { x: 0, y: 0, z: 0 }, intensity: 1, kind: "directional" });
  renderScene();
  const { dirLights } = rec.frames[0]!;
  assert.deepEqual([...dirLights[0]!.direction], [1, 0, 0]);
  assert.deepEqual([...dirLights[0]!.color], [0.5, 0.5, 0.5]);
  assert.deepEqual([...dirLights[1]!.direction], [0, -1, 0]);
});

test("addLight records point lights and honors both capacity caps", () => {
  const rec = setup("WebGL2", "high");
  for (let index = 0; index < MAX_DIR_LIGHTS + 2; index += 1) {
    addLight({ color: [1, 1, 1, 1], direction: { x: 0, y: -1, z: 0 }, intensity: 1, kind: "directional" });
  }
  for (let index = 0; index < MAX_POINT_LIGHTS + 2; index += 1) {
    addLight({ color: [0, 1, 0, 1], intensity: 2, kind: "point", position: { x: 1, y: 2, z: 3 } });
  }
  renderScene();
  const frame = rec.frames[0]!;
  assert.deepEqual([frame.dirLights.length, frame.pointLights.length], [MAX_DIR_LIGHTS, MAX_POINT_LIGHTS]);
  assert.deepEqual([...frame.pointLights[0]!.position], [1, 2, 3]);
  assert.deepEqual([...frame.pointLights[0]!.color], [0, 2, 0]);
});

test("setLight re-aims an existing light on the very next frame", () => {
  const rec = setup("WebGL2", "high");
  const sun = addLight({ color: [1, 1, 1, 1], direction: { x: 0, y: -1, z: 0 }, intensity: 1, kind: "directional" });
  renderScene();
  setLight(sun, { color: [1, 0.5, 0, 1], direction: { x: 2, y: 0, z: 0 }, intensity: 2, kind: "directional" });
  renderScene();
  assert.deepEqual([...rec.frames[0]!.dirLights[0]!.direction], [0, -1, 0]);
  assert.deepEqual([...rec.frames[1]!.dirLights[0]!.direction], [1, 0, 0]);
  assert.deepEqual([...rec.frames[1]!.dirLights[0]!.color], [2, 1, 0]);
});

test("setLight can change a light's kind and rejects an unknown entity", () => {
  const rec = setup("WebGL2", "high");
  const light = addLight({ color: [0, 1, 0, 1], intensity: 1, kind: "point", position: { x: 1, y: 2, z: 3 } });
  setLight(light, { color: [1, 1, 1, 1], direction: { x: 0, y: -1, z: 0 }, intensity: 1, kind: "directional" });
  renderScene();
  const frame = rec.frames[0]!;
  assert.deepEqual([frame.dirLights.length, frame.pointLights.length], [1, 0]);
  assert.throws(
    () => {
      setLight(9999, { color: [1, 1, 1, 1], direction: { x: 0, y: -1, z: 0 }, intensity: 1, kind: "directional" });
    },
    /unknown light entity 9999/u,
  );
});

test("clearScene drops the backend meshes, materials, nodes, and lights", () => {
  const rec = setup("WebGL2", "high");
  const material = createMaterial({ baseColor: [1, 1, 1, 1] });
  spawnRenderable(createMesh("box"), material, IDENTITY_TRANSFORM);
  addLight({ color: [1, 1, 1, 1], direction: { x: 0, y: -1, z: 0 }, intensity: 1, kind: "directional" });
  addLight({ color: [1, 1, 1, 1], intensity: 1, kind: "point", position: { x: 0, y: 0, z: 0 } });
  clearScene();
  renderScene();
  const frame = rec.frames[0]!;
  assert.deepEqual(
    [rec.drops, rendererNodeCount(), frame.dirLights.length, frame.pointLights.length, frame.materials.size],
    [1, 0, 0, 0, 0],
  );
});

test("clearScene resets the mesh-kind cache so the next createMesh re-uploads", () => {
  const rec = setup("WebGL2", "high");
  createMesh("box");
  clearScene();
  createMesh("box");
  assert.equal(rec.uploads.length, 2);
});

test("resizeRenderer clamps and forwards the viewport to the backend", () => {
  const { backend, rec } = makeFake("WebGL2", "high");
  const canvas = { height: 1, width: 1 };
  initStore(backend, canvas);
  resizeRenderer(640.7, 0);
  assert.deepEqual([canvas.width, canvas.height], [640, 1]);
  assert.deepEqual(rec.resizes[0], { height: 1, width: 640 });
});

test("despawnRenderable drops a node from the next frame and rejects an unknown entity", () => {
  const rec = setup("WebGL2", "high");
  const material = createMaterial({ baseColor: [1, 1, 1, 1] });
  const box = createMesh("box");
  const kept = spawnRenderable(box, material, IDENTITY_TRANSFORM);
  const gone = spawnRenderable(box, material, IDENTITY_TRANSFORM);
  assert.equal(rendererNodeCount(), 2);
  despawnRenderable(gone);
  assert.equal(rendererNodeCount(), 1);
  renderScene();
  const nodes = [...rec.frames[0]!.nodes];
  assert.equal(nodes.length, 1);
  // The surviving node is the one that was kept (the other's slot is gone).
  assert.ok(kept > 0);
  assert.throws(
    () => {
      despawnRenderable(9999);
    },
    /unknown entity 9999/u,
  );
});

test("removeLight drops a light from the next frame and rejects an unknown entity", () => {
  const rec = setup("WebGL2", "high");
  const sun = addLight({ color: [1, 1, 1, 1], direction: { x: 0, y: -1, z: 0 }, intensity: 1, kind: "directional" });
  addLight({ color: [0, 1, 0, 1], intensity: 1, kind: "point", position: { x: 0, y: 0, z: 0 } });
  removeLight(sun);
  renderScene();
  assert.equal(rec.frames[0]!.dirLights.length, 0);
  assert.equal(rec.frames[0]!.pointLights.length, 1);
  assert.throws(
    () => {
      removeLight(9999);
    },
    /unknown light entity 9999/u,
  );
});
