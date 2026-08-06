/*
 * store-resources.test.ts — the RESOURCE half of the store: geometry uploads,
 * the primitive cache, and material defaults.
 *
 * These paths were covered from `store.test.ts` while the code lived in
 * `store.ts`; the tests move with the code so the co-location gate holds and the
 * coverage report attributes them to the file that owns them. What is tested is
 * the contract callers depend on: a distinct handle per registration, exactly ONE
 * upload per (kind, facet budget), the geometry assertions, and the documented
 * material defaults — including that an unset `roughness` is fully matte, which
 * is what keeps a material authored before the specular lobe existed rendering
 * exactly as it always did.
 */

import assert from "node:assert/strict";
import test from "node:test";

import type { Handle, MeshData } from "./api.ts";
import { FULL_DETAIL_SCALE, SOFTWARE_DETAIL_SCALE } from "./tessellation.ts";
import type { RenderBackend, SceneFrame } from "./backend.ts";
import { initStore, renderScene, spawnRenderable } from "./store.ts";
import { createMaterial, createMesh, createMeshData } from "./store-resources.ts";

interface Recorder {
  uploads: { handle: Handle; data: MeshData }[];
  frames: SceneFrame[];
  resizes: number;
  drops: number;
}

/** A recording fake backend: it constructs no context, it just captures what the
 * store asks of it, so every path here runs without a browser. */
const setup = (detailScale: RenderBackend["detailScale"]): Recorder => {
  const rec: Recorder = { drops: 0, frames: [], resizes: 0, uploads: [] };
  initStore(
    {
      detailScale,
      dropMeshes: (): void => {
        rec.drops += 1;
      },
      name: "WebGL2",
      render: (frame): void => {
        rec.frames.push(frame);
      },
      resize: (): void => {
        rec.resizes += 1;
      },
      uploadMesh: (handle, data): void => {
        rec.uploads.push({ data, handle });
      },
    },
    { height: 1, width: 1 },
  );
  return rec;
};

const IDENTITY_TRANSFORM = {
  position: { x: 0, y: 0, z: 0 },
  rotation: [0, 0, 0, 1] as const,
  scale: { x: 1, y: 1, z: 1 },
};

/** Two vertices is enough: these tests are about bookkeeping, not shape. */
const pair = (): MeshData => ({
  indices: [0, 1, 0],
  normals: [
    { x: 0, y: 1, z: 0 },
    { x: 0, y: 1, z: 0 },
  ],
  positions: [
    { x: 0, y: 0, z: 0 },
    { x: 1, y: 0, z: 0 },
  ],
});

test("createMeshData uploads the geometry and hands back a fresh handle", () => {
  const rec = setup(FULL_DETAIL_SCALE);
  const first = createMeshData(pair());
  const second = createMeshData(pair());
  assert.notEqual(first, second, "each registration gets its own handle");
  assert.deepEqual(rec.uploads.map((u) => u.handle), [first, second], "both reached the backend, in order");
});

test("createMeshData accepts an absent ao array and rejects a mis-sized one", () => {
  setup(FULL_DETAIL_SCALE);
  // Absent ao is legal — the backends default it to 1.0 everywhere.
  assert.ok(createMeshData(pair()) > 0);
  assert.ok(createMeshData({ ...pair(), ao: [1, 1] }) > 0, "one scalar per vertex is legal");
  assert.throws(() => createMeshData({ ...pair(), ao: [1] }), /ao \(1\) must match positions \(2\)/u);
});

test("createMesh caches one upload per (kind, facet budget)", () => {
  const rec = setup(FULL_DETAIL_SCALE);
  const fine = createMesh("sphere", 24);
  assert.equal(createMesh("sphere", 24), fine, "the same budget reuses the cached upload");
  assert.equal(rec.uploads.length, 1, "and uploads only once");
  assert.notEqual(createMesh("sphere", 8), fine, "a different budget is different geometry");
  assert.equal(rec.uploads.length, 2);
});

test("the software backend's detail scale builds strictly less geometry", () => {
  const hard = setup(FULL_DETAIL_SCALE);
  createMesh("cylinder", 24);
  const soft = setup(SOFTWARE_DETAIL_SCALE);
  createMesh("cylinder", 24);
  assert.ok(
    soft.uploads[0]!.data.positions.length < hard.uploads[0]!.data.positions.length,
    "the same request costs the software path fewer facets",
  );
});

test("a material's unset fields take their documented defaults", () => {
  const rec = setup(FULL_DETAIL_SCALE);
  const bare = createMaterial({ baseColor: [0.5, 0.25, 0.125, 1] });
  spawnRenderable(createMesh("box"), bare, IDENTITY_TRANSFORM);
  renderScene();
  const resolved = rec.frames[0]!.materials.get(bare);
  assert.deepEqual(resolved!.baseColor, [0.5, 0.25, 0.125, 1]);
  assert.deepEqual(resolved!.emissive, [0, 0, 0], "no emissive by default");
  assert.equal(resolved!.opacity, 1, "opaque by default");
  // Fully MATTE: glossiness 0 -> zero specular, so a material that predates the
  // specular lobe renders byte-identically to the old Lambert-only path.
  assert.equal(resolved!.roughness, 1, "matte by default");
});

test("a material's given fields survive resolution, and emissive drops its alpha", () => {
  const rec = setup(FULL_DETAIL_SCALE);
  const full = createMaterial({
    baseColor: [1, 1, 1, 1],
    emissive: [0.1, 0.2, 0.3, 1],
    opacity: 0.5,
    roughness: 0.25,
  });
  spawnRenderable(createMesh("box"), full, IDENTITY_TRANSFORM);
  renderScene();
  const resolved = rec.frames[0]!.materials.get(full);
  assert.deepEqual(resolved!.emissive, [0.1, 0.2, 0.3]);
  assert.equal(resolved!.opacity, 0.5);
  assert.equal(resolved!.roughness, 0.25);
});

test("mesh and material handles are drawn from one counter, so they never collide", () => {
  setup(FULL_DETAIL_SCALE);
  const handles = [
    createMeshData(pair()),
    createMaterial({ baseColor: [1, 1, 1, 1] }),
    createMeshData(pair()),
    createMaterial({ baseColor: [0, 0, 0, 1] }),
  ];
  assert.equal(new Set(handles).size, handles.length, "every handle is distinct across both kinds");
});
