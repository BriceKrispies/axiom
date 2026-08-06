import assert from "node:assert/strict";
import test from "node:test";

import type { Handle, MeshData } from "./api.ts";
import { FULL_DETAIL_SCALE, SOFTWARE_DETAIL_SCALE } from "./tessellation.ts";
import { CLEAR_COLOR, DEFAULT_AMBIENT, MAX_DIR_LIGHTS, MAX_POINT_LIGHTS, type RenderBackend, type SceneFrame } from "./backend.ts";
import {
  addLight,
  clearScene,
  despawnRenderable,
  initStore,
  removeLight,
  rendererBackendName,
  rendererNodeCount,
  renderScene,
  resizeRenderer,
  setAmbient,
  setLabels,
  setCamera3D,
  setClearColor,
  setLight,
  setNodeTransform,
  spawnRenderable,
} from "./store.ts";
import { createMaterial, createMesh, createMeshData } from "./store-resources.ts";
import { fromTrs, normalMatrix } from "./mat4.ts";

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

const makeFake = (name: RenderBackend["name"], detailScale: RenderBackend["detailScale"]): Fake => {
  const rec: Recorder = { drops: 0, frames: [], resizes: [], uploads: [] };
  const backend: RenderBackend = {
    dropMeshes: (): void => {
      rec.drops += 1;
    },
    detailScale,
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

const setup = (name: RenderBackend["name"], detailScale: RenderBackend["detailScale"]): Recorder => {
  const { backend, rec } = makeFake(name, detailScale);
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

test("initStore seeds the default camera, clear color, and ambient", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  assert.equal(rendererBackendName(), "WebGL2");
  assert.equal(rendererNodeCount(), 0);
  renderScene();
  const frame = rec.frames[0]!;
  assert.deepEqual([...frame.clearColor], [...CLEAR_COLOR]);
  assert.deepEqual([...frame.ambient], [...DEFAULT_AMBIENT]);
  assert.deepEqual(frame.camera, EXPECTED_CAMERA);
});

test("createMesh caches per kind and builds high-detail primitives", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  const box = createMesh("box");
  const sphere = createMesh("sphere");
  const cylinder = createMesh("cylinder");
  assert.equal(new Set([box, sphere, cylinder]).size, 3);
  assert.equal(rec.uploads.length, 3);
  assert.equal(createMesh("box"), box);
  assert.equal(rec.uploads.length, 3);
});

test("createMesh builds lower-poly primitives on the software backend", () => {
  const low = setup("Canvas2D", SOFTWARE_DETAIL_SCALE);
  createMesh("box");
  createMesh("cylinder");
  createMesh("sphere");
  const lowSphereVerts = low.uploads[2]!.data.positions.length;
  const high = setup("WebGL2", FULL_DETAIL_SCALE);
  createMesh("sphere");
  assert.ok(lowSphereVerts < high.uploads[0]!.data.positions.length);
});

test("createMesh reproduces the historical fixed counts when no budget is given", () => {
  const high = setup("WebGL2", FULL_DETAIL_SCALE);
  createMesh("cylinder");
  createMesh("sphere");
  // 24-segment cylinder: 2·(24+1) wall verts + 2·(24+2) cap verts. 16×24 sphere.
  assert.equal(high.uploads[0]!.data.positions.length, 2 * 25 + 2 * 26);
  assert.equal(high.uploads[1]!.data.positions.length, 17 * 25);
  const low = setup("Canvas2D", SOFTWARE_DETAIL_SCALE);
  createMesh("cylinder");
  createMesh("sphere");
  // Halved by the software detail scale: 12 segments, and an 8×12 sphere.
  assert.equal(low.uploads[0]!.data.positions.length, 2 * 13 + 2 * 14);
  assert.equal(low.uploads[1]!.data.positions.length, 9 * 13);
});

test("createMesh caches per (kind, budget) so a big primitive can be smooth alone", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  const rivet = createMesh("cylinder");
  const lagoon = createMesh("cylinder", 72);
  assert.notEqual(rivet, lagoon, "a distinct budget is distinct geometry, not a replacement");
  assert.equal(rec.uploads.length, 2);
  assert.ok(rec.uploads[1]!.data.positions.length > rec.uploads[0]!.data.positions.length);
  assert.equal(createMesh("cylinder", 72), lagoon, "the same budget reuses the upload");
  assert.equal(createMesh("cylinder"), rivet, "the default budget is untouched by the big one");
  assert.equal(rec.uploads.length, 2);
});

test("createMesh applies the software detail scale to a requested budget", () => {
  const low = setup("Canvas2D", SOFTWARE_DETAIL_SCALE);
  createMesh("cylinder", 72);
  // The backend LOD still halves it: 36 segments, not the requested 72.
  assert.equal(low.uploads[0]!.data.positions.length, 2 * 37 + 2 * 38);
});

