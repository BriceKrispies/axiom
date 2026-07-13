/*
 * meshes.test.ts — `node --test` coverage for the procedural unit primitives in
 * `meshes.ts`: vertex/index counts, index validity, unit-size bounds, and unit
 * normals for the box, sphere, and capped cylinder. Assertions ported from the
 * reference renderer's render.test.ts. Pure data, no DOM and no WebGL.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import type { MeshData } from "./api.ts";
import { unitBox, unitCylinderY, unitSphere } from "./meshes.ts";

const v3 = (x: number, y: number, z: number): { x: number; y: number; z: number } => ({ x, y, z });

const assertClose = (actual: number, expected: number, msg: string, eps = 1e-5): void => {
  assert.ok(Math.abs(actual - expected) <= eps, `${msg}: expected ${expected}, got ${actual}`);
};

const assertVecClose = (
  actual: { x: number; y: number; z: number },
  expected: { x: number; y: number; z: number },
  msg: string,
  eps = 1e-5,
): void => {
  assertClose(actual.x, expected.x, `${msg} (x)`, eps);
  assertClose(actual.y, expected.y, `${msg} (y)`, eps);
  assertClose(actual.z, expected.z, `${msg} (z)`, eps);
};

const checkMeshInvariants = (mesh: MeshData, name: string): void => {
  assert.equal(mesh.positions.length, mesh.normals.length, `${name}: one normal per position`);
  assert.ok(mesh.indices.length > 0, `${name}: has triangles`);
  assert.equal(mesh.indices.length % 3, 0, `${name}: indices form whole triangles`);
  for (const index of mesh.indices) {
    assert.ok(Number.isInteger(index), `${name}: integer index`);
    assert.ok(index >= 0 && index < mesh.positions.length, `${name}: index ${index} in range`);
  }
  for (const n of mesh.normals) {
    assertClose(Math.sqrt(n.x * n.x + n.y * n.y + n.z * n.z), 1, `${name}: unit normal`, 1e-6);
  }
};

test("unitBox is a unit cube with flat per-face normals", () => {
  const box = unitBox();
  checkMeshInvariants(box, "box");
  assert.equal(box.positions.length, 24, "24 vertices (4 per face)");
  assert.equal(box.indices.length, 36, "12 triangles");
  for (const p of box.positions) {
    for (const c of [p.x, p.y, p.z]) {
      assert.ok(Math.abs(c) <= 0.5 + 1e-6, `corner component |${c}| ≤ 0.5`);
      assertClose(Math.abs(c), 0.5, "every box coordinate sits on a ±0.5 face", 1e-6);
    }
  }
  // Every vertex's normal is axis-aligned and points out of the face it is on.
  for (let i = 0; i < box.positions.length; i += 1) {
    const p = box.positions[i]!;
    const n = box.normals[i]!;
    assertClose(p.x * n.x + p.y * n.y + p.z * n.z, 0.5, "normal points out of its face", 1e-6);
  }
});

test("unitSphere has radius 0.5 with smooth unit normals", () => {
  const lat = 16;
  const lon = 24;
  const sphere = unitSphere(lat, lon);
  checkMeshInvariants(sphere, "sphere");
  assert.equal(sphere.positions.length, (lat + 1) * (lon + 1), "lat/lon grid vertex count");
  for (let i = 0; i < sphere.positions.length; i += 1) {
    const p = sphere.positions[i]!;
    assertClose(Math.sqrt(p.x * p.x + p.y * p.y + p.z * p.z), 0.5, "vertex on the r=0.5 shell", 1e-6);
    const n = sphere.normals[i]!;
    assertVecClose(v3(n.x * 0.5, n.y * 0.5, n.z * 0.5), p, "normal is the radial direction", 1e-6);
  }
});

test("unitSphere honors its default segment counts", () => {
  const sphere = unitSphere();
  checkMeshInvariants(sphere, "default sphere");
  assert.equal(sphere.positions.length, (16 + 1) * (24 + 1), "default 16×24 grid");
});

test("unitCylinderY spans radius 0.5 and height 1 around +Y", () => {
  const segments = 24;
  const cyl = unitCylinderY(segments);
  checkMeshInvariants(cyl, "cylinder");
  // side pairs + two caps (center + seam-duplicated ring each)
  assert.equal(cyl.positions.length, 2 * (segments + 1) + 2 * (segments + 2), "vertex count");
  let maxRadial = 0;
  for (const p of cyl.positions) {
    const radial = Math.sqrt(p.x * p.x + p.z * p.z);
    assert.ok(radial <= 0.5 + 1e-6, "radius ≤ 0.5");
    assert.ok(Math.abs(p.y) <= 0.5 + 1e-6, "height within ±0.5");
    assertClose(Math.abs(p.y), 0.5, "every vertex sits on the top or bottom rim/cap plane", 1e-6);
    maxRadial = Math.max(maxRadial, radial);
  }
  assertClose(maxRadial, 0.5, "wall reaches the full 0.5 radius", 1e-6);
  // Cap normals are flat ±Y; wall normals are horizontal.
  for (const n of cyl.normals) {
    const flat = Math.abs(Math.abs(n.y) - 1) < 1e-6 && Math.abs(n.x) < 1e-6 && Math.abs(n.z) < 1e-6;
    const radial = Math.abs(n.y) < 1e-6;
    assert.ok(flat || radial, "normal is a flat cap normal or a smooth radial wall normal");
  }
});

test("unitCylinderY honors its default segment count", () => {
  const cyl = unitCylinderY();
  checkMeshInvariants(cyl, "default cylinder");
  assert.equal(cyl.positions.length, 2 * (24 + 1) + 2 * (24 + 2), "default 24 segments");
});
