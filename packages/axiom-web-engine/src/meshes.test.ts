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

/**
 * The largest chord between ANGULARLY ADJACENT points on a cylinder's top rim —
 * the measurable stand-in for "how faceted does the silhouette look".
 *
 * Both filters matter. `y` alone also catches the top cap's CENTRE vertex (and
 * the cap ring duplicated behind the wall ring), so a naive scan measures a
 * centre-to-rim spoke of 0.5 at every segment count and reports a constant. The
 * radius filter drops the centre; sorting by angle then puts genuine neighbours
 * beside each other regardless of the order the builder emitted them in.
 */
const rimGap = (segments: number): number => {
  const rim = unitCylinderY(segments)
    .positions.filter((p) => Math.abs(p.y - 0.5) < 1e-6 && Math.abs(Math.hypot(p.x, p.z) - 0.5) < 1e-6)
    .map((p) => Math.atan2(p.z, p.x))
    .toSorted((a, b) => a - b);
  return Math.max(...rim.slice(1).map((theta, i) => 2 * 0.5 * Math.abs(Math.sin((theta - rim[i]!) / 2))));
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

test("unitSphere builds whatever ring counts the caller asks for", () => {
  const sphere = unitSphere(6, 9);
  checkMeshInvariants(sphere, "coarse sphere");
  assert.equal(sphere.positions.length, (6 + 1) * (9 + 1), "6×9 grid");
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

test("unitCylinderY scales its ring with the requested segment count", () => {
  const fine = unitCylinderY(72);
  checkMeshInvariants(fine, "fine cylinder");
  assert.equal(fine.positions.length, 2 * (72 + 1) + 2 * (72 + 2), "72 segments");
  // The whole point of the knob: more facets means a silhouette closer to a true
  // circle. The largest gap between neighbouring rim points shrinks with count.
  assert.ok(rimGap(72) < rimGap(12), "a finer ring has shorter chords");
});

/**
 * THE invariant every primitive must hold: each triangle's winding agrees with
 * its own vertex normals. Wound counter-clockwise seen from outside, a triangle's
 * right-hand-rule normal points the same way its vertices claim to.
 *
 * This is not pedantry about a convention — it is a rendering correctness bug
 * with a loud symptom. The WebGL2 backend draws with culling OFF and makes back
 * faces shade like front faces (`n = gl_FrontFacing ? n : -n`) so thin two-sided
 * meshes work. A primitive wound backwards makes every VISIBLE face report
 * back-facing, so that flip turns the correct outward normal inward, every
 * `max(dot(n, l), 0)` collapses to zero, and the surface renders on ambient alone
 * — flat and far too dark. Both curved-strip generators shipped inverted (the
 * whole sphere, and the cylinder's side wall while its caps were fine), which is
 * exactly how it presented: dark spheres, correct boxes.
 */
const assertWindingMatchesNormals = (mesh: MeshData, label: string): void => {
  const at = <T>(list: readonly T[], i: number): T => {
    const item = list[i];
    assert.ok(item !== undefined, `${label}: index ${i} in range`);
    return item;
  };
  /** For one triangle: the area its winding spans, and how much its right-hand-rule
   * normal agrees with the vertex normals (positive = agrees). */
  const facingAt = (tri: number): { readonly area: number; readonly facing: number; readonly tri: number } => {
    const ids = [at(mesh.indices, tri), at(mesh.indices, tri + 1), at(mesh.indices, tri + 2)];
    const a = at(mesh.positions, ids[0] ?? 0);
    const b = at(mesh.positions, ids[1] ?? 0);
    const c = at(mesh.positions, ids[2] ?? 0);
    const edge1 = v3(b.x - a.x, b.y - a.y, b.z - a.z);
    const edge2 = v3(c.x - a.x, c.y - a.y, c.z - a.z);
    const geo = v3(
      edge1.y * edge2.z - edge1.z * edge2.y,
      edge1.z * edge2.x - edge1.x * edge2.z,
      edge1.x * edge2.y - edge1.y * edge2.x,
    );
    const area = Math.sqrt(geo.x * geo.x + geo.y * geo.y + geo.z * geo.z);
    const normals = ids.map((id) => at(mesh.normals, id));
    const sum = v3(
      normals.reduce((total, n) => total + n.x, 0),
      normals.reduce((total, n) => total + n.y, 0),
      normals.reduce((total, n) => total + n.z, 0),
    );
    return { area, facing: (geo.x * sum.x + geo.y * sum.y + geo.z * sum.z) / Math.max(area, 1e-12), tri };
  };
  // A sphere's pole fans are degenerate slivers — zero area, so no facing at all
  // and nothing to agree with. Every triangle that actually covers a pixel must.
  const real = Array.from({ length: mesh.indices.length / 3 }, (unused, i) => facingAt(i * 3)).filter(
    (t) => t.area >= 1e-12,
  );
  real.forEach((t) =>
    assert.ok(t.facing > 0, `${label}: triangle at index ${t.tri} is wound against its own normals (facing ${t.facing})`),
  );
  assert.ok(real.length > 0, `${label}: actually checked some triangles`);
};

test("every primitive is wound counter-clockwise as seen from outside", () => {
  assertWindingMatchesNormals(unitBox(), "unitBox");
  assertWindingMatchesNormals(unitSphere(12, 16), "unitSphere");
  assertWindingMatchesNormals(unitSphere(3, 4), "unitSphere (coarse)");
  assertWindingMatchesNormals(unitCylinderY(16), "unitCylinderY");
  assertWindingMatchesNormals(unitCylinderY(5), "unitCylinderY (coarse)");
});