test("createMesh floors a degenerate budget at a closable ring", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  createMesh("cylinder", 0);
  assert.equal(rec.uploads[0]!.data.positions.length, 2 * 4 + 2 * 5, "clamped to 3 segments");
  // A sphere's latitude rings derive from the radial budget, and floor too.
  createMesh("sphere", 1);
  assert.equal(rec.uploads[1]!.data.positions.length, 4 * 4, "3 lat × 3 lon");
});

test("createMeshData rejects mismatched positions/normals", () => {
  setup("WebGL2", FULL_DETAIL_SCALE);
  assert.throws(
    () => createMeshData({ indices: [], normals: [], positions: [{ x: 0, y: 0, z: 0 }] }),
    /positions \(1\) and normals \(0\) differ/u,
  );
});

test("createMeshData accepts a per-vertex ao array and forwards it to the backend", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  const data = {
    ao: [0.5, 0.75],
    indices: [0, 1, 0],
    normals: [
      { x: 0, y: 1, z: 0 },
      { x: 0, y: 1, z: 0 },
    ],
    positions: [
      { x: 0, y: 0, z: 0 },
      { x: 1, y: 0, z: 0 },
    ],
  };
  createMeshData(data);
  assert.deepEqual(rec.uploads[0]!.data.ao, [0.5, 0.75]);
});

test("createMeshData rejects an ao array whose length differs from the vertices", () => {
  setup("WebGL2", FULL_DETAIL_SCALE);
  assert.throws(
    () =>
      createMeshData({
        ao: [1],
        indices: [],
        normals: [
          { x: 0, y: 1, z: 0 },
          { x: 0, y: 1, z: 0 },
        ],
        positions: [
          { x: 0, y: 0, z: 0 },
          { x: 1, y: 0, z: 0 },
        ],
      }),
    /ao \(1\) must match positions \(2\)/u,
  );
});

test("createMaterial applies emissive, opacity, roughness, and their defaults", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  const withDefaults = createMaterial({ baseColor: [0.2, 0.4, 0.6, 1] });
  const explicit = createMaterial({
    baseColor: [1, 0, 0, 1],
    emissive: [0.5, 0.5, 0.5, 1],
    opacity: 0.25,
    roughness: 0.2,
  });
  renderScene();
  const { materials } = rec.frames[0]!;
  // An omitted roughness resolves to the fully-matte default (1) — specular off.
  assert.deepEqual(materials.get(withDefaults), {
    baseColor: [0.2, 0.4, 0.6, 1],
    emissive: [0, 0, 0],
    opacity: 1,
    roughness: 1,
  });
  assert.deepEqual(materials.get(explicit), {
    baseColor: [1, 0, 0, 1],
    emissive: [0.5, 0.5, 0.5],
    opacity: 0.25,
    roughness: 0.2,
  });
});

test("spawnRenderable rejects an unknown material handle", () => {
  setup("WebGL2", FULL_DETAIL_SCALE);
  const box = createMesh("box");
  assert.throws(() => spawnRenderable(box, 9999, IDENTITY_TRANSFORM), /unknown material handle 9999/u);
});

test("setNodeTransform re-poses a node and rejects an unknown entity", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
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

// setAmbient is the authorable replacement for the old hard-coded 0.12 floor: the
// authored triple reaches the backend on the very next frame (alpha ignored), so a
// scene can describe a warm sky/bounce environment instead of faking one with a
// near-white directional fill or material emissive.
test("setAmbient flows into the next frame, alpha ignored", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  setAmbient([0.28, 0.25, 0.21, 1]);
  renderScene();
  assert.deepEqual([...rec.frames[0]!.ambient], [0.28, 0.25, 0.21]);
});

// Scene TEXT reaches the backend as frame data, and a frame that sets none carries
// an empty list rather than a missing field — a backend can always iterate it. The
// list is replaced wholesale (labels are few), so setting it again REPLACES rather
// than appends; that is the contract the DOM backend's keyed element pool relies on
// to drop a label whose key stopped being emitted.
test("setLabels replaces the scene text carried to the next frame", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  renderScene();
  assert.deepEqual(rec.frames[0]!.labels, [], "a scene with no text carries an empty list");

  const plaque = {
    color: [1, 1, 1, 1] as const,
    key: "chest4:brand",
    size: 0.2,
    text: "ACME",
    transform: { position: { x: 1, y: 2, z: 3 }, rotation: [0, 0, 0, 1] as const, scale: { x: 1, y: 1, z: 1 } },
  };
  setLabels([plaque]);
  renderScene();
  assert.deepEqual(rec.frames[1]!.labels, [plaque]);

  setLabels([]);
  renderScene();
  assert.deepEqual(rec.frames[2]!.labels, [], "the previous label is gone, not retained");
});

test("setCamera3D and setClearColor flow into the next frame", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  const camera = { far: 50, fovY: 1, near: 1, position: { x: 1, y: 2, z: 3 }, target: { x: 0, y: 0, z: 0 } };
  setCamera3D(camera);
  setClearColor([0.1, 0.2, 0.3, 1]);
  renderScene();
  assert.deepEqual(rec.frames[0]!.camera, camera);
  assert.deepEqual([...rec.frames[0]!.clearColor], [0.1, 0.2, 0.3]);
});

