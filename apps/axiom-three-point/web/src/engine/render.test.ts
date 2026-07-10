/*
 * engine/render.test.ts — `node --test` coverage for the renderer's PURE parts:
 * the column-major matrix math in `mat4.ts` (perspective clip range, lookAt
 * mapping the target axis onto -Z, T·R·S composition order, multiply against
 * hand-computed products) and the procedural unit primitives in `meshes.ts`
 * (vertex counts, index validity, unit-size bounds, unit normals). No DOM and
 * no WebGL — `renderer.ts` itself is exercised in the browser.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import type { EngineQuat, EngineVec3, MeshData } from "./api.ts";
import { AMBIENT } from "./backend.ts";
import { lambertLight } from "./backend-canvas2d.ts";
import { fromTrs, identity, lookAt, multiply, perspective, transformPoint } from "./mat4.ts";
import { unitBox, unitCylinderY, unitSphere } from "./meshes.ts";

const v3 = (x: number, y: number, z: number): EngineVec3 => ({ x, y, z });
const IDENTITY_QUAT: EngineQuat = [0, 0, 0, 1];

const assertClose = (actual: number, expected: number, msg: string, eps = 1e-5): void => {
  assert.ok(Math.abs(actual - expected) <= eps, `${msg}: expected ${expected}, got ${actual}`);
};

const assertVecClose = (actual: EngineVec3, expected: EngineVec3, msg: string, eps = 1e-5): void => {
  assertClose(actual.x, expected.x, `${msg} (x)`, eps);
  assertClose(actual.y, expected.y, `${msg} (y)`, eps);
  assertClose(actual.z, expected.z, `${msg} (z)`, eps);
};

// ── mat4 ──────────────────────────────────────────────────────────────────────

test("identity leaves points unchanged", () => {
  assertVecClose(transformPoint(identity(), v3(1, -2, 3)), v3(1, -2, 3), "identity transform");
});

test("perspective maps near to NDC -1, far to +1, and the fov edge to x=1", () => {
  // fovY = 90°, aspect 1, near 1, far 3: f = 1, so the entries are hand-computable.
  const p = perspective(Math.PI / 2, 1, 1, 3);
  assertClose(p[0]!, 1, "m[0] = f/aspect");
  assertClose(p[5]!, 1, "m[5] = f");
  assertClose(p[10]!, -2, "m[10] = (far+near)/(near-far)");
  assertClose(p[11]!, -1, "m[11]");
  assertClose(p[14]!, -3, "m[14] = 2·far·near/(near-far)");
  assertVecClose(transformPoint(p, v3(0, 0, -1)), v3(0, 0, -1), "near plane → NDC z=-1");
  assertVecClose(transformPoint(p, v3(0, 0, -3)), v3(0, 0, 1), "far plane → NDC z=+1");
  // At 90° fov and near=1, the frustum's right edge at the near plane is x=1.
  assertVecClose(transformPoint(p, v3(1, 0, -1)), v3(1, 0, -1), "fov edge → NDC x=1");
});

test("perspective guards degenerate inputs instead of producing NaN", () => {
  const p = perspective(0, 0, 1, 1);
  for (let i = 0; i < 16; i += 1) {
    assert.ok(Number.isFinite(p[i]!), `entry ${i} is finite`);
  }
});

test("lookAt maps the eye to the origin and the target onto -Z", () => {
  const view = lookAt(v3(0, 0, 5), v3(0, 0, 0), v3(0, 1, 0));
  assertVecClose(transformPoint(view, v3(0, 0, 5)), v3(0, 0, 0), "eye → origin");
  assertVecClose(transformPoint(view, v3(0, 0, 0)), v3(0, 0, -5), "target → (0,0,-5)");
  assertVecClose(transformPoint(view, v3(1, 0, 5)), v3(1, 0, 0), "camera-right stays +x");
  assertVecClose(transformPoint(view, v3(0, 1, 5)), v3(0, 1, 0), "camera-up stays +y");
});

test("lookAt looks down -Z from an off-axis eye too", () => {
  const view = lookAt(v3(3, 4, 5), v3(1, 1, 1), v3(0, 1, 0));
  const t = transformPoint(view, v3(1, 1, 1));
  const dist = Math.sqrt(4 + 9 + 16);
  assertClose(t.x, 0, "target view x");
  assertClose(t.y, 0, "target view y");
  assertClose(t.z, -dist, "target sits at -|eye-target| on Z");
});

test("fromTrs with identity rotation scales then translates", () => {
  const m = fromTrs(v3(1, 2, 3), IDENTITY_QUAT, v3(2, 2, 2));
  assertVecClose(transformPoint(m, v3(1, 1, 1)), v3(3, 4, 5), "scale 2 then translate (1,2,3)");
});

test("fromTrs applies translation last (origin always lands on position)", () => {
  const yaw90: EngineQuat = [0, Math.sin(Math.PI / 4), 0, Math.cos(Math.PI / 4)];
  const m = fromTrs(v3(5, -7, 2), yaw90, v3(3, 4, 5));
  assertVecClose(transformPoint(m, v3(0, 0, 0)), v3(5, -7, 2), "origin → position");
});

test("fromTrs composes T·R·S: rotate the scaled local point, then translate", () => {
  // +90° about Y maps +X to -Z. Local (1,0,0) scaled by 3 → (3,0,0), rotated →
  // (0,0,-3), translated by (5,0,0) → (5,0,-3).
  const yaw90: EngineQuat = [0, Math.sin(Math.PI / 4), 0, Math.cos(Math.PI / 4)];
  const m = fromTrs(v3(5, 0, 0), yaw90, v3(3, 1, 1));
  assertVecClose(transformPoint(m, v3(1, 0, 0)), v3(5, 0, -3), "T(R(S(p)))");
});

test("multiply matches the hand-computed product of translate × scale", () => {
  const t = fromTrs(v3(1, 2, 3), IDENTITY_QUAT, v3(1, 1, 1));
  const s = fromTrs(v3(0, 0, 0), IDENTITY_QUAT, v3(2, 3, 4));
  const ts = multiply(t, s); // scale first, then translate
  const expected = [2, 0, 0, 0, 0, 3, 0, 0, 0, 0, 4, 0, 1, 2, 3, 1];
  expected.forEach((value, i) => assertClose(ts[i]!, value, `T·S entry ${i}`));
  const st = multiply(s, t); // translate first, then scale
  const expectedSt = [2, 0, 0, 0, 0, 3, 0, 0, 0, 0, 4, 0, 2, 6, 12, 1];
  expectedSt.forEach((value, i) => assertClose(st[i]!, value, `S·T entry ${i}`));
  assertVecClose(transformPoint(ts, v3(1, 1, 1)), v3(3, 5, 7), "T·S applied to a point");
});

// ── mesh helpers ──────────────────────────────────────────────────────────────

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

// ── the software backend's lighting matches the WebGL2 shader model ───────────

test("lambertLight reproduces the shared shading model", () => {
  // No lights: pure ambient floor.
  const dark = lambertLight(0, 1, 0, 0, 0, 0, { dirLights: [], pointLights: [] });
  assert.deepEqual(dark, [AMBIENT, AMBIENT, AMBIENT]);

  // A directional light traveling straight down fully lights an up-facing
  // triangle: ambient + color·intensity, per channel.
  const sun = lambertLight(0, 1, 0, 0, 0, 0, {
    dirLights: [{ color: [1, 0.5, 0.25], direction: [0, -1, 0] }],
    pointLights: [],
  });
  assertClose(sun[0], AMBIENT + 1, "sun r");
  assertClose(sun[1], AMBIENT + 0.5, "sun g");
  assertClose(sun[2], AMBIENT + 0.25, "sun b");

  // The same light from behind contributes nothing (max(0, ·)).
  const back = lambertLight(0, -1, 0, 0, 0, 0, {
    dirLights: [{ color: [1, 1, 1], direction: [0, -1, 0] }],
    pointLights: [],
  });
  assert.deepEqual(back, [AMBIENT, AMBIENT, AMBIENT]);

  // A point light 2 m overhead: lambert 1, falloff 1/(1+0.08·4) — the exact
  // WebGL2 fragment expression.
  const point = lambertLight(0, 1, 0, 0, 0, 0, {
    dirLights: [],
    pointLights: [{ color: [1, 1, 1], position: [0, 2, 0] }],
  });
  assertClose(point[0], AMBIENT + 1 / (1 + 0.08 * 4), "point falloff");

  // Deterministic: identical inputs, identical output.
  const again = lambertLight(0, 1, 0, 0, 0, 0, {
    dirLights: [],
    pointLights: [{ color: [1, 1, 1], position: [0, 2, 0] }],
  });
  assert.deepEqual(point, again);
});
