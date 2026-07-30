/*
 * mat4.test.ts — `node --test` coverage for the column-major matrix math in
 * `mat4.ts`: perspective clip range, lookAt mapping the target axis onto -Z,
 * T·R·S composition order, multiply against hand-computed products, the
 * perspective-divide guard, and the degenerate-input fallbacks (zero-length
 * normalize, near-zero clip w). Assertions ported from the reference
 * renderer's render.test.ts, extended to drive every branchless arm. No DOM and
 * no WebGL.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import type { EngineQuat, EngineVec3 } from "./api.ts";
import { type Mat3, type Mat4, fromTrs, identity, lookAt, multiply, normalMatrix, perspective, transformPoint } from "./mat4.ts";

const v3 = (x: number, y: number, z: number): EngineVec3 => ({ x, y, z });
const IDENTITY_QUAT: EngineQuat = [0, 0, 0, 1];

// ── vector/mat3 helpers for the normal-matrix tests ──────────────────────────

const dot = (a: EngineVec3, b: EngineVec3): number => a.x * b.x + a.y * b.y + a.z * b.z;

const cross = (a: EngineVec3, b: EngineVec3): EngineVec3 =>
  v3(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);

const normalize = (v: EngineVec3): EngineVec3 => {
  const len = Math.sqrt(dot(v, v));
  return v3(v.x / len, v.y / len, v.z / len);
};

/** Apply a column-major 3×3 (`m[col * 3 + row]`) to a vector. */
const applyMat3 = (m: Mat3, v: EngineVec3): EngineVec3 =>
  v3(
    m[0]! * v.x + m[3]! * v.y + m[6]! * v.z,
    m[1]! * v.x + m[4]! * v.y + m[7]! * v.z,
    m[2]! * v.x + m[5]! * v.y + m[8]! * v.z,
  );

/** The model matrix's own upper-left 3×3 — the basis that carries TANGENTS, and
 * the transform the renderer used to (wrongly) put normals through. */
const upper3x3 = (m: Mat4): Mat3 => new Float32Array([m[0]!, m[1]!, m[2]!, m[4]!, m[5]!, m[6]!, m[8]!, m[9]!, m[10]!]);

const assertClose = (actual: number, expected: number, msg: string, eps = 1e-5): void => {
  assert.ok(Math.abs(actual - expected) <= eps, `${msg}: expected ${expected}, got ${actual}`);
};

const assertVecClose = (actual: EngineVec3, expected: EngineVec3, msg: string, eps = 1e-5): void => {
  assertClose(actual.x, expected.x, `${msg} (x)`, eps);
  assertClose(actual.y, expected.y, `${msg} (y)`, eps);
  assertClose(actual.z, expected.z, `${msg} (z)`, eps);
};

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