test("addLight normalizes directional lights and handles a degenerate direction", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  addLight({ color: [1, 1, 1, 1], direction: { x: 2, y: 0, z: 0 }, intensity: 0.5, kind: "directional" });
  addLight({ color: [1, 1, 1, 1], direction: { x: 0, y: 0, z: 0 }, intensity: 1, kind: "directional" });
  renderScene();
  const { dirLights } = rec.frames[0]!;
  assert.deepEqual([...dirLights[0]!.direction], [1, 0, 0]);
  assert.deepEqual([...dirLights[0]!.color], [0.5, 0.5, 0.5]);
  assert.deepEqual([...dirLights[1]!.direction], [0, -1, 0]);
});

test("addLight records point lights and honors both capacity caps", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
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
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  const sun = addLight({ color: [1, 1, 1, 1], direction: { x: 0, y: -1, z: 0 }, intensity: 1, kind: "directional" });
  renderScene();
  setLight(sun, { color: [1, 0.5, 0, 1], direction: { x: 2, y: 0, z: 0 }, intensity: 2, kind: "directional" });
  renderScene();
  assert.deepEqual([...rec.frames[0]!.dirLights[0]!.direction], [0, -1, 0]);
  assert.deepEqual([...rec.frames[1]!.dirLights[0]!.direction], [1, 0, 0]);
  assert.deepEqual([...rec.frames[1]!.dirLights[0]!.color], [2, 1, 0]);
});

test("setLight can change a light's kind and rejects an unknown entity", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
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
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
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
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  createMesh("box");
  clearScene();
  createMesh("box");
  assert.equal(rec.uploads.length, 2);
});

test("resizeRenderer clamps and forwards the viewport to the backend", () => {
  const { backend, rec } = makeFake("WebGL2", FULL_DETAIL_SCALE);
  const canvas = { height: 1, width: 1 };
  initStore(backend, canvas);
  resizeRenderer(640.7, 0);
  assert.deepEqual([canvas.width, canvas.height], [640, 1]);
  assert.deepEqual(rec.resizes[0], { height: 1, width: 640 });
});

test("despawnRenderable drops a node from the next frame and rejects an unknown entity", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
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
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
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

/*
 * The per-node matrix cache (`posedNode`). These are the contract the backends
 * rely on: the matrices exist and MATCH what each backend used to compute for
 * itself (so moving the work changes nothing on screen), a re-pose rebuilds them,
 * and a static node reuses the same objects frame after frame — which is the
 * saving itself, stated as an assertion.
 *
 * The stale-cache case is the regression that matters: it would freeze every
 * MOVING object while leaving the still ones correct, which reads as a
 * game-logic fault rather than a renderer one.
 */

const POSED = {
  position: { x: 3, y: -2, z: 5 },
  rotation: [0, Math.SQRT1_2, 0, Math.SQRT1_2] as const,
  scale: { x: 2, y: 2, z: 2 },
};

test("a spawned node carries the model and normal matrices its pose implies", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  const material = createMaterial({ baseColor: [1, 1, 1, 1] });
  spawnRenderable(createMesh("box"), material, POSED);
  renderScene();
  const node = [...rec.frames[0]!.nodes][0]!;
  const expected = fromTrs(POSED.position, POSED.rotation, POSED.scale);
  assert.deepEqual([...node.model], [...expected], "the model matrix the backends used to build per frame");
  assert.deepEqual([...node.normal], [...normalMatrix(expected)], "and its cofactor normal matrix");
});

test("re-posing a node rebuilds its cached matrices", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  const material = createMaterial({ baseColor: [1, 1, 1, 1] });
  const node = spawnRenderable(createMesh("box"), material, IDENTITY_TRANSFORM);
  renderScene();
  const before = [...[...rec.frames[0]!.nodes][0]!.model];
  setNodeTransform(node, POSED);
  renderScene();
  const after = [...[...rec.frames[1]!.nodes][0]!.model];
  assert.notDeepEqual(after, before, "the cache followed the re-pose");
  assert.deepEqual(after, [...fromTrs(POSED.position, POSED.rotation, POSED.scale)]);
});

test("a node the scene never re-poses reuses its matrix objects across frames", () => {
  const rec = setup("WebGL2", FULL_DETAIL_SCALE);
  const material = createMaterial({ baseColor: [1, 1, 1, 1] });
  spawnRenderable(createMesh("box"), material, POSED);
  renderScene();
  renderScene();
  const first = [...rec.frames[0]!.nodes][0]!;
  const second = [...rec.frames[1]!.nodes][0]!;
  // Object identity, not equality: an equal copy would mean it was rebuilt.
  assert.equal(second.model, first.model, "the same Float32Array, not an equal copy");
  assert.equal(second.normal, first.normal);
});