test("lookAt falls back on a degenerate eye/target and a degenerate up", () => {
  // eye == target: the forward vector has zero length → fallback (0,0,-1). up
  // parallel to forward: cross(fwd, up) is zero → the side fallback (1,0,0). The
  // result must stay finite (no 0·Infinity NaN from the branchless blend).
  const view = lookAt(v3(2, 2, 2), v3(2, 2, 2), v3(0, 1, 0));
  for (let i = 0; i < 16; i += 1) {
    assert.ok(Number.isFinite(view[i]!), `degenerate lookAt entry ${i} finite`);
  }
  const up = lookAt(v3(0, 0, 0), v3(0, 1, 0), v3(0, 1, 0));
  for (let i = 0; i < 16; i += 1) {
    assert.ok(Number.isFinite(up[i]!), `parallel-up lookAt entry ${i} finite`);
  }
  // forward is +Y, side falls back to +X, so world +X stays +X in view space.
  assertClose(up[0]!, 1, "degenerate side falls back to +X");
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

/**
 * The defining property of a normal matrix, and the one the renderer used to get
 * wrong: a normal stays PERPENDICULAR TO THE SURFACE after transformation. A
 * tangent is carried by the model basis, a normal by its inverse transpose, and
 * only then does `dot(N, T)` stay zero. Asserted on the exact shape that exposed
 * the bug — a sphere squashed on one axis, the crab body's `(0.62, 0.4, 0.5)`.
 */
test("normalMatrix keeps normals perpendicular to the surface under non-uniform scale", () => {
  const scale = v3(0.62, 0.4, 0.5);
  const model = fromTrs(v3(1, 2, 3), IDENTITY_QUAT, scale);
  const nrm = normalMatrix(model);
  // A point on the unit sphere and two independent tangents there (any vectors
  // perpendicular to the normal — on a sphere the normal IS the position).
  const n = normalize(v3(1, 1, 0.5));
  const tangents = [normalize(cross(n, v3(0, 0, 1))), normalize(cross(n, v3(0, 1, 0)))];
  const worldNormal = normalize(applyMat3(nrm, n));
  tangents.forEach((t, i) => {
    // Tangents ride the model basis (they are directions ON the surface).
    const worldTangent = normalize(applyMat3(upper3x3(model), t));
    assertClose(dot(worldNormal, worldTangent), 0, `transformed normal ⟂ transformed tangent ${i}`);
    // And the naive transform this replaced does NOT hold that property — which
    // is the whole bug: the normal ends up tilted off the surface, so the
    // Lambert term reads the wrong angle to the light.
    const naive = normalize(applyMat3(upper3x3(model), n));
    assert.ok(Math.abs(dot(naive, worldTangent)) > 0.1, `the naive upper-3x3 normal is NOT perpendicular (tangent ${i})`);
  });
});

test("normalMatrix agrees with the model basis for a rotation and a uniform scale", () => {
  // The case where the two transforms coincide (up to length, which the shader
  // normalizes away): a rigid rotation times a uniform scale.
  const rotation: EngineQuat = [0, Math.sin(Math.PI / 8), 0, Math.cos(Math.PI / 8)]; // 45° about +Y
  const model = fromTrs(v3(-2, 5, 0), rotation, v3(3, 3, 3));
  const n = normalize(v3(0.3, -0.6, 0.74));
  assertVecClose(
    normalize(applyMat3(normalMatrix(model), n)),
    normalize(applyMat3(upper3x3(model), n)),
    "uniform scale: cofactor and upper-3x3 point the same way",
  );
});

test("normalMatrix stays finite when a scale axis is flattened to zero", () => {
  // Division-free by construction (a cofactor, not a literal inverse), so a
  // degenerate basis yields a zero column rather than Infinity/NaN — the reason
  // this needs no branchless guard of its own.
  const nrm = normalMatrix(fromTrs(v3(0, 0, 0), IDENTITY_QUAT, v3(2, 0, 3)));
  assert.ok([...nrm].every((value) => Number.isFinite(value)), `finite entries, got ${[...nrm].join(", ")}`);
  // The surviving axes still describe the flattened plane's own normal: X and Z
  // are scaled, so the Y column (their cross product) is the only nonzero one.
  assertVecClose(applyMat3(nrm, v3(0, 1, 0)), v3(0, 6, 0), "the flattened plane's normal survives");
  assertVecClose(applyMat3(nrm, v3(1, 0, 1)), v3(0, 0, 0), "the collapsed directions carry no normal");
});

test("transformPoint guards a near-zero clip w (falls back to inv = 1)", () => {
  // A z=0 point through the perspective matrix has clip w = -z = 0; the guard
  // keeps inv = 1 rather than dividing by zero, so x/y/z pass through unscaled.
  const p = perspective(Math.PI / 2, 1, 1, 3);
  const out = transformPoint(p, v3(2, 3, 0));
  assert.ok(Number.isFinite(out.x) && Number.isFinite(out.y) && Number.isFinite(out.z), "finite output");
  assertVecClose(out, v3(2, 3, -3), "w=0 → inv=1, components pass through");
});
